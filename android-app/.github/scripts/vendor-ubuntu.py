import hashlib
import json
from pathlib import Path
import urllib.request

root = Path(__file__).resolve().parents[2]
manifest = json.loads((root / 'assets/linux/ubuntu-base.json').read_text())
archive = root / 'assets' / manifest['asset']
archive.parent.mkdir(parents=True, exist_ok=True)
if not archive.exists():
    with urllib.request.urlopen(manifest['url'], timeout=60) as source, archive.open('wb') as target:
        while chunk := source.read(1024 * 1024):
            target.write(chunk)
with archive.open('rb') as source:
    digest = hashlib.file_digest(source, 'sha256').hexdigest()
if digest != manifest['sha256']:
    raise RuntimeError('Ubuntu Base archive does not match the pinned source')
print(json.dumps({'asset': manifest['asset'], 'bytes': archive.stat().st_size, 'sha256': digest}))
