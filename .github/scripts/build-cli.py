import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import struct
import subprocess
import tarfile
import tempfile
import tomllib


ROOT = Path(__file__).resolve().parents[2]
PROJECTS = ['devtool', 'devtool-auto', 'devtool-proof', 'devtools-watch']
TARGETS = {
    'ubuntu-24.04-x86_64': ('x86_64-unknown-linux-gnu', 'Linux', 'x86_64', 'ELF', 62),
    'ubuntu-24.04-arm64': ('aarch64-unknown-linux-gnu', 'Linux', 'aarch64', 'ELF', 183),
    'macos-15-x86_64': ('x86_64-apple-darwin', 'Darwin', 'x86_64', 'Mach-O', 0x01000007),
    'macos-15-arm64': ('aarch64-apple-darwin', 'Darwin', 'arm64', 'Mach-O', 0x0100000C),
}


def run(arguments, **kwargs):
    return subprocess.run(arguments, cwd=ROOT, check=True, **kwargs)


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def checksums(directory):
    entries = sorted(p for p in directory.rglob('*') if p.is_file() and p.name != 'SHA256SUMS')
    (directory / 'SHA256SUMS').write_text(''.join(f'{digest(p)}  {p.relative_to(directory).as_posix()}\n' for p in entries))


def executable_identity(path, target):
    data = path.read_bytes()[:32]
    if target[3] == 'ELF':
        valid = data[:6] == b'\x7fELF\x02\x01' and struct.unpack_from('<H', data, 18)[0] == target[4]
    else:
        valid = struct.unpack_from('<II', data) == (0xFEEDFACF, target[4])
    if not valid:
        raise RuntimeError(f'Unexpected executable format: {path}')
    return {'format': target[3], 'architecture': target[2], 'sha256': digest(path)}


def archive(source, destination, name):
    with tarfile.open(destination, 'w:gz') as stream:
        stream.add(source, arcname=name)


