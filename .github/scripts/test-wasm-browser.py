import os
from pathlib import Path
import shutil
import subprocess
import sys
import time
import urllib.request
from playwright.sync_api import sync_playwright

root = Path(__file__).resolve().parents[2]
binaries = Path(os.environ['CARGO_TARGET_DIR']) / os.environ['DEVTOOLS_TARGET'] / 'release'
for name in ['devtool', 'devtool-auto', 'devtool-proof', 'devtools-watch']:
    shutil.copy2(binaries / name, root / 'devtool/dist' / name)
log = root / '.build/wasm-browser-server.log'
with log.open('w') as output:
    server = subprocess.Popen([sys.executable, root / 'devtool/demo/server.py', '--port', '8766'], stdout=output, stderr=subprocess.STDOUT)
    try:
        for attempt in range(100):
            try:
                urllib.request.urlopen('http://127.0.0.1:8766/api/health', timeout=1).close()
                break
            except OSError:
                time.sleep(.1)
        else:
            raise RuntimeError(log.read_text())
        with sync_playwright() as playwright:
            browser = playwright.chromium.launch(headless=True, args=['--remote-debugging-port=9222'])
            try:
                page = browser.new_page()
                page.goto('http://127.0.0.1:8766')
                subprocess.run([sys.executable, root / 'devtool/ui/tests/browser.py', '--endpoint', 'http://127.0.0.1:9222', '--evidence', root / '.build/wasm-browser-evidence'], check=True)
            finally:
                browser.close()
    finally:
        server.terminate()
        server.wait(timeout=15)
