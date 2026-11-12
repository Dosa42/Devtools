import argparse
import asyncio
import base64
import json
from pathlib import Path
import time
import urllib.request
import websockets


async def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--endpoint', default='http://127.0.0.1:9224')
    parser.add_argument('--url', default='http://127.0.0.1:8766')
    parser.add_argument('--evidence', type=Path, required=True)
    args = parser.parse_args()
    args.evidence.mkdir(parents=True, exist_ok=True)
    with urllib.request.urlopen(args.endpoint + '/json/list') as source:
        targets = json.load(source)
    target = next(target for target in targets if target['url'].startswith(('http://127.0.0.1:8765', 'http://127.0.0.1:8766')))
    counter = 0
    events = []
    async with websockets.connect(target['webSocketDebuggerUrl'], max_size=16 * 1024 * 1024) as socket:
        async def call(method, params=None):
            nonlocal counter
            counter += 1
            identifier = counter
            await socket.send(json.dumps({'id': identifier, 'method': method, 'params': params or {}}))
            while True:
                message = json.loads(await socket.recv())
                if message.get('id') == identifier:
                    if 'error' in message:
                        raise RuntimeError(message['error'])
                    return message.get('result', {})
                events.append(message)

        async def evaluate(expression):
            result = await call('Runtime.evaluate', {'expression': expression, 'returnByValue': True, 'awaitPromise': True})
            if result.get('exceptionDetails'):
                raise RuntimeError(result['exceptionDetails'])
            return result.get('result', {}).get('value')

        async def wait(expression, seconds=15):
            deadline = time.monotonic() + seconds
            while time.monotonic() < deadline:
                result = await evaluate(expression)
                if result:
                    return result
                await asyncio.sleep(.15)
            raise AssertionError(expression)

        async def command(text):
            await evaluate("document.getElementById('line-input').value=" + json.dumps(text) + ";document.getElementById('line-form').requestSubmit();true")

        await call('Runtime.enable')
        await call('Network.enable')
        await call('Page.enable')
        await call('Page.navigate', {'url': args.url})
        await wait("document.getElementById('wasm-root')?.dataset.ready==='true'")
        await wait("document.querySelector('[data-command=\"devtool -D\"]')?.disabled===false")
        await evaluate("document.getElementById('new-session').click();true")
        await wait("document.querySelector('[data-command=\"devtool -D\"]')?.disabled===false")
        initial = await evaluate("({runtime:document.getElementById('wasm-root').dataset.runtime,engines:document.querySelectorAll('.engine-row').length,session:document.getElementById('session-label').title})")
        assert initial['runtime'] == 'rust-wasm'
        await evaluate("document.querySelector('[data-command=\"devtool -D\"]').click();true")
        await command('')
        await wait("document.getElementById('run-status').textContent==='run-nr.001'")
        await command('devtool --canonical-tree')
        await wait("document.getElementById('file-count').textContent==='17 bestanden'")
        await wait("document.querySelectorAll('.engine-row').length===4")
        await wait("document.querySelector('#telemetry-history').textContent.includes('devtool --canonical-tree')")
        await evaluate("document.getElementById('files-tab').click();true")
        await evaluate("[...document.querySelectorAll('[data-file]')].find(b=>b.dataset.file.endsWith('/state.txt')).click();true")
        await wait("document.getElementById('preview-content').textContent.includes('canonical_tree: true')")
        preview = await evaluate("document.getElementById('preview-content').textContent")
        await evaluate("document.getElementById('close-preview').click();document.getElementById('telemetry-tab').click();true")
        await command("printf 'wasm-pipe' | devtool file create 'wasm note.txt' --stdin")
        await wait("document.querySelector('[data-file=\"wasm note.txt\"]')!==null")
        await command('devtool-auto --help')
        await wait("document.querySelector('#telemetry-history').textContent.includes('devtool-auto --help')")
        await command("devtool-proof hash 'wasm note.txt'")
        await wait("document.querySelector('#telemetry-history').textContent.includes('devtool-proof hash')")
        await command('devtools-watch --config')
        await wait("document.querySelector('#telemetry-history').textContent.includes('devtools-watch --config')")
        await evaluate("document.getElementById('compression-toggle').click();document.getElementById('compression-source').value=\"folder's name\";document.getElementById('compression-format').value='tar.gz';document.getElementById('compression-source').dispatchEvent(new Event('input',{bubbles:true}));true")
        compression = await evaluate("document.getElementById('compression-preview').textContent")
        assert "'folder'\\''s name'" in compression
        await evaluate("document.getElementById('compression-toggle').click();document.getElementById('catalog-toggle').click();true")
        assert await evaluate("document.querySelectorAll('#catalog-panel [data-command]').length>10")
        await evaluate("document.getElementById('catalog-toggle').click();true")
        widths = []
        for width, height in [(320,800),(390,844),(720,900),(900,700),(1440,900)]:
            await call('Emulation.setDeviceMetricsOverride', {'width':width, 'height':height, 'deviceScaleFactor':1, 'mobile':True})
            await call('Emulation.setPageScaleFactor', {'pageScaleFactor':1})
            await asyncio.sleep(.3)
            layout = await evaluate("({width:innerWidth,scroll:document.documentElement.scrollWidth,terminal:document.getElementById('terminal').getBoundingClientRect().width,inspector:document.getElementById('inspector').getBoundingClientRect().toJSON(),panel:document.getElementById('terminal').getBoundingClientRect().toJSON()})")
            assert layout['scroll'] <= layout['width'], layout
            assert layout['terminal'] > 100, layout
            widths.append(layout)
            if width in [390,900]:
                screenshot = await call('Page.captureScreenshot', {'format':'png','captureBeyondViewport':False})
                (args.evidence / f'wasm-ui-{width}.png').write_bytes(base64.b64decode(screenshot['data']))
        saved = await evaluate("localStorage.getItem('devtool-terminal-session')")
        await call('Page.reload', {'ignoreCache':True})
        await wait("document.getElementById('wasm-root')?.dataset.ready==='true'")
        assert await evaluate("localStorage.getItem('devtool-terminal-session')") == saved
        await wait("document.getElementById('file-count').textContent==='18 bestanden'")
        await command('devtool config menu')
        await wait("document.getElementById('telemetry-current-command').textContent==='devtool config menu'")
        await command('5')
        await command('2')
        await command('s')
        await wait("[...document.querySelectorAll('#settings-values .setting')].some(s=>s.querySelector('span')?.textContent==='level' && s.querySelector('b')?.textContent==='2')")
        await call('Network.emulateNetworkConditions', {'offline':True,'latency':0,'downloadThroughput':-1,'uploadThroughput':-1})
        await wait("document.getElementById('connection').textContent==='Opnieuw verbinden…'")
        await call('Network.emulateNetworkConditions', {'offline':False,'latency':0,'downloadThroughput':-1,'uploadThroughput':-1})
        await wait("document.getElementById('connection').textContent==='Verbonden'")
        await call('Emulation.setEmulatedMedia', {'features':[{'name':'prefers-reduced-motion','value':'reduce'}]})
        await call('Emulation.setDeviceMetricsOverride', {'width':900,'height':700,'deviceScaleFactor':1,'mobile':True})
        await call('Page.reload', {'ignoreCache':True})
        await wait("document.getElementById('wasm-root')?.dataset.ready==='true'")
        assert await evaluate("matchMedia('(prefers-reduced-motion: reduce)').matches")
        network = [event['params']['response'] for event in events if event.get('method')=='Network.responseReceived' and '.wasm' in event['params']['response']['url']]
        assert any(response['mimeType']=='application/wasm' and response['status']==200 for response in network), network
        exceptions = [event for event in events if event.get('method')=='Runtime.exceptionThrown']
        assert not exceptions, exceptions
        result = {'url':args.url, 'initial':initial, 'state_preview':preview, 'compression':compression, 'layouts':widths, 'wasm_network':network, 'runtime_exceptions':exceptions, 'checks':['Rust/WASM mount','header command through xterm and PTY','canonical tree 17 files','state.txt preview','Bash pipe','four native executable command history','compression quoting','CLI catalog','320/390/720/900/1440 reflow','session reload','interactive configuration menu','offline recovery','reduced motion']}
        (args.evidence/'wasm-browser.json').write_text(json.dumps(result,indent=2)+'\n')
        print(json.dumps({'checks':len(result['checks']),'wasm_responses':len(network),'exceptions':len(exceptions),'session':saved}))


if __name__ == '__main__':
    asyncio.run(main())
