import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess


ROOT = Path(__file__).resolve().parents[2]
PLATFORMS = ['ubuntu-24.04-x86_64', 'ubuntu-24.04-arm64', 'macos-15-x86_64', 'macos-15-arm64']


def git(*arguments):
    return subprocess.run(['git', *arguments], cwd=ROOT, check=True, capture_output=True, text=True)


def main():
    incoming = Path(os.environ['DEVTOOLS_ARTIFACTS'])
    revision = os.environ['DEVTOOLS_SOURCE_SHA']
    destination = ROOT / 'all-dist'
    destination.mkdir(exist_ok=True)
    records = []
    for platform in PLATFORMS:
        source = incoming / f'devtools-{platform}'
        record = json.loads((source / 'manifest.json').read_text())
        if record['platform'] != platform or record['source_commit'] != revision:
            raise RuntimeError('Artifact identity does not match the build')
        for line in (source / 'SHA256SUMS').read_text().splitlines():
            expected, relative = line.split('  ', 1)
            path = source / relative
            if hashlib.sha256(path.read_bytes()).hexdigest() != expected:
                raise RuntimeError(f'Artifact checksum mismatch: {platform}/{relative}')
        target = destination / platform
        if target.exists():
            shutil.rmtree(target)
        shutil.copytree(source, target)
        records.append({'platform': platform, 'target': record['target'], 'source_commit': revision, 'versions': record['versions'], 'archive': f'{platform}/{record["archive"]}'})
    browser = incoming / 'wasm-ui-assets'
    browser_record = None
    if browser.is_dir():
        browser_record = json.loads((browser / 'manifest.json').read_text())
        if browser_record['source_commit'] != revision:
            raise RuntimeError('WASM UI does not match the source build')
        for line in (browser / 'SHA256SUMS').read_text().splitlines():
            expected, relative = line.split('  ', 1)
            if hashlib.sha256((browser / relative).read_bytes()).hexdigest() != expected:
                raise RuntimeError(f'WASM UI checksum mismatch: {relative}')
        target = destination / 'wasm-ui'
        if target.exists():
            shutil.rmtree(target)
        shutil.copytree(browser, target)
    (destination / 'index.json').write_text(json.dumps({'schema': 'devtools.platform-index/1', 'source_commit': revision, 'platforms': records, 'wasm_ui': {'archive': 'wasm-ui/' + browser_record['archive'], 'target': 'wasm32-unknown-unknown'} if browser_record else None}, indent=2) + '\n')
    git('config', 'user.name', 'github-actions[bot]')
    git('config', 'user.email', '41898282+github-actions[bot]@users.noreply.github.com')
    git('add', 'all-dist')
    changed = subprocess.run(['git', 'diff', '--cached', '--quiet'], cwd=ROOT).returncode
    if changed == 0:
        print(json.dumps({'published': False, 'reason': 'unchanged'}))
        return
    if changed != 1:
        raise RuntimeError('Cannot inspect distribution changes')
    git('commit', '-m', f'Publish four-platform Devtools binaries from {revision[:12]}')
    git('push', 'origin', 'HEAD:main')
    print(json.dumps({'published': True, 'platforms': PLATFORMS, 'source_commit': revision}))


if __name__ == '__main__':
    main()
