import asyncio
import base64
import json
from pathlib import Path
import urllib.request

import websockets


ROOT = Path(__file__).resolve().parents[2]


async def main():
    targets = json.load(urllib.request.urlopen('http://127.0.0.1:9224/json/list'))
    target = next(item for item in targets if ':8765' in item['url'])
    async with websockets.connect(target['webSocketDebuggerUrl'], max_size=16000000) as socket:
        sequence = 0
        errors = []

        async def call(method, params=None):
            nonlocal sequence
            sequence += 1
            await socket.send(json.dumps({'id': sequence, 'method': method, 'params': params or {}}))
            while True:
                response = json.loads(await socket.recv())
                if response.get('method') == 'Runtime.exceptionThrown':
                    errors.append(response['params'])
                if response.get('id') == sequence:
                    assert 'error' not in response, response
                    return response.get('result', {})

        async def evaluate(expression):
            response = await call('Runtime.evaluate', {'expression': expression, 'returnByValue': True, 'awaitPromise': True})
            assert 'exceptionDetails' not in response, response
            return response['result'].get('value')

        async def wait(expression):
            for _ in range(100):
                if await evaluate(expression):
                    return
                await asyncio.sleep(.1)
            raise AssertionError(expression)

        async def line(command):
            await evaluate('document.getElementById("line-input").value=' + json.dumps(command) + ';document.getElementById("line-form").requestSubmit()')

        async def layout(width, height, name):
            await call('Emulation.setDeviceMetricsOverride', {'width': width, 'height': height, 'deviceScaleFactor': 1, 'mobile': True})
            await call('Emulation.setPageScaleFactor', {'pageScaleFactor': 1})
            await evaluate('window.dispatchEvent(new Event("resize"))')
            await asyncio.sleep(.5)
            value = await evaluate('(() => {const box=id=>{const r=document.getElementById(id).getBoundingClientRect();return {x:r.x,y:r.y,width:r.width,height:r.height,bottom:r.bottom,right:r.right}};return {width:innerWidth,overflow:document.documentElement.scrollWidth>innerWidth,terminal:box("terminal"),inspector:box("inspector"),window:document.querySelector(".terminal-window").getBoundingClientRect().toJSON(),live:document.getElementById("telemetry-status").textContent}})()')
            assert not value['overflow'], value
            assert value['terminal']['height'] >= 75, value
            assert value['inspector']['width'] > 200, value
            if value['width'] > 760:
                assert value['inspector']['x'] > value['terminal']['x'], value
            else:
                assert value['inspector']['y'] > value['terminal']['y'], value
            if width <= 900:
                capture = await call('Page.captureScreenshot', {'format': 'png', 'captureBeyondViewport': False})
                (ROOT / 'evidence' / f'telemetry-{name}.png').write_bytes(base64.b64decode(capture['data']))
            return value

        await call('Page.bringToFront')
        await call('Runtime.enable')
        await call('Page.reload', {'ignoreCache': True})
        await wait('document.getElementById("telemetry-status")?.textContent === "Live"')
        session = await evaluate('localStorage.getItem("devtool-terminal-session")')
        await wait('!document.querySelector("[data-command]").disabled')
        await line('devtool --version; devtool-auto --version; devtool-proof --version')
        await wait('document.querySelector(".history-entry code")?.textContent.includes("devtool-proof --version") && document.querySelector(".history-result")?.textContent.includes("exit 0")')
        assert await evaluate('document.querySelector(".history-executables").textContent') == 'devtool → devtool-auto → devtool-proof'
        await line('false')
        await wait('document.querySelector(".history-entry[data-failed=true]")?.querySelector("code").textContent === "false"')
        await line('devtool config menu')
        await wait('document.getElementById("telemetry-mode").textContent === "Command draait" && [...document.querySelectorAll(".engine-row[data-active=true] .engine-name")].some(e=>e.textContent==="devtool")')
        layouts = [await layout(1440, 1000, 'desktop-wide'), await layout(900, 900, 'desktop-active')]
        await line('q')
        await wait('document.getElementById("telemetry-mode").textContent === "Prompt gereed"')
        await line("printf '%s' '<long-command-" + 'x' * 180 + "> 漢字' > /dev/null")
        await wait('document.querySelector(".history-entry code")?.textContent.includes("long-command-") && document.querySelector(".history-result").textContent.includes("exit 0")')
        for width, height, name in [(1024, 768, 'tablet'), (390, 844, 'mobile'), (320, 740, 'narrow'), (640, 450, 'landscape'), (720, 500, 'zoom-200')]:
            layouts.append(await layout(width, height, name))
        await evaluate('document.getElementById("files-tab").click()')
        assert await evaluate('!document.getElementById("file-panel").hidden && document.getElementById("telemetry-panel").hidden')
        await evaluate('document.getElementById("telemetry-tab").click()')
        await evaluate('document.getElementById("files-toggle").click()')
        assert await evaluate('getComputedStyle(document.getElementById("inspector")).display === "none"')
        await evaluate('document.getElementById("files-toggle").click()')
        await evaluate('document.getElementById("telemetry-tab").focus()')
        await call('Input.dispatchKeyEvent', {'type': 'keyDown', 'key': 'Tab', 'code': 'Tab', 'windowsVirtualKeyCode': 9})
        await call('Input.dispatchKeyEvent', {'type': 'keyUp', 'key': 'Tab', 'code': 'Tab', 'windowsVirtualKeyCode': 9})
        assert await evaluate('document.activeElement.id === "files-tab"')
        await call('Network.enable')
        await call('Network.emulateNetworkConditions', {'offline': True, 'latency': 0, 'downloadThroughput': -1, 'uploadThroughput': -1})
        await wait('document.getElementById("telemetry-status").textContent === "Geen verbinding"')
        await call('Network.emulateNetworkConditions', {'offline': False, 'latency': 0, 'downloadThroughput': -1, 'uploadThroughput': -1})
        await wait('document.getElementById("telemetry-status").textContent === "Live"')
        await call('Page.reload')
        await wait('document.querySelector(".history-entry code")?.textContent.includes("long-command-")')
        assert await evaluate('localStorage.getItem("devtool-terminal-session")') == session
        await call('Emulation.setDeviceMetricsOverride', {'width': 1440, 'height': 1000, 'deviceScaleFactor': 1, 'mobile': True})
        await call('Emulation.setEmulatedMedia', {'features': [{'name': 'prefers-reduced-motion', 'value': 'reduce'}]})
        assert await evaluate('matchMedia("(prefers-reduced-motion: reduce)").matches')
        await call('Emulation.setEmulatedMedia', {'features': []})
        assert not errors, errors
        evidence = {'session': session, 'provider': 'direct CDP after REA invalid_endpoint_response', 'checks': ['actual CLI names and shell exit codes', 'foreground menu and background Watch', 'long Unicode commands', 'file and telemetry navigation', 'pane collapse', 'keyboard focus', 'offline and reconnect', 'reload retains history', 'reduced motion'], 'layouts': layouts, 'page_errors': errors, 'limitations': ['Android screenshot compositor tiles captures wider than its physical surface; captures retained only up to 900 pixels. Wider layouts verified through DOM geometry. No screen reader tested.']}
        (ROOT / 'evidence/telemetry-browser-check.json').write_text(json.dumps(evidence, indent=2) + '\n')
        print(json.dumps(evidence))


if __name__ == '__main__':
    asyncio.run(main())
