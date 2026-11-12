import os
from pathlib import Path
import sys

os.login_tty(int(sys.argv[1]))
os.execvpe('bash', ['bash', '--noprofile', '--rcfile', str(Path(__file__).with_name('shell_hooks.sh')), '-i'], os.environ)