def main():
    selected = os.environ['DEVTOOLS_PLATFORM']
    target = TARGETS[selected]
    if os.environ['DEVTOOLS_TARGET'] != target[0] or (platform.system(), platform.machine()) != target[1:3]:
        raise RuntimeError('Runner does not match the selected native target')
    output = Path(os.environ['DEVTOOLS_OUTPUT']).resolve() / selected
    output.mkdir(parents=True, exist_ok=True)
    build_root = Path(os.environ['CARGO_TARGET_DIR']).resolve()
    binaries = build_root / target[0] / 'release'
    source_sha = run(['git', 'rev-parse', 'HEAD'], capture_output=True, text=True).stdout.strip()
    compiler = run(['rustc', '--version'], capture_output=True, text=True).stdout.strip()
    for project in PROJECTS:
        manifest = str(ROOT / project / 'Cargo.toml')
        run(['cargo', 'fmt', '--manifest-path', manifest, '--check'])
        run(['cargo', 'build', '--release', '--locked', '--target', target[0], '--manifest-path', manifest])
    identities = {name: executable_identity(binaries / name, target) for name in PROJECTS}
    environment = dict(os.environ, PATH=str(binaries) + os.pathsep + os.environ['PATH'])
    environment['DEVTOOLS_WATCH_TEST_PROOF'] = str(binaries / 'devtool-proof')
    for project in PROJECTS:
        run(['cargo', 'test', '--release', '--locked', '--target', target[0], '--manifest-path', str(ROOT / project / 'Cargo.toml')], env=environment)
    with tempfile.TemporaryDirectory(prefix='devtools-assemble-') as temporary:
        bundle = Path(temporary) / 'devtools'
        bundle.mkdir()
        shutil.copy2(ROOT / 'README.md', bundle / 'README.md')
        versions = {}
        for project in PROJECTS:
            source = ROOT / project
            destination = bundle / project
            destination.mkdir()
            versions[project] = tomllib.loads((source / 'Cargo.toml').read_text())['package']['version']
            shutil.copy2(source / 'README.md', destination / 'README.md')
            dist = destination / 'dist'
            dist.mkdir()
            shutil.copy2(binaries / project, dist / project)
            (dist / project).chmod(0o755)
            for name in ['schemas', 'templates']:
                if (source / name).is_dir():
                    shutil.copytree(source / name, destination / name)
            for name in ['config.example.toml', 'profile.schema.json', 'rules.schema.json', 'RULES.md']:
                if (source / name).is_file():
                    shutil.copy2(source / name, destination / name)
            if (source / 'examples').is_dir():
                shutil.copytree(source / 'examples', destination / 'examples')
        for project in PROJECTS[1:]:
            shutil.copy2(binaries / project, bundle / 'devtool/dist' / project)
        shutil.copytree(ROOT / 'devtool/demo', bundle / 'devtool/demo', ignore=shutil.ignore_patterns('workspaces', '.runtime', '__pycache__', '*.pyc', 'node_modules'))
        for project in PROJECTS:
            checksums(bundle / project / 'dist')
        demo = bundle / 'devtool/demo'
        subprocess.run(['python', '-m', 'unittest', 'discover', '-s', 'tests', '-v'], cwd=demo, env=environment, check=True)
        for project in PROJECTS:
            directory = output / project
            directory.mkdir()
            tarball = directory / f'{project}-{selected}.tar.gz'
            archive(bundle / project, tarball, project)
            record = {'schema': 'devtools.distribution/1', 'platform': selected, 'target': target[0], 'source_commit': source_sha, 'rustc': compiler, 'executable': project, 'version': versions[project], **identities[project], 'archive': tarball.name, 'archive_sha256': digest(tarball)}
            (directory / 'manifest.json').write_text(json.dumps(record, indent=2) + '\n')
            (directory / 'README.md').write_text(f'# {project} — {selected}\n\nVersie {versions[project]}, target `{target[0]}`, formaat {target[3]}.\n\nPak `{tarball.name}` uit. De executable staat in `{project}/dist/{project}`.\n\nBroncommit: `{source_sha}`. Schema’s en templates zijn opgenomen waar van toepassing.\n')
            checksums(directory)
        tarball = output / f'devtools-{selected}.tar.gz'
        archive(bundle, tarball, 'devtools')
        record = {'schema': 'devtools.bundle/1', 'platform': selected, 'target': target[0], 'source_commit': source_sha, 'rustc': compiler, 'versions': versions, 'executables': identities, 'archive': tarball.name, 'archive_sha256': digest(tarball), 'tests': 'native Rust integration suites and packaged demo integration suite passed'}
        (output / 'manifest.json').write_text(json.dumps(record, indent=2) + '\n')
        (output / 'README.md').write_text(f'# Devtools — {selected}\n\nVier native {target[3]}-executables voor `{target[0]}`.\n\nPak `devtools-{selected}.tar.gz` uit en start vanuit de uitgepakte map:\n\n```sh\nexport PATH="$PWD/devtool/dist:$PATH"\npython devtool/demo/server.py --port 8765\n```\n\nOpen http://127.0.0.1:8765. Python 3.11 of nieuwer en Bash zijn nodig voor de demo.\n\nDe vier submappen bevatten ook afzonderlijke pakketten. `devtool` bevat de complete demo en vier binaries; de overige pakketten bevatten hun eigen executable.\n\nBroncommit: `{source_sha}`.\n')
        checksums(output)
    summary = os.environ.get('GITHUB_STEP_SUMMARY')
    if summary:
        with open(summary, 'a') as stream:
            stream.write(f'## {selected}\n\nTarget: `{target[0]}`. Vier executables gebouwd en native getest, inclusief de verpakte PTY-demo.\n')
    print(json.dumps({'platform': selected, 'target': target[0], 'output': str(output), 'executables': PROJECTS}))


if __name__ == '__main__':
    main()
