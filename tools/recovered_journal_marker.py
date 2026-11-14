"""Requalify a previously installed original-inode journal marker after restart.

No CLI. The caller must independently authenticate the target, mount/path,
process identity, protected state directory and external writer exclusion.
"""
import fcntl
import hashlib
import json
import os
import signal
import stat
from housekeeping_generation_observer import ReportGenerationObserver
from sticky_journal_parent import _acl,_FENCED_ACL
from original_journal_marker import _flags,_IMMUTABLE

_EXPECTED_JOURNAL='57568bee1c28385f25798edf25fe30093de25473f5f91a863f8408472b15295d'


def _read_intent(dirfd,name):
    fd=os.open(name,os.O_RDONLY|os.O_NOFOLLOW|os.O_CLOEXEC,dir_fd=dirfd)
    try:
        before=os.fstat(fd)
        if not stat.S_ISREG(before.st_mode) or before.st_uid!=0 or before.st_gid!=0 or before.st_nlink!=1 or stat.S_IMODE(before.st_mode)!=0o600 or before.st_size>16384:
            raise RuntimeError('marker intent custody differs')
        raw=os.read(fd,16385);after=os.fstat(fd)
        if (before.st_dev,before.st_ino,before.st_size,before.st_mtime_ns,before.st_ctime_ns)!=(after.st_dev,after.st_ino,after.st_size,after.st_mtime_ns,after.st_ctime_ns):
            raise RuntimeError('marker intent changed')
        return json.loads(raw)
    finally:os.close(fd)


class RecoveredJournalMarker:
    def __init__(self,parent_path,state_fd):
        if os.geteuid()!=0:raise RuntimeError('root required')
        self.parent_fd=os.open(parent_path,os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW|os.O_CLOEXEC)
        self.state_fd=os.dup(state_fd);self.journal_fd=None;self.observer=None;self.previous_sigio=None;self.leased=False;self.broken=False
        try:
            state=os.fstat(self.state_fd)
            if not stat.S_ISDIR(state.st_mode) or state.st_uid!=0 or state.st_gid!=0 or stat.S_IMODE(state.st_mode)!=0o700:raise RuntimeError('state directory differs')
            intent=_read_intent(self.state_fd,'original-marker-intent.json')
            p_intent=_read_intent(self.state_fd,'parent-intent.json')
            keys={'kind','parent','journal','journal_sha256','uid','gid','mode','size','mtime_ns','ctime_ns','original_flags'}
            if (set(intent)!=keys or intent['kind']!='das-original-journal-marker-v1' or intent['journal_sha256']!=_EXPECTED_JOURNAL
                or (intent['uid'],intent['gid'],intent['mode'])!=(994,972,0o600)
                or p_intent.get('schema')!='sticky-journal-parent-v1' or p_intent.get('parent')!=intent['parent']
                or p_intent.get('journal')!=intent['journal'] or p_intent.get('journal_sha256')!=intent['journal_sha256']):
                raise RuntimeError('marker intents differ')
            self.intent=intent
            self.journal_fd=os.open('operation.json',os.O_RDONLY|os.O_NOFOLLOW|os.O_CLOEXEC,dir_fd=self.parent_fd)
            self.observer=ReportGenerationObserver(parent_path)
            if (os.fstat(self.observer.dirfd).st_dev,os.fstat(self.observer.dirfd).st_ino)!=tuple(intent['parent']):raise RuntimeError('observer parent differs')
            self.observer.arm_existing_marker()
            self.previous_sigio=signal.signal(signal.SIGIO,self._sigio)
            fcntl.fcntl(self.journal_fd,fcntl.F_SETLEASE,fcntl.F_RDLCK);self.leased=True
            self.recheck()
        except BaseException:
            self.close();raise
    def _sigio(self,*_):self.broken=True
    def recheck_without_observer(self):
        p=os.fstat(self.parent_fd);f=os.fstat(self.journal_fd);n=os.stat('operation.json',dir_fd=self.parent_fd,follow_symlinks=False)
        if ((p.st_dev,p.st_ino)!=tuple(self.intent['parent']) or (p.st_uid,p.st_gid,stat.S_IMODE(p.st_mode),_acl(self.parent_fd))!=(0,972,0o1770,_FENCED_ACL)
            or not stat.S_ISREG(n.st_mode) or n.st_nlink!=1 or (n.st_dev,n.st_ino)!=(f.st_dev,f.st_ino) or (f.st_dev,f.st_ino)!=tuple(self.intent['journal'])
            or (f.st_uid,f.st_gid,stat.S_IMODE(f.st_mode),f.st_size,f.st_mtime_ns)!=(0,0,0o600,self.intent['size'],self.intent['mtime_ns'])
            or _flags(self.journal_fd)!=(self.intent['original_flags']|_IMMUTABLE)):
            raise RuntimeError('persistent marker differs')
        data=os.pread(self.journal_fd,f.st_size+1,0)
        if len(data)!=f.st_size or hashlib.sha256(data).hexdigest()!=self.intent['journal_sha256']:
            raise RuntimeError('persistent journal bytes differ')
        if self.broken or not self.leased or fcntl.fcntl(self.journal_fd,fcntl.F_GETLEASE)!=fcntl.F_RDLCK:raise RuntimeError('marker lease lost')
    def recheck(self):
        self.recheck_without_observer()
        self.observer.assert_intact()
    def close(self):
        if self.leased:fcntl.fcntl(self.journal_fd,fcntl.F_SETLEASE,fcntl.F_UNLCK);self.leased=False
        if self.previous_sigio is not None:signal.signal(signal.SIGIO,self.previous_sigio);self.previous_sigio=None
        if self.observer is not None:self.observer.close();self.observer=None
        if self.journal_fd is not None:os.close(self.journal_fd);self.journal_fd=None
        if self.state_fd is not None:os.close(self.state_fd);self.state_fd=None
        if self.parent_fd is not None:os.close(self.parent_fd);self.parent_fd=None
