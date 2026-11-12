import base64
import importlib.util
import json
from pathlib import Path
import shlex
import sys
import tarfile
import tempfile
import threading
import time
import unittest
from unittest.mock import patch
import urllib.request


ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
SPEC = importlib.util.spec_from_file_location('devtool_demo', ROOT / 'server.py')
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class DemoTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.workspace = tempfile.TemporaryDirectory(prefix='devtool-demo-tests-')
        cls.server = MODULE.DemoServer(('127.0.0.1', 0), Path(cls.workspace.name))
        cls.thread = threading.Thread(target=cls.server.serve_forever, daemon=True)
        cls.thread.start()
        cls.origin = f'http://127.0.0.1:{cls.server.server_port}'

    @classmethod
    def tearDownClass(cls):
        cls.server.shutdown()
        cls.server.close_sessions()
        cls.server.server_close()
        cls.workspace.cleanup()

    def request(self, path, data=None):
        body = json.dumps(data).encode() if data is not None else None
        request = urllib.request.Request(self.origin + path, data=body, headers={'Content-Type': 'application/json'})
        with urllib.request.urlopen(request, timeout=20) as response:
            return json.load(response)

    def wait_for(self, predicate, diagnostic=None):
        deadline = time.monotonic() + 12
        while time.monotonic() < deadline:
            if predicate():
                return
            time.sleep(.025)
        self.fail('PTY condition did not complete' + (': ' + diagnostic() if diagnostic else ''))

    def new_session(self):
        value = self.request('/api/session', {})
        session = self.server.sessions[value['id']]
        self.addCleanup(session.close)
        self.wait_for(lambda: session.ready, lambda: base64.b64decode(session.read(0)['data']).decode(errors='replace'))
        return session

    def command(self, session, text):
        self.request('/api/input', {'session': session.id, 'data': base64.b64encode((text + '\r').encode()).decode()})
        self.wait_for(lambda: session.ready)

    def test_local_assets_and_actual_cli_metadata(self):
        for path in ['/', '/app.js', '/style.css', '/vendor/xterm.js', '/vendor/addon-fit.js', '/vendor/xterm.css']:
            with urllib.request.urlopen(self.origin + path) as response:
                self.assertEqual(response.status, 200)
                self.assertGreater(len(response.read()), 50)
        meta = self.request('/api/meta')
        self.assertEqual(meta['version'], 'devtool 0.5.0')
        self.assertEqual(meta['schema']['version'], '0.5.0')
        self.assertEqual(len(meta['binary_sha256']), 64)

    def test_pty_shell_directory_and_native_template(self):
        session = self.new_session()
        self.command(session, 'devtool -D')
        snapshot = session.snapshot()
        root = Path(snapshot['last_root'])
        self.assertEqual(snapshot['last_run'], 1)
        self.assertEqual(root.parent, session.work)
        self.command(session, 'devtool --canonical-tree')
        expected_state = (ROOT.parent / 'templates/files/state.txt').read_bytes().replace(b'canonical_tree: false', b'canonical_tree: true').replace(b'sha_present: false', b'sha_present: true')
        self.wait_for(lambda: (root / 'state.txt').read_bytes() == expected_state)
        files = session.files()
        self.assertEqual(len([entry for entry in files if not entry['directory']]), 17)
        manifest = json.loads((ROOT.parent / 'templates/canonical-tree.json').read_text())
        for entry in manifest['files']:
            expected = expected_state if entry['path'] == 'state.txt' else (ROOT.parent / 'templates' / entry['asset']).read_bytes()
            self.assertEqual((root / entry['path']).read_bytes(), expected)

    def test_interactive_settings_menu_persists_native_value(self):
        session = self.new_session()
        session.write(b'devtool config menu\r')
        self.wait_for(lambda: b'Choose a number' in base64.b64decode(session.read(0)['data']))
        self.assertFalse(session.ready)
        session.write(b'5\r2\rs\r')
        self.wait_for(lambda: session.ready)
        self.assertEqual(session.snapshot()['settings']['compression']['level'], 2)
        self.command(session, 'devtool config get compression.level')
        self.assertIn(b'2', base64.b64decode(session.read(0)['data']))

    def test_current_compression_commands_create_readable_archives(self):
        session = self.new_session()
        self.command(session, 'devtool -D')
        self.command(session, 'devtool --canonical-tree')
        root = Path(session.snapshot()['last_root'])
        expected_state = (ROOT.parent / 'templates/files/state.txt').read_bytes().replace(b'canonical_tree: false', b'canonical_tree: true').replace(b'sha_present: false', b'sha_present: true')
        self.wait_for(lambda: (root / 'state.txt').read_bytes() == expected_state)
        for format in ['7z', 'tar.gz']:
            self.command(session, f'devtool compress {shlex.quote(str(root))} --format {format} --level 1 -o archief.{format}')
            self.assertTrue((session.work / f'archief.{format}').is_file())
        self.assertTrue((session.work / 'archief.7z').read_bytes().startswith(bytes.fromhex('377abcaf271c')))
        with tarfile.open(session.work / 'archief.tar.gz') as archive:
            self.assertEqual(len(archive.getmembers()), 22)
            self.assertEqual(archive.extractfile(root.name + '/state.txt').read(), expected_state)

    def test_bash_pipes_cwd_and_cli_error_are_actual(self):
        session = self.new_session()
        self.command(session, "printf 'hello PTY' | devtool file create note.txt --stdin")
        self.assertEqual((session.work / 'note.txt').read_text(), 'hello PTY')
        self.command(session, 'devtool mkdir nested')
        self.command(session, 'cd nested')
        self.assertEqual(session.snapshot()['cwd'], str(session.work / 'nested'))
        self.command(session, 'devtool compress missing --format zip')
        self.assertIn(b'invalid value', base64.b64decode(session.read(0)['data']))
        self.assertTrue(session.alive)

    def test_reload_uses_the_same_pty_and_output_cursor(self):
        session = self.new_session()
        self.command(session, "printf 'reload-evidence\\n'")
        again = self.request('/api/session', {'session': session.id})
        self.assertEqual(again['id'], session.id)
        self.assertEqual(again['session_runtime']['watch_pid'], session.session_runtime.state['watch_pid'])
        output = self.request(f'/api/output?session={session.id}&cursor=0')
        self.assertIn(b'reload-evidence', base64.b64decode(output['data']))
        empty = self.request(f'/api/output?session={session.id}&cursor={output["cursor"]}')
        self.assertEqual(empty['data'], '')
        resize = self.request('/api/resize', {'session': session.id, 'columns': 64, 'rows': 18})
        self.assertEqual(resize['columns'], 64)

    def test_watch_is_ready_before_bash_and_monitors_native_commands(self):
        session = self.new_session()
        runtime = session.snapshot()['session_runtime']
        self.assertLess(runtime['watch_ready_ns'], runtime['bash_started_ns'])
        self.assertEqual(runtime['bash_pid'], session.pid)
        self.assertEqual(len(runtime['executables']), 4)
        status = session.session_runtime.watch('--config')
        self.assertTrue(status['running'])
        self.assertEqual(status['process']['pid'], runtime['watch_pid'])
        self.wait_for(lambda: session.telemetry.snapshot()['watch_running'])
        watch = next(p for p in session.telemetry.snapshot()['processes'] if p['name'] == 'devtools-watch')
        self.assertEqual(watch['tty'], 0)
        self.command(session, 'devtool -D && devtool --canonical-tree')
        root = Path(session.snapshot()['last_root'])
        self.wait_for(lambda: 'canonical_tree: true' in (root / 'state.txt').read_text())
        session.write(b'sleep 30\r')
        time.sleep(.1)
        session.write(b'\x03')
        self.wait_for(lambda: session.ready)
        self.assertEqual(session.session_runtime.watch('--config')['process']['pid'], runtime['watch_pid'])

    def test_shell_exit_stops_only_its_own_watch(self):
        session = self.new_session()
        other = self.new_session()
        other_pid = other.session_runtime.state['watch_pid']
        self.assertNotEqual(session.session_runtime.state['watch_pid'], other_pid)
        session.write(b'exit\r')
        self.wait_for(lambda: session.session_runtime.state['phase'] == 'stopped')
        self.assertFalse(session.session_runtime.watch('--config')['running'])
        self.assertEqual(other.session_runtime.watch('--config')['process']['pid'], other_pid)
        session.close()
        self.assertTrue(other.alive)

    def test_watch_start_failure_prevents_pty_creation(self):
        with patch.object(MODULE.SessionRuntime, 'start', side_effect=RuntimeError('watch_not_ready')):
            with patch.object(MODULE.pty, 'openpty') as openpty:
                with self.assertRaisesRegex(RuntimeError, 'watch_not_ready'):
                    MODULE.Session('fff000fff000', self.server.sessions_root)
                openpty.assert_not_called()

    def test_telemetry_records_short_commands_pipelines_and_exit_status(self):
        session = self.new_session()
        text = "devtool --version; printf 'a < b' | cat; false"
        self.command(session, text)
        data = self.request(f'/api/telemetry?session={session.id}')
        command = data['commands'][0]
        self.assertEqual(command['command'], text)
        self.assertEqual(command['status'], 'finished')
        self.assertEqual(command['exit_code'], 1)
        self.assertEqual(command['executables'], ['devtool'])
        self.assertIn('cat', command['steps'])
        self.assertGreaterEqual(command['duration_ms'], 0)
        self.command(session, "printf '%s' '長い名前 \\\" test' > 'long name.txt'")
        latest = session.telemetry.snapshot()['commands'][0]
        self.assertEqual(latest['exit_code'], 0)
        self.assertEqual(latest['status'], 'finished')
        self.assertNotIn('__devtool_prompt', json.dumps(session.telemetry.snapshot()['commands']))

    def test_telemetry_tracks_foreground_menu_background_watch_and_reload(self):
        session = self.new_session()
        self.wait_for(lambda: session.telemetry.snapshot()['watch_running'])
        session.write(b'devtool config menu\r')
        self.wait_for(lambda: any(p['name'] == 'devtool' for p in session.telemetry.snapshot()['processes']))
        data = session.telemetry.snapshot()
        self.assertEqual(data['coverage'], 'complete')
        self.assertFalse(data['shell_ready'])
        self.assertEqual(data['commands'][0]['status'], 'running')
        self.assertTrue(next(e for e in data['executables'] if e['name'] == 'devtool')['active'])
        watch = next(p for p in data['processes'] if p['name'] == 'devtools-watch')
        self.assertEqual(watch['layer'], 'watch')
        self.assertEqual(watch['tty'], 0)
        session.write(b'q\r')
        self.wait_for(lambda: session.ready)
        self.assertEqual(len(session.telemetry.snapshot()['commands']), 1)
        self.request('/api/session', {'session': session.id})
        self.assertEqual(len(session.telemetry.snapshot()['commands']), 1)
        self.command(session, 'devtools-watch --off')
        self.wait_for(lambda: not session.telemetry.snapshot()['watch_running'])
        self.command(session, 'devtools-watch --on')
        self.wait_for(lambda: session.telemetry.snapshot()['watch_running'])


if __name__ == '__main__':
    unittest.main()
