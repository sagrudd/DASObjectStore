"""Bounded journal-parent protection; not a complete recovery coordinator.

No command-line entry point. This helper refuses an existing intent and may
restore only before the journal name changes; post-exchange recovery belongs
to the separately reviewed transaction coordinator.
"""
import fcntl
import hashlib
import json
import os
import stat
import subprocess

_BASE_ACL = 'user::rwx\ngroup::r-x\nother::---\n\n'
_FENCED_ACL = 'user::rwx\nuser:994:rwx\ngroup::r-x\nmask::rwx\nother::---\n\n'


def _acl(fd):
    p = subprocess.run(['/usr/bin/getfacl', '-cpn', f'/proc/self/fd/{fd}'],
        pass_fds=(fd,), stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
        stderr=subprocess.PIPE, text=True, check=True, timeout=10)
    return p.stdout


def _set_acl(fd, text):
    subprocess.run(['/usr/bin/setfacl', '--set-file=-', f'/proc/self/fd/{fd}'],
        pass_fds=(fd,), input=text, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        text=True, check=True, timeout=10)


def _regular_at(dirfd, name):
    return os.stat(name, dir_fd=dirfd, follow_symlinks=False)


class StickyJournalParent:
    """Preserve an exact directory baseline and protect future root marker names."""
    def __init__(self, parent_fd, intent_dir_fd, expected_parent, expected_journal, journal_sha256):
        self.parent_fd = os.dup(parent_fd)
        self.intent_dir_fd = os.dup(intent_dir_fd)
        self.parent = tuple(expected_parent)
        self.journal = tuple(expected_journal)
        self.journal_sha256 = journal_sha256
        self.journal_fd = None
        self.lock_fd = None
        try:
            self._check_parent()
            self.journal_fd = os.open('operation.json',os.O_RDONLY|os.O_NOFOLLOW|os.O_CLOEXEC,dir_fd=self.parent_fd)
            self.journal_stat = os.fstat(self.journal_fd)
            self._check_journal()
        except BaseException:
            self.close()
            raise

    def _check_parent(self):
        s = os.fstat(self.parent_fd)
        if not stat.S_ISDIR(s.st_mode) or (s.st_dev, s.st_ino) != self.parent:
            raise RuntimeError('journal parent identity differs')
        return s

    def _check_journal(self):
        named = _regular_at(self.parent_fd, 'operation.json')
        held = os.fstat(self.journal_fd)
        initial = self.journal_stat
        def fields(s):
            return (s.st_dev,s.st_ino,s.st_size,s.st_mode,s.st_uid,s.st_gid,
                    s.st_nlink,s.st_mtime_ns,s.st_ctime_ns)
        if (not stat.S_ISREG(named.st_mode) or fields(named) != fields(held)
                or fields(held) != fields(initial) or held.st_nlink != 1
                or (held.st_dev,held.st_ino) != self.journal
                or (held.st_uid,held.st_gid,stat.S_IMODE(held.st_mode)) != (994,972,0o600)):
            raise RuntimeError('journal identity differs; manual reconciliation required')
        os.lseek(self.journal_fd,0,os.SEEK_SET)
        digest=hashlib.sha256()
        while True:
            chunk=os.read(self.journal_fd,65536)
            if not chunk: break
            digest.update(chunk)
        if digest.hexdigest() != self.journal_sha256 or fields(os.fstat(self.journal_fd)) != fields(initial):
            raise RuntimeError('journal bytes differ; manual reconciliation required')

    def _intent(self):
        s = os.fstat(self.intent_dir_fd)
        if not stat.S_ISDIR(s.st_mode) or s.st_uid != 0 or s.st_gid != 0 or stat.S_IMODE(s.st_mode) != 0o700:
            raise RuntimeError('intent directory is not root-private')
        return {'schema':'sticky-journal-parent-v1', 'parent':list(self.parent),
                'journal':list(self.journal), 'journal_sha256':self.journal_sha256, 'uid':994, 'gid':972,
                'mode':0o750, 'acl':_BASE_ACL}

    def acquire(self):
        if os.geteuid() != 0 or self.lock_fd is not None:
            raise RuntimeError('sticky parent fence unavailable')
        expected = self._intent()
        s = self._check_parent(); self._check_journal()
        if (s.st_uid,s.st_gid,stat.S_IMODE(s.st_mode),_acl(self.parent_fd)) != (994,972,0o750,_BASE_ACL):
            raise RuntimeError('journal parent baseline differs')
        fd = os.open('parent-intent.json', os.O_WRONLY|os.O_CREAT|os.O_EXCL|os.O_NOFOLLOW|os.O_CLOEXEC, 0o600, dir_fd=self.intent_dir_fd)
        self.lock_fd = fd
        try:
            fcntl.flock(fd, fcntl.LOCK_EX|fcntl.LOCK_NB)
            data = (json.dumps(expected, sort_keys=True)+'\n').encode()
            if os.write(fd,data) != len(data): raise RuntimeError('short intent write')
            os.fsync(fd); os.fsync(self.intent_dir_fd)
            self._check_journal(); os.fchown(self.parent_fd,0,972); self._check_journal()
            _set_acl(self.parent_fd,_FENCED_ACL); self._check_journal()
            os.fchmod(self.parent_fd,0o1770); self._check_journal()
            os.fsync(self.parent_fd)
            s=self._check_parent(); self._check_journal()
            if (s.st_uid,s.st_gid,stat.S_IMODE(s.st_mode),_acl(self.parent_fd)) != (0,972,0o1770,_FENCED_ACL):
                raise RuntimeError('sticky parent fence differs')
            return self
        except BaseException:
            # Before exchange, the original journal inode must still be named.
            self.restore_before_exchange()
            raise

    def _verified_intent(self):
        fd = os.open('parent-intent.json', os.O_RDONLY|os.O_NOFOLLOW|os.O_CLOEXEC, dir_fd=self.intent_dir_fd)
        keep = False
        try:
            s = os.fstat(fd)
            if not stat.S_ISREG(s.st_mode) or s.st_uid != 0 or s.st_gid != 0 or s.st_nlink != 1 or stat.S_IMODE(s.st_mode) != 0o600 or s.st_size > 16384:
                raise RuntimeError('intent file differs')
            if self.lock_fd is not None:
                held = os.fstat(self.lock_fd)
                if (held.st_dev, held.st_ino) != (s.st_dev, s.st_ino):
                    raise RuntimeError('intent inode changed')
            if self.lock_fd is None:
                fcntl.flock(fd, fcntl.LOCK_EX|fcntl.LOCK_NB)
                self.lock_fd = fd; keep = True
            data = b''
            while True:
                chunk = os.read(fd,16384)
                if not chunk: break
                data += chunk
                if len(data) > 16384: raise RuntimeError('intent too large')
            if json.loads(data) != self._intent():
                raise RuntimeError('intent binding differs')
        finally:
            if not keep: os.close(fd)

    def restore_before_exchange(self):
        """Restore only the unchanged original name; never clear a live marker."""
        self._verified_intent(); s=self._check_parent(); self._check_journal()
        current=(s.st_uid,s.st_gid,stat.S_IMODE(s.st_mode),_acl(self.parent_fd))
        allowed={(994,972,0o750,_BASE_ACL),(0,972,0o750,_BASE_ACL),
                 (0,972,0o770,_FENCED_ACL),(0,972,0o1770,_FENCED_ACL)}
        if current not in allowed:
            raise RuntimeError('journal parent state ambiguous; retain intent')
        self._check_journal(); os.fchown(self.parent_fd,994,972); self._check_journal()
        _set_acl(self.parent_fd,_BASE_ACL); self._check_journal()
        os.fchmod(self.parent_fd,0o750); self._check_journal(); os.fsync(self.parent_fd)
        s=self._check_parent(); self._check_journal()
        if (s.st_uid,s.st_gid,stat.S_IMODE(s.st_mode),_acl(self.parent_fd)) != (994,972,0o750,_BASE_ACL):
            raise RuntimeError('journal parent restoration differs')
        os.unlink('parent-intent.json',dir_fd=self.intent_dir_fd); os.fsync(self.intent_dir_fd)
        if self.lock_fd is not None: os.close(self.lock_fd); self.lock_fd=None

    def close(self):
        if self.lock_fd is not None: os.close(self.lock_fd); self.lock_fd=None
        if self.journal_fd is not None: os.close(self.journal_fd); self.journal_fd=None
        os.close(self.parent_fd); os.close(self.intent_dir_fd)
