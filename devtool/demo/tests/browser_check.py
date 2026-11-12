import asyncio
import base64
import json
import urllib.request
from pathlib import Path

import websockets

ROOT = Path(__file__).resolve().parents[2]

async def main():
    targets = json.load(urllib.request.urlopen('http://127.0.0.1:9224/json/list'))
    target = next(item for item in targets if ':8765' in item['url'])
    async with websockets.connect(target['webSocketDebuggerUrl'], max_size=16000000) as socket:
        sequence = 0
        async def call(method, params=None):
            nonlocal sequence
            sequence += 1
            await socket.send(json.dumps({'id': sequence, 'method': method, 'params': params or {}}))
            while True:
                response = json.loads(await socket.recv())
                if response.get('id') == sequence:
                    assert 'error' not in response, response
                    return response['result']
        async def evaluate(expression):
            result = await call('Runtime.evaluate', {'expression': expression, 'returnByValue': True, 'awaitPromise': True})
            assert 'exceptionDetails' not in result, result
            return result['result'].get('value')
        async def wait(expression):
            for attempt in range(80):
                if await evaluate(expression):
                    return
                await asyncio.sleep(.2)
            raise AssertionError(expression)
        async def line(command):
            await evaluate('document.getElementById("line-input").value=' + json.dumps(command) + ';document.getElementById("line-form").requestSubmit()')
            await asyncio.sleep(.4)
        previous = await evaluate('localStorage.getItem("devtool-terminal-session")')
        await evaluate('document.getElementById("new-session").click()')
        await wait('localStorage.getItem("devtool-terminal-session") !== ' + json.dumps(previous))
        await asyncio.sleep(.5)
        await wait('!document.querySelector("[data-command]").disabled')
        await evaluate('Array.from(document.querySelectorAll("[data-command]")).find(e=>e.dataset.command.trim()==="devtool -D").click()')
        await asyncio.sleep(.3)
        assert await evaluate('document.getElementById("run-status").textContent') == 'Nog geen hoofdmap'
        await line('')
        await wait('document.getElementById("run-status").textContent === "run-nr.001"')
        await wait('!document.querySelector("[data-command]").disabled')
        await line('devtool --canonical-tree')
        await wait('document.getElementById("file-count").textContent === "17 bestanden"')
        await line('devtool config menu')
        await asyncio.sleep(.5)
        for value in ['5', '2', 's']:
            await line(value)
        await wait('Array.from(document.querySelectorAll(".setting")).some(x => x.textContent === "level2")')
        root = await evaluate('document.querySelector(".file-row.directory").title')
        await line('devtool compress ' + root + ' --format 7z -o demo.7z')
        await wait('document.body.innerText.includes("demo.7z")')
        await line('devtool compress ' + root + ' --format tar.gz -o demo.tar.gz')
        await wait('document.getElementById("file-count").textContent === "19 bestanden"')
        session = await evaluate('localStorage.getItem("devtool-terminal-session")')
        await call('Page.reload')
        await wait('document.getElementById("file-count")?.textContent === "19 bestanden"')
        assert await evaluate('localStorage.getItem("devtool-terminal-session")') == session
        layouts = []
        for width, height in [(1280, 900), (390, 844), (320, 740), (640, 450)]:
            await call('Emulation.setDeviceMetricsOverride', {'width': width, 'height': height, 'deviceScaleFactor': 1, 'mobile': width <= 650})
            await call('Emulation.setPageScaleFactor', {'pageScaleFactor': 1})
            await asyncio.sleep(.7)
            layout = await evaluate('({width:innerWidth,overflow:document.documentElement.scrollWidth>innerWidth,terminalHeight:document.getElementById("terminal").getBoundingClientRect().height,headers:Array.from(document.querySelectorAll(".command-header,.settings-header")).map(e=>({top:e.getBoundingClientRect().top,bottom:e.getBoundingClientRect().bottom}))})')
            assert not layout['overflow'], layout
            assert layout['terminalHeight'] > 80, layout
            layouts.append(layout)
            capture = await call('Page.captureScreenshot', {'format': 'png'})
            (ROOT / 'evidence' / f'demo-{width}.png').write_bytes(base64.b64decode(capture['data']))
        await call('Emulation.clearDeviceMetricsOverride')
        evidence = {'session': session, 'checks': ['header inserts without executing', 'native directory run001', 'canonical17files', 'interactive settings level2', '7z', 'tar.gz', 'reload retains session'], 'layouts': layouts, 'provider': 'direct CDP fallback after REA unreadable_output'}
        (ROOT / 'evidence' / 'demo-browser-check.json').write_text(json.dumps(evidence, indent=2) + '\n')
        print(json.dumps(evidence))

asyncio.run(main())
