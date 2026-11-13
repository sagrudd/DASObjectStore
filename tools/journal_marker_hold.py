"""Durable marker exchange primitive; not an integrated recovery coordinator.

No CLI. A caller must independently authenticate the target, pinned parent,
original journal, client fence, trusted operator window and live writer state.
The sentinel deliberately remains installed after exchange or interruption.
"""
import array
import ctypes
import fcntl
import hashlib
import json
import os
import stat

from sticky_journal_parent import StickyJournalParent, _acl, _FENCED_ACL
from housekeeping_generation_observer import ReportGenerationObserver

_GETFLAGS = 0x80086601
_SETFLAGS = 0x40086602
_IMMUTABLE = 0x10
_libc = ctypes.CDLL(None, use_errno=True)
_libc.renameat2.argtypes = (ctypes.c_int, ctypes.c_char_p, ctypes.c_int, ctypes.c_char_p, ctypes.c_uint)
_libc.renameat2.restype = ctypes.c_int


def _flags(fd):
    value = array.array('L',[0]); fcntl.ioctl(fd,_GETFLAGS,value,True); return value[0]


def _set_immutable(fd):
    flags = _flags(fd)
    value = array.array('L',[flags | _IMMUTABLE]); fcntl.ioctl(fd,_SETFLAGS,value,True)
    os.fsync(fd)
    if _flags(fd) != (flags | _IMMUTABLE): raise RuntimeError('marker flags differ')


def _identity(fd):
    s=os.fstat(fd); return (s.st_dev,s.st_ino)


def _named(dirfd,name,identity):
    s=os.stat(name,dir_fd=dirfd,follow_symlinks=False)
    if not stat.S_ISREG(s.st_mode) or s.st_nlink != 1 or (s.st_dev,s.st_ino) != identity:
        raise RuntimeError('marker exchange identity differs')
    return s


class JournalMarkerHold:
    """Install an immutable unreadable marker while retaining the old journal.

    This does not establish a worker boundary or authorize a DB/claim change.
    It never clears an installed marker, including on exceptions.
    """
    def __init__(self, parent, private_fd, observer):
        if type(parent) is not StickyJournalParent or type(observer) is not ReportGenerationObserver:
            raise RuntimeError('marker requires exact reviewed component types')
        if os.geteuid() != 0 or parent.lock_fd is None:
            raise RuntimeError('sticky parent not held')
        p = parent._check_parent(); q = os.fstat(private_fd)
        if ((p.st_uid,p.st_gid,stat.S_IMODE(p.st_mode),_acl(parent.parent_fd)) != (0,972,0o1770,_FENCED_ACL)
                or not stat.S_ISDIR(q.st_mode) or q.st_uid != 0 or q.st_gid != 0
                or stat.S_IMODE(q.st_mode) != 0o700 or p.st_dev != q.st_dev):
            raise RuntimeError('marker directories differ')
        if _identity(observer.dirfd) != parent.parent:
            raise RuntimeError('observer does not watch pinned parent')
        parent._verified_intent(); parent._check_journal()
        self.parent=parent; self.private_fd=os.dup(private_fd); self.observer=observer
        self.marker_fd=None; self.intent_fd=None; self.exchanged=False

    def install(self):
        if self.marker_fd is not None: raise RuntimeError('marker already attempted')
        self.marker_fd=os.open('marker',os.O_RDWR|os.O_CREAT|os.O_EXCL|os.O_NOFOLLOW|os.O_CLOEXEC,0o600,dir_fd=self.private_fd)
        os.fchmod(self.marker_fd,0o600)
        data=b'DAS housekeeping maintenance hold; do not remove\n'
        if os.write(self.marker_fd,data) != len(data): raise RuntimeError('short marker write')
        os.fsync(self.marker_fd);os.fsync(self.private_fd)
        marker_id=_identity(self.marker_fd); old_id=self.parent.journal
        if _flags(self.marker_fd) & _IMMUTABLE: raise RuntimeError('marker unexpectedly immutable')
        record={'kind':'das-journal-marker-v1','parent':list(self.parent.parent),
                'private':list(_identity(self.private_fd)),'old':list(old_id),
                'old_sha256':self.parent.journal_sha256,'marker':list(marker_id),
                'marker_sha256':hashlib.sha256(data).hexdigest()}
        self.intent_fd=os.open('marker-intent.json',os.O_WRONLY|os.O_CREAT|os.O_EXCL|os.O_NOFOLLOW|os.O_CLOEXEC,0o600,dir_fd=self.private_fd)
        raw=(json.dumps(record,sort_keys=True)+'\n').encode()
        if os.write(self.intent_fd,raw) != len(raw): raise RuntimeError('short marker intent write')
        os.fsync(self.intent_fd);os.fsync(self.private_fd)
        self.observer.begin_exchange()
        self.parent._check_parent();self.parent._check_journal()
        if _libc.renameat2(self.parent.parent_fd,b'operation.json',self.private_fd,b'marker',2):
            raise OSError(ctypes.get_errno(),'journal marker exchange')
        self.exchanged=True
        # Root-owned sticky parent protects marker during exchange-to-flag gap.
        _named(self.parent.parent_fd,'operation.json',marker_id)
        _named(self.private_fd,'marker',old_id)
        _set_immutable(self.marker_fd)
        os.fsync(self.parent.parent_fd);os.fsync(self.private_fd)
        self.assert_held(marker_id,old_id)
        self.observer.finish_exchange()
        self.assert_held(marker_id,old_id)
        return record

    def assert_held(self,marker_id,old_id):
        p=self.parent._check_parent()
        if (p.st_uid,p.st_gid,stat.S_IMODE(p.st_mode),_acl(self.parent.parent_fd)) != (0,972,0o1770,_FENCED_ACL):
            raise RuntimeError('sticky parent lost')
        s=_named(self.parent.parent_fd,'operation.json',tuple(marker_id))
        if (s.st_uid,s.st_gid,stat.S_IMODE(s.st_mode)) != (0,0,0o600) or not (_flags(self.marker_fd)&_IMMUTABLE):
            raise RuntimeError('immutable marker lost')
        old = _named(self.private_fd,'marker',tuple(old_id))
        held=os.fstat(self.parent.journal_fd)
        if ((held.st_dev,held.st_ino) != tuple(old_id) or (old.st_uid,old.st_gid,stat.S_IMODE(old.st_mode)) != (994,972,0o600)
                or (held.st_dev,held.st_ino,held.st_size,held.st_mtime_ns,held.st_ctime_ns) !=
                   (old.st_dev,old.st_ino,old.st_size,old.st_mtime_ns,old.st_ctime_ns)):
            raise RuntimeError('preserved journal changed')
        if held.st_size > 131072: raise RuntimeError('preserved journal too large')
        data=os.pread(self.parent.journal_fd,held.st_size+1,0)
        if len(data) != held.st_size or hashlib.sha256(data).hexdigest() != self.parent.journal_sha256:
            raise RuntimeError('preserved journal bytes differ')

    def close(self):
        if self.marker_fd is not None: os.close(self.marker_fd); self.marker_fd=None
        if self.intent_fd is not None: os.close(self.intent_fd); self.intent_fd=None
        os.close(self.private_fd)
