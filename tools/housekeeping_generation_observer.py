"""Observe report replacements only; this is not a writer-exclusion primitive."""
import ctypes
import hashlib
import os
import select
import stat
import struct
import time

_IN_MOVED_TO = 0x00000080
_IN_MOVED_FROM = 0x00000040
_IN_MUTATION = _IN_MOVED_TO | _IN_MOVED_FROM | 0x00000100 | 0x00000200 | 0x00000004 | 0x00000002
_IN_WATCH_INVALID = 0x00000400 | 0x00000800
_IN_Q_OVERFLOW = 0x00004000
_IN_IGNORED = 0x00008000
_EVENT = struct.Struct('iIII')
_libc = ctypes.CDLL(None, use_errno=True)


class ReportGenerationObserver:
    """Observe two distinct pinned atomic report replacements after a barrier.

    The caller must watch before exchange, call begin_exchange immediately
    before it and finish_exchange immediately after. These events do not prove
    source identity, single-worker execution, marker integrity, or exclusion.
    """
    def __init__(self, directory):
        self.dirfd = os.open(directory, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC)
        self.fd = _libc.inotify_init1(os.O_NONBLOCK | os.O_CLOEXEC)
        if self.fd < 0:
            os.close(self.dirfd)
            raise OSError(ctypes.get_errno(), 'inotify_init1')
        self.held = []
        self.ready = False
        self.exchanging = False
        self.watch = _libc.inotify_add_watch(self.fd, os.fsencode(f'/proc/self/fd/{self.dirfd}'), _IN_MUTATION | _IN_WATCH_INVALID)
        if self.watch < 0:
            err = ctypes.get_errno(); self.close(); raise OSError(err, 'inotify_add_watch')

    def close(self):
        for fd in self.held: os.close(fd)
        self.held.clear()
        for name in ('fd', 'dirfd'):
            fd = getattr(self, name, -1)
            if fd >= 0: os.close(fd); setattr(self, name, -1)

    def _events(self):
        while True:
            try: raw = os.read(self.fd, 65536)
            except BlockingIOError: return
            if not raw: raise RuntimeError('inotify stream ended')
            pos = 0
            while pos < len(raw):
                if len(raw) - pos < _EVENT.size: raise RuntimeError('truncated event')
                wd, mask, cookie, length = _EVENT.unpack_from(raw, pos); pos += _EVENT.size
                if len(raw) - pos < length: raise RuntimeError('truncated event name')
                name = raw[pos:pos+length].split(b'\0',1)[0]; pos += length
                if mask & (_IN_Q_OVERFLOW | _IN_IGNORED | _IN_WATCH_INVALID): raise RuntimeError('report watch lost')
                if wd != self.watch: raise RuntimeError('unexpected watch')
                yield mask, name

    def begin_inode_marker(self):
        """Arm before metadata-only promotion of the existing journal inode."""
        if self.ready or self.exchanging: raise RuntimeError('barrier already armed')
        for _ in self._events(): pass
        self.exchanging = True

    def finish_inode_marker(self, timeout=5):
        if not self.exchanging or self.ready: raise RuntimeError('barrier not armed')
        deadline = time.monotonic() + timeout
        observed = False
        while not observed:
            remaining = deadline - time.monotonic()
            if remaining <= 0 or not select.select([self.fd], [], [], remaining)[0]:
                raise TimeoutError('journal metadata event not observed')
            for mask, name in self._events():
                if name != b'operation.json': continue
                if mask != 0x00000004:
                    raise RuntimeError('unexpected journal mutation during promotion')
                observed = True
        self.exchanging = False
        self.ready = True
        self.assert_intact()

    def begin_exchange(self):
        if self.ready or self.exchanging: raise RuntimeError('exchange already armed')
        for _ in self._events(): pass
        self.exchanging = True

    def finish_exchange(self, timeout=5):
        if not self.exchanging or self.ready: raise RuntimeError('exchange not armed')
        deadline = time.monotonic() + timeout
        expected = [_IN_MOVED_FROM, _IN_MOVED_TO]
        while expected:
            remaining = deadline - time.monotonic()
            if remaining <= 0 or not select.select([self.fd], [], [], remaining)[0]:
                raise TimeoutError('journal exchange events not observed')
            for mask, name in self._events():
                if name != b'operation.json': continue
                if not expected or mask != expected.pop(0):
                    raise RuntimeError('unexpected journal mutation during exchange')
        self.exchanging = False
        self.ready = True
        self.assert_intact()

    def assert_intact(self):
        if not self.ready: raise RuntimeError('observer not armed')
        for mask, name in self._events():
            if name == b'operation.json' or (name == b'' and mask & 0x00000004):
                raise RuntimeError('journal or parent changed after barrier')

    def wait_two(self, timeout):
        if not self.ready or timeout <= 0: raise RuntimeError('observer not armed')
        deadline = time.monotonic() + timeout
        found = []
        while len(found) < 2:
            remaining = deadline - time.monotonic()
            if remaining <= 0: raise TimeoutError('two report generations not observed')
            if not select.select([self.fd], [], [], remaining)[0]: raise TimeoutError('two report generations not observed')
            for mask, name in self._events():
                if name == b'operation.json' or (name == b'' and mask & 0x00000004): raise RuntimeError('journal or parent changed after barrier')
                if len(found) == 2 or not (mask & _IN_MOVED_TO) or name != b'latest.json': continue
                fd = os.open('latest.json', os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC, dir_fd=self.dirfd)
                try:
                    before = os.fstat(fd)
                    if not stat.S_ISREG(before.st_mode) or before.st_nlink != 1 or before.st_size > 131072:
                        raise RuntimeError('report file differs')
                    data = b''
                    while True:
                        part = os.read(fd, 65536)
                        if not part: break
                        data += part
                        if len(data) > 131072: raise RuntimeError('report too large')
                    after = os.fstat(fd)
                    if (before.st_dev,before.st_ino,before.st_size,before.st_mtime_ns,before.st_ctime_ns) != (after.st_dev,after.st_ino,after.st_size,after.st_mtime_ns,after.st_ctime_ns):
                        raise RuntimeError('report changed during read')
                    identity = (before.st_dev,before.st_ino)
                    if identity in [item['identity'] for item in found]: raise RuntimeError('report generations collapsed')
                    self.held.append(fd); fd = -1
                    found.append({'identity':identity,'sha256':hashlib.sha256(data).hexdigest()})
                finally:
                    if fd >= 0: os.close(fd)
        return found
