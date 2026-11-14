"""Promote the exact original journal inode to a root-only immutable gate.

No CLI. This component does not authorize recovery, mutation of claims, or
owner resumption. The parent and caller must independently authenticate target,
backups, process identities, client hold and external writer exclusion.
"""
import array
import fcntl
import hashlib
import json
import os
import signal
import stat

from sticky_journal_parent import StickyJournalParent, _acl, _FENCED_ACL
from housekeeping_generation_observer import ReportGenerationObserver

_GETFLAGS=0x80086601
_SETFLAGS=0x40086602
_IMMUTABLE=0x10


def _flags(fd):
    data=array.array('L',[0]);fcntl.ioctl(fd,_GETFLAGS,data,True);return data[0]


class OriginalJournalMarker:
    """Protect a fixed original inode; failures retain intent and current state."""
    def __init__(self,parent,observer):
        if type(parent) is not StickyJournalParent or type(observer) is not ReportGenerationObserver:
            raise RuntimeError('exact parent and observer required')
        if os.geteuid()!=0 or parent.lock_fd is None: raise RuntimeError('parent not held')
        if (os.fstat(observer.dirfd).st_dev,os.fstat(observer.dirfd).st_ino) != parent.parent:
            raise RuntimeError('observer parent differs')
        self.parent=parent;self.observer=observer;self.intent_fd=None;self.leased=False
        self.broken=False;self.previous_sigio=None;self.original_flags=None

    def _sigio(self,*_): self.broken=True

    def _check_lease(self):
        if self.broken or not self.leased or fcntl.fcntl(self.parent.journal_fd,fcntl.F_GETLEASE)!=fcntl.F_RDLCK:
            raise RuntimeError('journal read lease lost')

    def _check_marked(self,immutable):
        p=self.parent._check_parent();named=os.stat('operation.json',dir_fd=self.parent.parent_fd,follow_symlinks=False)
        held=os.fstat(self.parent.journal_fd);initial=self.parent.journal_stat
        if (not stat.S_ISREG(named.st_mode) or named.st_nlink!=1
            or (named.st_dev,named.st_ino)!=(held.st_dev,held.st_ino)
            or (held.st_dev,held.st_ino)!=(initial.st_dev,initial.st_ino)
            or (held.st_size,held.st_mtime_ns)!=(initial.st_size,initial.st_mtime_ns)
            or (held.st_uid,held.st_gid,stat.S_IMODE(held.st_mode))!=(0,0,0o600)
            or held.st_size>131072): raise RuntimeError('protected original journal differs')
        data=os.pread(self.parent.journal_fd,held.st_size+1,0)
        if len(data)!=held.st_size or hashlib.sha256(data).hexdigest()!=self.parent.journal_sha256:
            raise RuntimeError('protected original journal bytes differ')
        if immutable and _flags(self.parent.journal_fd) != (self.original_flags|_IMMUTABLE):
            raise RuntimeError('original journal flags differ')
        self._check_lease()
        return p

    def install(self):
        if self.intent_fd is not None: raise RuntimeError('promotion already attempted')
        self.parent._verified_intent();self.parent._check_journal()
        p=self.parent._check_parent()
        if (p.st_uid,p.st_gid,stat.S_IMODE(p.st_mode),_acl(self.parent.parent_fd))!=(0,972,0o1770,_FENCED_ACL):
            raise RuntimeError('sticky parent not held')
        initial=self.parent.journal_stat
        original_flags=_flags(self.parent.journal_fd)
        if original_flags & (_IMMUTABLE | 0x20): raise RuntimeError('original journal flags already restrict writes')
        self.original_flags=original_flags
        intent={'kind':'das-original-journal-marker-v1','parent':list(self.parent.parent),
                'journal':list(self.parent.journal),'journal_sha256':self.parent.journal_sha256,
                'uid':initial.st_uid,'gid':initial.st_gid,'mode':stat.S_IMODE(initial.st_mode),
                'size':initial.st_size,'mtime_ns':initial.st_mtime_ns,'ctime_ns':initial.st_ctime_ns,
                'original_flags':original_flags}
        self.intent_fd=os.open('original-marker-intent.json',os.O_WRONLY|os.O_CREAT|os.O_EXCL|os.O_NOFOLLOW|os.O_CLOEXEC,0o600,dir_fd=self.parent.intent_dir_fd)
        data=(json.dumps(intent,sort_keys=True)+'\n').encode()
        if os.write(self.intent_fd,data)!=len(data): raise RuntimeError('short marker intent')
        os.fsync(self.intent_fd);os.fsync(self.parent.intent_dir_fd)
        self.observer.begin_inode_marker()
        self.previous_sigio=signal.signal(signal.SIGIO,self._sigio)
        fcntl.fcntl(self.parent.journal_fd,fcntl.F_SETLEASE,fcntl.F_RDLCK);self.leased=True
        self._check_lease();self.parent._check_journal()
        # Temporarily remove UID994 parent write while preserving traverse/read.
        os.fchmod(self.parent.parent_fd,0o1550);os.fsync(self.parent.parent_fd)
        p=self.parent._check_parent()
        if (p.st_uid,p.st_gid,stat.S_IMODE(p.st_mode))!=(0,972,0o1550): raise RuntimeError('parent write denial differs')
        self._check_lease();self.parent._check_journal()
        os.fchown(self.parent.journal_fd,0,0);os.fchmod(self.parent.journal_fd,0o600)
        self._check_marked(False)
        flags=array.array('L',[original_flags|_IMMUTABLE]);fcntl.ioctl(self.parent.journal_fd,_SETFLAGS,flags,True)
        os.fsync(self.parent.journal_fd);self._check_marked(True)
        os.fchmod(self.parent.parent_fd,0o1770);os.fsync(self.parent.parent_fd)
        p=self._check_marked(True)
        if (p.st_uid,p.st_gid,stat.S_IMODE(p.st_mode),_acl(self.parent.parent_fd))!=(0,972,0o1770,_FENCED_ACL):
            raise RuntimeError('report-write parent differs')
        self.observer.finish_inode_marker();self._check_marked(True)
        return intent

    def recheck_without_observer(self):
        p=self._check_marked(True)
        if (p.st_uid,p.st_gid,stat.S_IMODE(p.st_mode),_acl(self.parent.parent_fd))!=(0,972,0o1770,_FENCED_ACL):
            raise RuntimeError('report-write parent lost')

    def recheck(self):
        for _ in range(2):
            self.recheck_without_observer()
            self.observer.assert_intact()

    def close(self):
        # Closing the lease does not clear the immutable marker or intent.
        if self.leased:
            fcntl.fcntl(self.parent.journal_fd,fcntl.F_SETLEASE,fcntl.F_UNLCK);self.leased=False
        if self.previous_sigio is not None:
            signal.signal(signal.SIGIO,self.previous_sigio);self.previous_sigio=None
        if self.intent_fd is not None:os.close(self.intent_fd);self.intent_fd=None
