import argparse
import atexit
import base64
import errno
import fcntl
import hashlib
import json
import mimetypes
import os
from pathlib import Path
import pty
import re
import signal
import struct
import subprocess
import sys
import termios
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs, urlsplit
import uuid
from session_runtime import SessionRuntime
from telemetry import Telemetry


ROOT = Path(__file__).resolve().parent
PROJECT = ROOT.parent
BINARY = PROJECT / 'dist/devtool'
APP_ID = 'devtool-terminal-demo'
PROMPT_MARKER = b'\x1b]133;A\x07'
SETTINGS_KEYS = ['schema_version', 'filesystem.create_parents', 'filesystem.existing_file', 'compression.format', 'compression.level', 'output.format', 'output.color']


def cli(arguments, environment=None, cwd=None):
    result = subprocess.run([str(BINARY), *arguments], env=environment, cwd=cwd, capture_output=True, text=True, timeout=20)
    return {'code': result.returncode, 'stdout': result.stdout, 'stderr': result.stderr}


class Session:
    def __init__(self, identifier, sessions_root):
        self.id = identifier
        self.root = sessions_root / identifier
        self.work = self.root / 'workspace'
        self.home = self.root / 'home'
        self.runtime = self.root / 'runtime'
        for directory in [self.work, self.home, self.runtime]:
            directory.mkdir(parents=True, exist_ok=True)
        self.environment = {key: value for key, value in os.environ.items() if not key.startswith('DEVTOOL_')}
        self.environment.update({
            'HOME': str(self.home),
            'XDG_CONFIG_HOME': str(self.home / '.config'),
            'XDG_STATE_HOME': str(self.home / '.local/state'),
            'PATH': str(BINARY.parent) + os.pathsep + os.environ.get('PATH', ''),
            'TERM': 'xterm-256color',
            'COLORTERM': 'truecolor',
            'HISTFILE': str(self.home / '.bash_history'),
            'DEMO_CWD_FILE': str(self.runtime / 'cwd'),
            'DEMO_EVENTS_FILE': str(self.runtime / 'commands.bin'),
            'PROMPT_COMMAND': 'printf "%s" "$PWD" > "$DEMO_CWD_FILE"',
            'PS1': '\\[\x1b]133;A\x07\\]\\[\x1b[38;5;114m\\]devtool\\[\x1b[0m\\] \\[\x1b[38;5;245m\\]\\w\\[\x1b[0m\\]\\n\\$ ',
        })
        self.environment['DEMO_PS1'] = self.environment['PS1']
        self.environment.pop('DEVTOOLS_WATCH_DAEMON', None)
        self.guard = threading.RLock()
        self.output = bytearray()
        self.base = 0
        self.ready = False
        self.alive = True
        self.tail = b''
        self.created = time.time()
        self.transcript = self.runtime / 'terminal.bin'
        if self.transcript.exists():
            data = self.transcript.read_bytes()[-4_000_000:]
            self.output.extend(data)
        cwd_file = self.runtime / 'cwd'
        cwd = Path(cwd_file.read_text()) if cwd_file.exists() else self.work
        cwd = cwd if cwd.is_dir() else self.work
        self.session_runtime = SessionRuntime(BINARY.parent, self.environment, self.work, self.runtime)
        self.session_runtime.start()
        try:
            self.master, slave = pty.openpty()
            try:
                self.process = subprocess.Popen([sys.executable, str(ROOT / 'pty_child.py'), str(slave)], pass_fds=(slave,), env=self.environment, cwd=cwd)
            finally:
                os.close(slave)
            self.pid = self.process.pid
            self.session_runtime.attach(self.pid)
        except BaseException:
            if hasattr(self, 'master'):
                os.close(self.master)
            self.session_runtime.stop()
            raise
        self.resize(100, 28)
        self.thread = threading.Thread(target=self.read_output, daemon=True)
        self.telemetry = Telemetry(self)
        self.thread.start()

    def read_output(self):
        master = self.master
        try:
            with self.transcript.open('ab', buffering=0) as transcript:
                while self.alive:
                    try:
                        data = os.read(master, 65536)
                    except OSError as error:
                        if error.errno in [errno.EIO, errno.EBADF]:
                            break
                        raise
                    if not data:
                        break
                    transcript.write(data)
                    with self.guard:
                        self.output.extend(data)
                        if PROMPT_MARKER in self.tail + data:
                            self.ready = True
                        self.tail = (self.tail + data)[-len(PROMPT_MARKER):]
                        if len(self.output) > 4_000_000:
                            excess = len(self.output) - 4_000_000
                            del self.output[:excess]
                            self.base += excess
        finally:
            self.alive = False
            self.session_runtime.stop()
            self.telemetry.close()

    def write(self, data):
        with self.guard:
            if b'\r' in data or b'\n' in data:
                self.ready = False
            os.write(self.master, data)

    def resize(self, columns, rows):
        fcntl.ioctl(self.master, termios.TIOCSWINSZ, struct.pack('HHHH', max(2, int(rows)), max(2, int(columns)), 0, 0))

    def read(self, cursor):
        with self.guard:
            start = max(cursor, self.base)
            data = bytes(self.output[start - self.base:])
            return {'data': base64.b64encode(data).decode(), 'cursor': self.base + len(self.output), 'reset': cursor < self.base, 'alive': self.alive, 'ready': self.ready}

    def snapshot(self):
        cwd_file = self.runtime / 'cwd'
        cwd = cwd_file.read_text() if cwd_file.exists() else str(self.work)
        settings = cli(['--json', 'config', 'show'], self.environment, self.work)
        value = json.loads(settings['stdout']) if settings['code'] == 0 else None
        state_path = self.home / '.local/state/devtool/directory-counter.json'
        state = json.loads(state_path.read_text()) if state_path.exists() else {}
        return {'id': self.id, 'workspace': str(self.work), 'cwd': cwd, 'home': str(self.home), 'ready': self.ready, 'alive': self.alive, 'settings': value, 'settings_error': settings['stderr'] if settings['code'] else None, 'last_root': state.get('last_root'), 'last_run': state.get('last_run'), 'config_path': str(self.home / '.config/devtool/config.toml'), 'session_runtime': dict(self.session_runtime.state)}

    def files(self):
        entries = []
        for current, directories, filenames in os.walk(self.work, followlinks=False):
            directories.sort()
            filenames.sort()
            for name in [*directories, *filenames]:
                path = Path(current) / name
                try:
                    metadata = path.lstat()
                    entries.append({'path': str(path.relative_to(self.work)), 'name': name, 'directory': path.is_dir(), 'symlink': path.is_symlink(), 'bytes': metadata.st_size, 'modified': metadata.st_mtime_ns})
                except FileNotFoundError:
                    pass
        return entries

    def file(self, relative):
        path = (self.work / relative).resolve()
        path.relative_to(self.work.resolve())
        return path

    def close(self):
        if self.master is None:
            return
        if self.alive:
            self.alive = False
            try:
                os.killpg(self.pid, signal.SIGHUP)
            except ProcessLookupError:
                pass
        try:
            os.close(self.master)
        except OSError:
            pass
        self.master = None
        try:
            self.process.wait(timeout=2)
        except subprocess.TimeoutExpired:
            self.process.kill()
            self.process.wait(timeout=2)
        self.session_runtime.stop()
        self.telemetry.close()


