import json
import os
from pathlib import Path
import shlex
import threading
import time


class Telemetry:
    def __init__(self, session):
        self.session = session
        self.path = session.runtime / 'commands.bin'
        self.offset = 0
        self.pending = b''
        self.commands = []
        self.processes = []
        self.guard = threading.RLock()
        self.stopping = threading.Event()
        self.last_sample = None
        self.last_observed = {}
        self.coverage = 'complete'
        self.thread = threading.Thread(target=self.run, daemon=True)
        self.thread.start()

    def consume(self):
        if not self.path.exists():
            return
        with self.path.open('rb') as stream:
            stream.seek(self.offset)
            data = stream.read()
            self.offset += len(data)
        self.pending += data
        while self.pending.count(b'\0') >= 4:
            fields = self.pending.split(b'\0', 4)
            self.pending = fields[4]
            kind, identifier, stamp, value = [part.decode(errors='replace') for part in fields[:4]]
            try:
                stamp = float(stamp)
            except ValueError:
                continue
            command = next((entry for entry in reversed(self.commands) if entry['id'] == identifier), None)
            if kind == 'start':
                command = {'id': identifier, 'command': value, 'started': stamp, 'ended': None, 'exit_code': None, 'steps': [], 'executables': []}
                self.commands.append(command)
                self.commands = self.commands[-100:]
            elif command and kind == 'step':
                if not command['steps'] or command['steps'][-1] != value:
                    command['steps'].append(value)
                    command['steps'] = command['steps'][-64:]
                try:
                    words = shlex.split(value)
                except ValueError:
                    words = []
                words = [word for word in words if '=' not in word or word.startswith('/')]
                if words and words[0] in ['command', 'exec', 'env']:
                    words = words[1:]
                if words:
                    name = Path(words[0]).name
                    if name in self.session.session_runtime.state['executables'] and name not in command['executables']:
                        command['executables'].append(name)
            elif command and kind == 'end':
                command['ended'] = stamp
                command['exit_code'] = int(value)

    def process(self, pid, layer):
        proc = Path('/proc') / str(pid)
        try:
            stat = (proc / 'stat').read_text()
            values = stat.rsplit(') ', 1)[1].split()
            if values[0] == 'Z':
                return None
            arguments = [part.decode(errors='replace') for part in (proc / 'cmdline').read_bytes().split(b'\0') if part]
            name = Path(arguments[0]).name if arguments else stat[stat.index('(') + 1:stat.rindex(')')]
            try:
                executable = os.readlink(proc / 'exe')
            except OSError:
                executable = None
            return {'pid': pid, 'ppid': int(values[1]), 'group': int(values[2]), 'tty': int(values[4]), 'foreground_group': int(values[5]), 'state': values[0], 'nice': int(values[16]), 'rss_bytes': int(values[21]) * os.sysconf('SC_PAGE_SIZE'), 'name': name, 'arguments': arguments, 'executable': executable, 'layer': layer}
        except (OSError, ValueError, IndexError):
            return None

    def sample(self):
        self.consume()
        runtime = self.session.session_runtime.state
        watch_pid = runtime.get('watch_pid')
        try:
            identity = self.session.home / '.local/state/devtools-watch/process.json'
            watch_pid = json.loads(identity.read_text())['pid']
        except (OSError, ValueError, KeyError):
            watch_pid = None
        queue = [(self.session.pid, 'shell')]
        if watch_pid:
            queue.append((watch_pid, 'watch'))
        seen = set()
        processes = []
        children = {}
        for proc in Path('/proc').iterdir():
            if not proc.name.isdecimal():
                continue
            try:
                values = (proc / 'stat').read_text().rsplit(') ', 1)[1].split()
                children.setdefault(int(values[1]), []).append(int(proc.name))
            except (OSError, ValueError, IndexError):
                continue
        while queue:
            pid, layer = queue.pop(0)
            if pid in seen:
                continue
            seen.add(pid)
            value = self.process(pid, layer)
            if value:
                processes.append(value)
                queue.extend((child, layer) for child in children.get(pid, []))
        self.processes = processes
        self.last_sample = time.time()
        for process in processes:
            name = process['name']
            if name in runtime['executables']:
                self.last_observed[name] = self.last_sample
                if process['layer'] == 'shell' and self.commands and self.commands[-1]['ended'] is None:
                    command = self.commands[-1]
                    if name not in command['executables']:
                        command['executables'].append(name)
        self.coverage = 'complete' if any(p['pid'] == self.session.pid for p in processes) or not self.session.alive else 'limited'

    def run(self):
        while not self.stopping.is_set():
            with self.guard:
                self.sample()
            self.stopping.wait(.15)

    def snapshot(self):
        with self.guard:
            self.consume()
            now = time.time()
            rows = []
            for entry in reversed(self.commands):
                row = dict(entry)
                row['steps'] = list(entry['steps'])
                row['executables'] = list(entry['executables'])
                row['status'] = 'finished' if entry['ended'] is not None else 'running' if self.session.alive and entry['id'].split(':')[0] == str(self.session.pid) else 'interrupted'
                row['duration_ms'] = None if row['status'] == 'interrupted' else max(0, round(((entry['ended'] or now) - entry['started']) * 1000))
                rows.append(row)
            active = {p['name'] for p in self.processes}
            executables = []
            for name, path in self.session.session_runtime.state['executables'].items():
                last = next((c for c in rows if name in c['executables']), None)
                used = max(last['started'] if last else 0, self.last_observed.get(name, 0)) or None
                executables.append({'name': name, 'path': path, 'active': name in active, 'pids': [p['pid'] for p in self.processes if p['name'] == name], 'last_command': last['command'] if last else None, 'last_used': used})
            return {'sampled_at': self.last_sample, 'sample_interval_ms': 150, 'coverage': self.coverage, 'processes': list(self.processes), 'commands': rows, 'executables': executables, 'shell_pid': self.session.pid, 'shell_ready': self.session.ready, 'shell_alive': self.session.alive, 'watch_running': any(p['layer'] == 'watch' and p['name'] == 'devtools-watch' for p in self.processes), 'sources': ['bash-debug-prompt', 'proc'], 'history_limit': 100}

    def close(self):
        self.stopping.set()
        self.thread.join(timeout=2)
        with self.guard:
            self.sample()
