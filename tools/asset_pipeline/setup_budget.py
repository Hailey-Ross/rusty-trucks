"""How much of the machine setup may use: worker counts and process priority.

Setup should finish fast but leave the PC usable (user, 2026-09-30): bounded
worker counts, below-normal priority for the conversion processes, and
overrides for people who want more or less.

Overrides (environment variables, read when setup starts the work):
- SKATE_SETUP_MAP_WORKERS=<n>  maps converted at once (default: see install.map_workers)
- SKATE_SETUP_THREADS=<n>      threads inside one map job for compression (default: cpu/4, 2..6)
- SKATE_SETUP_PRIORITY=below_normal|normal|idle   (default below_normal)

Nothing here changes what setup writes, only how many things run at once and
at which priority. This module's name matches no pipeline fingerprint glob on
purpose: changing a budget must not rebuild anyone's assets.
"""
import os

BELOW_NORMAL_PRIORITY_CLASS = 0x00004000
IDLE_PRIORITY_CLASS = 0x00000040
NORMAL_PRIORITY_CLASS = 0x00000020
PRIORITIES = {'below_normal': BELOW_NORMAL_PRIORITY_CLASS, 'normal': NORMAL_PRIORITY_CLASS,
              'idle': IDLE_PRIORITY_CLASS}


def _positive(name, environ=None):
    value = (os.environ if environ is None else environ).get(name, '').strip()
    if not value:
        return None
    try:
        number = int(value)
    except ValueError:
        return None
    return number if number >= 1 else None


def map_workers(default, environ=None):
    """`SKATE_SETUP_MAP_WORKERS` when set to a positive integer, else `default`."""
    return _positive('SKATE_SETUP_MAP_WORKERS', environ) or default


def job_threads(cpu_count=None, environ=None):
    """Threads one map job may use for zlib/texture compression (zlib releases the GIL)."""
    override = _positive('SKATE_SETUP_THREADS', environ)
    if override:
        return min(override, 32)
    cpus = cpu_count if cpu_count is not None else (os.cpu_count() or 1)
    return max(2, min(6, cpus // 4))


def priority_class(environ=None):
    """Windows priority class for setup's conversion work (below normal by default)."""
    value = (os.environ if environ is None else environ).get('SKATE_SETUP_PRIORITY', '').strip().lower()
    return PRIORITIES.get(value, BELOW_NORMAL_PRIORITY_CLASS)


class lowered_priority:
    """Runs the block with this process at `priority_class()`; child processes
    inherit below-normal and idle classes. Restores the previous class after.
    A no-op outside Windows or when the class cannot be read or set."""

    def __init__(self, environ=None):
        self.target = priority_class(environ)
        self.previous = None

    def __enter__(self):
        if os.name != 'nt':
            return self
        import ctypes
        kernel32 = ctypes.windll.kernel32
        kernel32.GetCurrentProcess.restype = ctypes.c_void_p
        kernel32.GetPriorityClass.argtypes = [ctypes.c_void_p]
        kernel32.SetPriorityClass.argtypes = [ctypes.c_void_p, ctypes.c_uint32]
        process = kernel32.GetCurrentProcess()
        previous = kernel32.GetPriorityClass(process)
        if previous and previous != self.target and kernel32.SetPriorityClass(process, self.target):
            self.previous = previous
        return self

    def __exit__(self, *_):
        if self.previous is not None:
            import ctypes
            kernel32 = ctypes.windll.kernel32
            kernel32.SetPriorityClass(kernel32.GetCurrentProcess(), self.previous)
            self.previous = None
        return False
