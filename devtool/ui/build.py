import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import tomllib
import urllib.request

ROOT = Path(__file__).resolve().parent
DESTINATION = ROOT.parent / 'demo/wasm'


def digest(path):
    with path.open('rb') as source:
        return hashlib.file_digest(source, 'sha256').hexdigest()


def run(arguments, environment):
    return subprocess.run([str(argument) for argument in arguments], env=environment, check=True)


def restore(component, reference, destination):
    base = f'https://raw.githubusercontent.com/Dosa42/Assembler-compiler-lib/{reference}/all-dist/ubuntu-24.04-arm64/{component}/'
    with urllib.request.urlopen(base + 'manifest.json') as stream:
        manifest = json.load(stream)
    downloads = destination / 'downloads' / component
    downloads.mkdir(parents=True, exist_ok=True)
    for item in manifest['files']:
        path = downloads / item['name']
        if not path.exists() or digest(path) != item['sha256']:
            with urllib.request.urlopen(base + item['name']) as source, path.open('wb') as target:
                shutil.copyfileobj(source, target)
        if digest(path) != item['sha256']:
            raise RuntimeError(f'Archive part mismatch: {path}')
    archive = downloads / manifest['archive']['name']
    if len(manifest['files']) > 1:
        with archive.open('wb') as target:
            for item in manifest['files']:
                with (downloads / item['name']).open('rb') as source:
                    shutil.copyfileobj(source, target)
    if digest(archive) != manifest['archive']['sha256']:
        raise RuntimeError(f'Archive mismatch: {archive}')
    with tarfile.open(archive, 'r:xz') as source:
        source.extractall(destination, filter='data')
    return manifest


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--toolchain-ref')
    parser.add_argument('--toolchain-root', type=Path, default=ROOT / 'target/toolchains')
    arguments = parser.parse_args()
    environment = dict(os.environ)
    manifests = {}
    if arguments.toolchain_ref:
        for component in ['rust', 'wasm-bindgen', 'binaryen', 'wasm-tools']:
            manifests[component] = restore(component, arguments.toolchain_ref, arguments.toolchain_root)
        prefix = arguments.toolchain_root.resolve() / 'opt'
        environment['RUSTC'] = str(prefix / 'rust/bin/rustc')
        environment['PATH'] = os.pathsep.join(str(prefix / component / 'bin') for component in ['rust', 'wasm-bindgen', 'binaryen', 'wasm-tools']) + os.pathsep + environment['PATH']
        environment['LD_LIBRARY_PATH'] = str(prefix / 'rust/lib') + os.pathsep + environment.get('LD_LIBRARY_PATH', '')
    environment['PATH'] += os.pathsep + str(Path.home() / '.cargo/bin')
    target = Path(environment.get('CARGO_TARGET_DIR', ROOT / 'target')).resolve()
    environment['CARGO_TARGET_DIR'] = str(target)
    version = tomllib.loads((ROOT / 'Cargo.toml').read_text())['dependencies']['wasm-bindgen'].lstrip('=')
    actual = subprocess.check_output(['wasm-bindgen', '--version'], env=environment, text=True).strip()
    if actual != f'wasm-bindgen {version}':
        raise RuntimeError(f'Expected wasm-bindgen {version}, found {actual}')
    run(['cargo', 'fmt', '--manifest-path', ROOT / 'Cargo.toml', '--check'], environment)
    run(['cargo', 'test', '--locked', '--manifest-path', ROOT / 'Cargo.toml'], environment)
    run(['cargo', 'build', '--locked', '--release', '--target', 'wasm32-unknown-unknown', '--manifest-path', ROOT / 'Cargo.toml'], environment)
    DESTINATION.mkdir(exist_ok=True)
    run(['wasm-bindgen', '--target', 'web', '--no-typescript', '--out-dir', DESTINATION, target / 'wasm32-unknown-unknown/release/devtools_ui.wasm'], environment)
    if not (ROOT.parent / 'demo/node_modules/terser').is_dir():
        subprocess.run(['npm', 'ci', '--ignore-scripts'], cwd=ROOT.parent / 'demo', check=True)
    run(['node', ROOT / 'minify.cjs', DESTINATION / 'devtools_ui.js'], environment)
    wasm = DESTINATION / 'devtools_ui_bg.wasm'
    optimized = DESTINATION / 'devtools_ui_optimized.wasm'
    run(['wasm-opt', wasm, '-Os', '--enable-bulk-memory', '--enable-nontrapping-float-to-int', '--enable-sign-ext', '--enable-mutable-globals', '--enable-reference-types', '--enable-multivalue', '-o', optimized], environment)
    optimized.replace(wasm)
    validator = 'wasm-tools' if shutil.which('wasm-tools', path=environment['PATH']) else 'wasm-validate'
    run([validator, *(['validate'] if validator == 'wasm-tools' else []), wasm], environment)
    compiler = subprocess.check_output([environment.get('RUSTC', 'rustc'), '--version', '--verbose'], env=environment, text=True).strip()
    sources = sorted([ROOT / 'Cargo.toml', ROOT / 'Cargo.lock', *ROOT.joinpath('src').glob('*'), ROOT / 'build.py', ROOT / 'minify.cjs', ROOT.parent / 'demo/terminal.js', ROOT.parent / 'demo/index.html', ROOT.parent / 'demo/app.js', ROOT.parent / 'demo/style.css'])
    manifest = {'schema': 'devtools.wasm-ui/1', 'version': '0.1.0', 'target': 'wasm32-unknown-unknown', 'rustc': compiler, 'wasm_bindgen': version, 'toolchain_ref': arguments.toolchain_ref, 'toolchain_manifests': manifests, 'sources': {str(path.relative_to(ROOT.parent)): digest(path) for path in sources}, 'files': {path.name: {'sha256': digest(path), 'bytes': path.stat().st_size} for path in sorted(DESTINATION.iterdir()) if path.is_file() and path.name not in ['manifest.json', 'SHA256SUMS']}}
    (DESTINATION / 'manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
    (DESTINATION / 'SHA256SUMS').write_text(''.join(f'{digest(path)}  {path.name}\n' for path in sorted(DESTINATION.iterdir()) if path.is_file() and path.name != 'SHA256SUMS'))
    print(json.dumps({'wasm_bytes': wasm.stat().st_size, 'output': str(DESTINATION), 'toolchain_ref': arguments.toolchain_ref}))


if __name__ == '__main__':
    main()