class DemoServer(ThreadingHTTPServer):
    daemon_threads = True

    def __init__(self, address, sessions_root=None):
        self.sessions_root = Path(sessions_root or ROOT / 'workspaces')
        self.sessions_root.mkdir(parents=True, exist_ok=True)
        self.sessions = {}
        self.sessions_lock = threading.RLock()
        super().__init__(address, Handler)

    def session(self, identifier=None):
        identifier = identifier or uuid.uuid4().hex[:12]
        if not re.fullmatch('[a-f0-9]{12}', identifier):
            raise ValueError('Ongeldige sessiecode')
        with self.sessions_lock:
            if identifier not in self.sessions:
                self.sessions[identifier] = Session(identifier, self.sessions_root)
            return self.sessions[identifier]

    def close_sessions(self):
        for session in self.sessions.values():
            session.close()


class Handler(BaseHTTPRequestHandler):
    server_version = 'DevtoolDemo/0.4.0'

    def log_message(self, format, *arguments):
        print(f'{self.log_date_time_string()} {format % arguments}', flush=True)

    def send_data(self, status, data, content_type):
        self.send_response(status)
        self.send_header('Content-Type', content_type)
        self.send_header('Content-Length', str(len(data)))
        self.send_header('Cache-Control', 'no-store')
        self.end_headers()
        self.wfile.write(data)

    def send_json(self, status, data):
        self.send_data(status, json.dumps(data).encode(), 'application/json; charset=utf-8')

    def request_session(self, query):
        return self.server.session(query['session'][0])

    def do_GET(self):
        try:
            url = urlsplit(self.path)
            query = parse_qs(url.query)
            if url.path == '/api/health':
                return self.send_json(200, {'app': APP_ID, 'binary': str(BINARY), 'version': cli(['--version'])['stdout'].strip(), 'pid': os.getpid()})
            if url.path == '/api/meta':
                return self.send_json(200, {'version': cli(['--version'])['stdout'].strip(), 'schema': json.loads(cli(['schema'])['stdout']), 'binary_sha256': hashlib.sha256(BINARY.read_bytes()).hexdigest()})
            if url.path == '/api/output':
                return self.send_json(200, self.request_session(query).read(int(query.get('cursor', ['0'])[0])))
            if url.path == '/api/state':
                return self.send_json(200, self.request_session(query).snapshot())
            if url.path == '/api/telemetry':
                return self.send_json(200, self.request_session(query).telemetry.snapshot())
            if url.path == '/api/files':
                return self.send_json(200, {'entries': self.request_session(query).files()})
            if url.path == '/api/file':
                path = self.request_session(query).file(query['path'][0])
                return self.send_data(200, path.read_bytes(), mimetypes.guess_type(path.name)[0] or 'application/octet-stream')
            path = ROOT / ('index.html' if url.path == '/' else url.path.lstrip('/'))
            path = path.resolve()
            path.relative_to(ROOT)
            if path.parts[len(ROOT.parts)] not in ['index.html', 'app.js', 'style.css', 'vendor', 'favicon.svg']:
                return self.send_json(404, {'error': 'Bestand bestaat niet'})
            return self.send_data(200, path.read_bytes(), (mimetypes.guess_type(path.name)[0] or 'application/octet-stream') + ('; charset=utf-8' if path.suffix in ['.html', '.js', '.css', '.svg'] else ''))
        except (BrokenPipeError, ConnectionResetError):
            return
        except (FileNotFoundError, KeyError) as error:
            self.send_json(404, {'error': str(error)})
        except Exception as error:
            self.send_json(400, {'error': str(error)})

    def do_POST(self):
        try:
            data = json.loads(self.rfile.read(int(self.headers.get('Content-Length', 0))) or b'{}')
            path = urlsplit(self.path).path
            if path == '/api/session':
                return self.send_json(200, self.server.session(data.get('session')).snapshot())
            session = self.server.session(data['session'])
            if path == '/api/input':
                session.write(base64.b64decode(data['data']))
                return self.send_json(200, {'written': True})
            if path == '/api/resize':
                session.resize(data['columns'], data['rows'])
                return self.send_json(200, {'columns': data['columns'], 'rows': data['rows']})
            self.send_json(404, {'error': 'Onbekende actie'})
        except (BrokenPipeError, ConnectionResetError):
            return
        except Exception as error:
            self.send_json(400, {'error': str(error)})


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--port', type=int, default=8765)
    args = parser.parse_args()
    server = DemoServer(('127.0.0.1', args.port))
    atexit.register(server.close_sessions)
    runtime = ROOT / '.runtime'
    runtime.mkdir(exist_ok=True)
    (runtime / 'server.json').write_text(json.dumps({'pid': os.getpid(), 'url': f'http://127.0.0.1:{server.server_port}', 'app': APP_ID}))
    def stop(signum, frame):
        threading.Thread(target=server.shutdown, daemon=True).start()
    signal.signal(signal.SIGTERM, stop)
    signal.signal(signal.SIGINT, stop)
    print(f'Devtool terminal demo: http://127.0.0.1:{server.server_port}', flush=True)
    try:
        server.serve_forever(poll_interval=0.2)
    finally:
        server.close_sessions()
        server.server_close()


if __name__ == '__main__':
    main()
