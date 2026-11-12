import json
import os
from pathlib import Path
import subprocess
import threading
import time


class SessionRuntime:
    def __init__(self, binaries, environment, work, runtime):
        self.binaries = Path(binaries)
        self.environment = environment
        self.work = work
        self.path = runtime / 'session.json'
        self.guard = threading.RLock()
        self.active = False
        self.state = {'schema': 'devtools.session/1', 'phase': 'starting', 'executables': {}}

    def watch(self, action):
        result = subprocess.run([str(self.binaries / 'devtools-watch'), action], env=self.environment, cwd=self.work, capture_output=True, text=True, timeout=15)
        value = json.loads(result.stdout)
        if result.returncode or not value.get('ok'):
            raise RuntimeError('watch_start' if action == '--on' else 'watch_control')
        return value

    def save(self):
        self.path.write_text(json.dumps(self.state, indent=2) + '\n')

    def start(self):
        with self.guard:
            for name in ['devtool', 'devtool-auto', 'devtool-proof', 'devtools-watch']:
                path = self.binaries / name
                if not path.is_file() or not os.access(path, os.X_OK):
                    raise RuntimeError('executable_unavailable:' + name)
                self.state['executables'][name] = str(path)
            value = self.watch('--on')
            self.active = True
            try:
                status = self.watch('--config')
                if not status.get('running') or status['process']['pid'] != value['pid']:
                    raise RuntimeError('watch_not_ready')
                self.state.update({'phase': 'watch_ready', 'watch_pid': value['pid'], 'watch_ready_ns': time.monotonic_ns(), 'watch_config': status['config_path']})
                self.save()
            except BaseException:
                self.stop()
                raise

    def attach(self, pid):
        with self.guard:
            self.state.update({'phase': 'shell_running', 'bash_pid': pid, 'bash_started_ns': time.monotonic_ns()})
            self.save()

    def stop(self):
        with self.guard:
            if self.active:
                self.watch('--off')
                self.active = False
                self.state.update({'phase': 'stopped', 'stopped_ns': time.monotonic_ns()})
                self.save()
