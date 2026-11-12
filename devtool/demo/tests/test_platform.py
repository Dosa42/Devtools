import sys
from pathlib import Path
import unittest


sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from telemetry import darwin_processes


class DarwinProcessTests(unittest.TestCase):
    def test_process_identity_foreground_and_paths_with_spaces(self):
        values = darwin_processes(' 31 1 31 -1 ?? Ss 0 2048 /tmp/tools/devtools-watch\n 40 30 40 40 ttys001 S+ 0 4096 /tmp/a project/devtool\n')
        self.assertEqual(values[31]['tty'], 0)
        self.assertEqual(values[31]['rss_bytes'], 2097152)
        self.assertEqual(values[40]['group'], values[40]['foreground_group'])
        self.assertEqual(values[40]['name'], 'devtool')
        self.assertEqual(values[40]['executable'], '/tmp/a project/devtool')

    def test_zombie_and_incomplete_rows_are_not_active_processes(self):
        values = darwin_processes('31 1 31 -1 ?? Z 0 0 /tmp/devtools-watch\nincomplete\n')
        self.assertEqual(values, {})


if __name__ == '__main__':
    unittest.main()
