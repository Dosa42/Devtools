import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile
import urllib.request

root = Path(__file__).resolve().parents[1]
app = root / 'android-app'
record_path = app / 'import.json'
record = json.loads(record_path.read_text())

def identity(directory):
    entries = []
    for path in sorted(directory.rglob('*')):
        if not path.is_file():
            continue
        name = path.relative_to(directory).as_posix()
        if name == 'import.json' or name.startswith('build/'):
            continue
        digest = hashlib.sha256(path.read_bytes()).hexdigest()
        entries.append(name + '\0' + digest + '\n')
    return hashlib.sha256(''.join(entries).encode()).hexdigest(), len(entries)

if record['state'] == 'complete':
    print(json.dumps({'import': 'already_complete', 'source': record['source_commit']}))
else:
    with tempfile.TemporaryDirectory(prefix='devtools-android-import-') as temporary:
        work = Path(temporary)
        archive = work / 'source.tar.gz'
        url = 'https://codeload.github.com/' + record['source_repository'] + '/tar.gz/' + record['source_commit']
        with urllib.request.urlopen(url, timeout=120) as source, archive.open('wb') as target:
            shutil.copyfileobj(source, target)
        stage = work / 'app'
        stage.mkdir()
        with tarfile.open(archive, 'r:gz') as source:
            for member in source:
                pieces = member.name.split('/', 1)
                if len(pieces) != 2 or not pieces[1]:
                    continue
                name = pieces[1]
                if name == 'build' or name.startswith('build/') or name == '.github/workflows/build.yml':
                    continue
                member.name = name
                source.extract(member, stage, filter='data')
        shutil.copytree(app, stage, dirs_exist_ok=True, ignore=shutil.ignore_patterns('build', '__pycache__'))
        digest, count = identity(stage)
        if digest != record['content_sha256'] or count != record['files']:
            raise RuntimeError('Imported app differs from the locally verified migration: ' + str((digest, count)))
        record['state'] = 'complete'
        (stage / 'import.json').write_text(json.dumps(record, indent=2) + '\n')
        shutil.copytree(stage, app, dirs_exist_ok=True)
    subprocess.run(['git', 'add', '--', 'android-app'], cwd=root, check=True)
    print(json.dumps({'import': 'complete', 'files': count, 'content_sha256': digest}))
