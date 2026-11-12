import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tarfile

root = Path(__file__).resolve().parents[2]
output = root / '.build/wasm-ui'
output.mkdir(parents=True, exist_ok=True)
browser = output / 'browser'
browser.mkdir(exist_ok=True)
for name in ['index.html', 'app.js', 'terminal.js', 'style.css', 'favicon.svg', 'vendor', 'wasm']:
    source = root / 'devtool/demo' / name
    if source.is_dir():
        shutil.copytree(source, browser / name, dirs_exist_ok=True)
    else:
        shutil.copy2(source, browser / name)
archive = output / 'devtools-wasm-ui.tar.gz'
with tarfile.open(archive, 'w:gz') as target:
    target.add(browser, arcname='devtool/demo')
manifest = json.loads((browser / 'wasm/manifest.json').read_text())
manifest['source_commit'] = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=root, text=True).strip()
manifest['archive'] = archive.name
manifest['archive_sha256'] = hashlib.sha256(archive.read_bytes()).hexdigest()
(output / 'manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
(output / 'README.md').write_text('# Devtools Rust/WASM UI\n\nPak devtools-wasm-ui.tar.gz uit in de hoofdmap van een Devtools-installatie. De UI gebruikt de localhostserver en vier native CLI’s van die installatie. Browserassets zijn voor alle platformen gelijk.\n')
(output / 'SHA256SUMS').write_text(''.join(hashlib.sha256(path.read_bytes()).hexdigest() + '  ' + path.relative_to(output).as_posix() + '\n' for path in sorted(output.rglob('*')) if path.is_file() and path != output / 'SHA256SUMS'))
print(json.dumps({'output': str(output), 'source_commit': manifest['source_commit'], 'wasm_bytes': (browser / 'wasm/devtools_ui_bg.wasm').stat().st_size}))
