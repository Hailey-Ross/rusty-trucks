"""Setup keeps converting when its console is gone (issue #20: OSError Errno 22 on print)."""
import errno
import io
import subprocess
import sys
import tempfile
import unittest
from contextlib import redirect_stderr, redirect_stdout
from pathlib import Path
from tools import setup as setup_ui

REPO = Path(__file__).resolve().parents[1]

TASK = '''\
import sys
from pathlib import Path
out = Path(sys.argv[1])
for i in range(2000):
    print(f'progress {i}', flush=True)
    print(f'warning {i}', file=sys.stderr, flush=True)
(out/'done.txt').write_text('finished', encoding='utf-8')
'''

# Starts the conversion only after the parent has closed both pipe ends, so every
# progress print hits a dead console.
CHILD = '''\
import sys, time
from pathlib import Path
sys.path.insert(0, {repo!r})
from tools import setup
root = Path({root!r})
go = root/'go'
while not go.exists():
    time.sleep(0.01)
if {guarded!r}:
    setup.ROOT = root
    sys.argv = ['skate3setup', '--task', 'tools/progress_task.py', str(root)]
    raise SystemExit(setup.main())
import runpy
sys.argv = [str(root/'tools/progress_task.py'), str(root)]
runpy.run_path(sys.argv[0], run_name='__main__')
'''


class FailingStream:
    def __init__(self, error):
        self.error = error
        self.writes = 0

    def write(self, text):
        self.writes += 1
        raise self.error

    def flush(self):
        raise self.error


class DeadConsole(unittest.TestCase):
    def test_print_to_failing_stream_is_ignored(self):
        for error in (OSError(errno.EINVAL, 'Invalid argument'), BrokenPipeError(errno.EPIPE, 'Broken pipe'),
                      ValueError('I/O operation on closed file.')):
            failing = FailingStream(error)
            stream = setup_ui.TolerantStream(failing)
            print('Preparing library', file=stream, flush=True)
            print('next stage', file=stream, flush=True)
            stream.writelines(['a\n', 'b\n'])
            stream.flush()
            # The first failure marks the console dead; later output is dropped without retrying.
            self.assertEqual(failing.writes, 1)

    def test_working_stream_is_forwarded(self):
        target = io.StringIO()
        stream = setup_ui.TolerantStream(target)
        print('progress 1', file=stream, flush=True)
        self.assertEqual(target.getvalue(), 'progress 1\n')
        self.assertEqual(stream.getvalue(), 'progress 1\n')  # other attributes pass through

    def test_guard_wraps_both_streams_once(self):
        out, err = io.StringIO(), io.StringIO()
        with redirect_stdout(out), redirect_stderr(err):
            setup_ui.tolerate_dead_console()
            setup_ui.tolerate_dead_console()
            self.assertIsInstance(sys.stdout, setup_ui.TolerantStream)
            self.assertIs(sys.stdout._stream, out)
            self.assertIs(sys.stderr._stream, err)
            print('hello')
        self.assertEqual(out.getvalue(), 'hello\n')

    def run_child(self, guarded):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp).resolve()
            (root/'tools').mkdir()
            (root/'tools/progress_task.py').write_text(TASK, encoding='utf-8')
            code = CHILD.format(repo=str(REPO), root=str(root), guarded=guarded)
            child = subprocess.Popen([sys.executable, '-c', code], stdout=subprocess.PIPE,
                                     stderr=subprocess.PIPE, cwd=root)
            child.stdout.close()
            child.stderr.close()
            (root/'go').touch()
            code = child.wait(timeout=120)
            return code, (root/'done.txt').is_file()

    def test_conversion_task_finishes_through_a_closed_pipe(self):
        self.assertEqual(self.run_child(guarded=True), (0, True))

    def test_closed_pipe_aborts_an_unguarded_task(self):
        # Control: the same task without the guard dies on its first progress print, as in #20.
        code, finished = self.run_child(guarded=False)
        self.assertNotEqual(code, 0)
        self.assertFalse(finished)


if __name__ == '__main__':
    unittest.main()
