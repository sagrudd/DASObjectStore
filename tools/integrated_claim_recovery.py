"""Internal staged recovery transaction; not a standalone appliance command.

Requires independently authenticated target, retained backups, fixed intent,
client admission and owner/process identity.  The coordinator leaves the
journal marker and payload protections held after success or any uncertainty;
it does not authorize owner resumption or APT.
"""
import array
import ctypes
import datetime
import fcntl
import hashlib
import json
import os
import sqlite3
import struct
import stat
import time

from claimed_recovery_transaction import validate_exact_claim, release_exact_claim
from original_journal_marker import OriginalJournalMarker, _flags, _IMMUTABLE
from sticky_journal_parent import _acl

_SETFLAGS=0x40086602
_libc=ctypes.CDLL(None,use_errno=True)
_libc.renameat2.argtypes=(ctypes.c_int,ctypes.c_char_p,ctypes.c_int,ctypes.c_char_p,ctypes.c_uint)
_libc.renameat2.restype=ctypes.c_int


def _ident(s): return (s.st_dev,s.st_ino)
def _finger(s): return (s.st_dev,s.st_ino,s.st_size,s.st_uid,s.st_gid,s.st_mode,s.st_nlink,s.st_mtime_ns,s.st_ctime_ns)


def _hash_fd(fd,check):
    h=hashlib.sha256();offset=0
    while True:
        data=os.pread(fd,8*1024*1024,offset)
        if not data:break
        h.update(data);offset+=len(data);check()
    return h.hexdigest()


# The live entry point must bind this to the independently accepted receipt.
# Tests may override it only in their own disposable process.
EXPECTED_BACKUP_RECEIPT_SHA256 = '13c07d6d8e297359320eb95912ca73866178ca9172dde4cc6e33cacf4f8d853f'
EXPECTED_ORIGINAL_JOURNAL_SHA256 = '57568bee1c28385f25798edf25fe30093de25473f5f91a863f8408472b15295d'


class VerifiedBackups:
    """Prehash root-protected backup copies before taking a live hold."""
    def __init__(self,dirfd):
        self.dirfd=os.dup(dirfd);self.files={};self.rows={}
        try:
            directory=os.fstat(self.dirfd)
            if not stat.S_ISDIR(directory.st_mode) or directory.st_uid!=0 or directory.st_gid!=0 or stat.S_IMODE(directory.st_mode)!=0o700:
                raise RuntimeError('backup directory differs')
            receipt=os.open('receipt.json',os.O_RDONLY|os.O_NOFOLLOW|os.O_CLOEXEC,dir_fd=self.dirfd)
            try:
                before=os.fstat(receipt)
                if not stat.S_ISREG(before.st_mode) or before.st_uid!=0 or before.st_gid!=0 or before.st_nlink!=1 or before.st_mode&0o077 or before.st_size>16384:
                    raise RuntimeError('backup receipt custody differs')
                raw=os.read(receipt,16385)
                if _finger(os.fstat(receipt))!=_finger(before) or hashlib.sha256(raw).hexdigest()!=EXPECTED_BACKUP_RECEIPT_SHA256:
                    raise RuntimeError('backup receipt digest differs')
            finally:os.close(receipt)
            rows=json.loads(raw)
            if type(rows) is not list or len(rows)!=2 or {r.get('name') for r in rows}!={'source','partial'}:
                raise RuntimeError('backup receipt rows differ')
            for row in rows:
                if set(row)!={'name','size','sha256','device','inode'} or any(type(row[k]) is not int or row[k]<0 for k in ('size','device','inode')) or type(row['sha256']) is not str or len(row['sha256'])!=64:
                    raise RuntimeError('backup receipt row differs')
                fd=os.open(row['name'],os.O_RDONLY|os.O_NOFOLLOW|os.O_CLOEXEC,dir_fd=self.dirfd)
                self.files[row['name']]=fd;before=os.fstat(fd)
                if not stat.S_ISREG(before.st_mode) or before.st_uid!=0 or before.st_gid!=0 or before.st_nlink!=1 or stat.S_IMODE(before.st_mode)!=0o600 or before.st_size!=row['size']:
                    raise RuntimeError('backup copy custody differs')
                if _hash_fd(fd,lambda:None)!=row['sha256'] or _finger(os.fstat(fd))!=_finger(before):
                    raise RuntimeError('backup copy hash differs')
                self.rows[row['name']]=dict(row);self.rows[row['name']]['backup_fingerprint']=_finger(before)
            self.directory_identity=_ident(directory)
        except BaseException:
            self.close();raise
    def recheck(self):
        directory=os.fstat(self.dirfd)
        if _ident(directory)!=self.directory_identity or directory.st_uid!=0 or directory.st_gid!=0 or stat.S_IMODE(directory.st_mode)!=0o700:
            raise RuntimeError('backup directory changed')
        for name,fd in self.files.items():
            s=os.fstat(fd);named=os.stat(name,dir_fd=self.dirfd,follow_symlinks=False)
            if _finger(s)!=tuple(self.rows[name]['backup_fingerprint']) or _finger(named)!=_finger(s):
                raise RuntimeError('backup copy changed')
    def close(self):
        for fd in self.files.values():os.close(fd)
        self.files.clear()
        if self.dirfd is not None:os.close(self.dirfd);self.dirfd=None


class PayloadNameWatch:
    """Reject watched payload-name/parent changes outside exact local stages."""
    def __init__(self,dirfd):
        self.fd=_libc.inotify_init1(os.O_NONBLOCK|os.O_CLOEXEC)
        if self.fd<0:raise OSError(ctypes.get_errno(),'inotify_init1')
        mask=0x00000002|0x00000004|0x00000040|0x00000080|0x00000100|0x00000200|0x00000400|0x00000800
        self.wd=_libc.inotify_add_watch(self.fd,os.fsencode(f'/proc/self/fd/{dirfd}'),mask)
        if self.wd<0:
            err=ctypes.get_errno();os.close(self.fd);raise OSError(err,'inotify_add_watch')
    def consume(self,*,file_mask=0,parent_attrib=False):
        while True:
            try:raw=os.read(self.fd,65536)
            except BlockingIOError:return
            if not raw:raise RuntimeError('payload watch ended')
            pos=0
            while pos<len(raw):
                if len(raw)-pos<16:raise RuntimeError('truncated payload event')
                wd,mask,cookie,n=struct.unpack_from('iIII',raw,pos);pos+=16
                if len(raw)-pos<n:raise RuntimeError('truncated payload event name')
                name=raw[pos:pos+n].split(b'\0',1)[0];pos+=n
                if mask&(0x00004000|0x00008000|0x00000400|0x00000800) or wd!=self.wd:
                    raise RuntimeError('payload directory watch lost')
                relevant=mask & ~0x40000000
                if name==b'payload' and (not file_mask or relevant & ~file_mask):
                    raise RuntimeError('unexpected payload-name change')
                if name==b'' and relevant & 0x00000004 and not parent_attrib:
                    raise RuntimeError('unexpected payload-parent change')
    def close(self):os.close(self.fd)


class Payload:
    """Pinned directory and payload descriptors plus authenticated backup row."""
    def __init__(self, directory_fd, file_fd, row):
        self.directory_fd=os.dup(directory_fd);self.file_fd=os.dup(file_fd);self.row=dict(row)
        self.before=os.fstat(self.file_fd);self.parent_before=os.fstat(self.directory_fd)
        self.file_acl=_acl(self.file_fd);self.parent_acl=_acl(self.directory_fd)
        self.flags=_flags(self.file_fd);self.leased=False
        self.watch=PayloadNameWatch(self.directory_fd)
        try:self.check_original()
        except BaseException:
            self.watch.close();os.close(self.file_fd);os.close(self.directory_fd);raise
    def check_original(self):
        f=os.fstat(self.file_fd);p=os.fstat(self.directory_fd)
        n=os.stat('payload',dir_fd=self.directory_fd,follow_symlinks=False)
        if (not stat.S_ISDIR(p.st_mode) or not stat.S_ISREG(f.st_mode) or f.st_nlink!=1
            or _ident(n)!=_ident(f) or not stat.S_ISREG(n.st_mode)
            or (f.st_dev,f.st_ino,f.st_size)!=(self.row['device'],self.row['inode'],self.row['size'])
            or (f.st_uid,f.st_gid)!=(994,972) or _finger(f)!=_finger(self.before)
            or _ident(p)!=_ident(self.parent_before) or self.flags & (_IMMUTABLE|0x20)):
            raise RuntimeError('payload original differs')
    def acquire_lease(self):
        fcntl.fcntl(self.file_fd,fcntl.F_SETLEASE,fcntl.F_RDLCK);self.leased=True
    def check_lease(self):
        if not self.leased or fcntl.fcntl(self.file_fd,fcntl.F_GETLEASE)!=fcntl.F_RDLCK:
            raise RuntimeError('payload lease lost')
    def snapshot(self):
        f=self.before;p=self.parent_before
        return {'file':{'device':f.st_dev,'inode':f.st_ino,'uid':f.st_uid,'gid':f.st_gid,'mode':stat.S_IMODE(f.st_mode),'size':f.st_size,'mtime_ns':f.st_mtime_ns,'ctime_ns':f.st_ctime_ns,'acl':self.file_acl,'flags':self.flags,'sha256':self.row['sha256']},
                'parent':{'device':p.st_dev,'inode':p.st_ino,'uid':p.st_uid,'gid':p.st_gid,'mode':stat.S_IMODE(p.st_mode),'acl':self.parent_acl}}
    def close(self):
        if self.leased:
            fcntl.fcntl(self.file_fd,fcntl.F_SETLEASE,fcntl.F_UNLCK);self.leased=False
        self.watch.close();os.close(self.file_fd);os.close(self.directory_fd)


class IntegratedClaimRecovery:
    """Execute only with fixed, independently authenticated NUC bindings."""
    def __init__(self,marker,backups,source,partial,state_fd,quarantine_fd,conn,intent):
        if type(marker) is not OriginalJournalMarker or type(backups) is not VerifiedBackups or type(source) is not Payload or type(partial) is not Payload:
            raise RuntimeError('exact reviewed hold types required')
        state=os.fstat(state_fd);q=os.fstat(quarantine_fd)
        if (not stat.S_ISDIR(state.st_mode) or state.st_uid!=0 or state.st_gid!=0 or stat.S_IMODE(state.st_mode)!=0o700
            or not stat.S_ISDIR(q.st_mode) or q.st_uid!=0 or q.st_gid!=0 or stat.S_IMODE(q.st_mode)!=0o700
            or q.st_dev!=partial.before.st_dev):raise RuntimeError('private state or quarantine differs')
        if conn.in_transaction:raise RuntimeError('connection already has transaction')
        if marker.parent.journal_sha256 != EXPECTED_ORIGINAL_JOURNAL_SHA256:
            raise RuntimeError('original journal digest differs')
        raw=os.pread(marker.parent.journal_fd,131073,0)
        if len(raw)>131072 or hashlib.sha256(raw).hexdigest()!=EXPECTED_ORIGINAL_JOURNAL_SHA256:
            raise RuntimeError('original journal bytes differ')
        operation=json.loads(raw)
        if (operation.get('schema')!='dasobjectstore.storage_assurance.operation.v1'
            or operation.get('phase')!='claimed' or operation.get('action')!='rotate'
            or operation.get('candidate')!=intent.get('candidate')
            or operation.get('claim_owner')!=intent['claim']['owner_id']
            or operation.get('destination_disk_id')!=intent['claim']['disk_id']
            or operation.get('destination_relative_path')!=intent['candidate']['relative_path']):
            raise RuntimeError('transaction intent differs from original journal')
        try:
            if datetime.datetime.strptime(intent['release_time'],'%Y-%m-%dT%H:%M:%SZ').strftime('%Y-%m-%dT%H:%M:%SZ') != intent['release_time']:
                raise ValueError('noncanonical')
        except (KeyError,TypeError,ValueError) as exc:raise RuntimeError('release time differs') from exc
        backups.recheck()
        original_devices={source.before.st_dev,partial.before.st_dev}
        for name,item in (('source',source),('partial',partial)):
            if any(item.row[k]!=backups.rows[name][k] for k in ('name','size','sha256','device','inode')):
                raise RuntimeError('payload not bound to retained backup')
            if os.fstat(backups.files[name]).st_dev in original_devices:
                raise RuntimeError('backup not on independent device')
            fs=intent['filesystem'][name];parent=item.parent_before
            if (set(fs)!={'parent_device','parent_inode','parent_uid','parent_gid','parent_mode','parent_acl','file_uid','file_gid','file_mode','file_acl','file_flags'}
                or (parent.st_dev,parent.st_ino,parent.st_uid,parent.st_gid,stat.S_IMODE(parent.st_mode),item.parent_acl)!=(fs['parent_device'],fs['parent_inode'],fs['parent_uid'],fs['parent_gid'],fs['parent_mode'],fs['parent_acl'])
                or (item.before.st_uid,item.before.st_gid,stat.S_IMODE(item.before.st_mode),item.file_acl,item.flags)!=(fs['file_uid'],fs['file_gid'],fs['file_mode'],fs['file_acl'],fs['file_flags'])):
                raise RuntimeError('authenticated filesystem baseline differs')
        self.marker=marker;self.backups=backups;self.source=source;self.partial=partial;self.state_fd=os.dup(state_fd);self.qfd=os.dup(quarantine_fd)
        self.conn=conn;self.intent=intent;self.record=None
    def _write(self,phase):
        self.record['phase']=phase
        raw=(json.dumps(self.record,sort_keys=True)+'\n').encode()
        fd=os.open('recovery.new',os.O_WRONLY|os.O_CREAT|os.O_EXCL|os.O_NOFOLLOW|os.O_CLOEXEC,0o600,dir_fd=self.state_fd)
        try:
            if os.write(fd,raw)!=len(raw):raise RuntimeError('short recovery state')
            os.fsync(fd)
        finally:os.close(fd)
        os.rename('recovery.new','recovery.json',src_dir_fd=self.state_fd,dst_dir_fd=self.state_fd)
        os.fsync(self.state_fd)
    def _check(self):
        self.marker.recheck();self.backups.recheck()
        for item in (self.source,self.partial):
            item.check_lease();item.watch.consume()
    def _verify_name(self,item,name='payload',dirfd=None):
        parent_fd=item.directory_fd if dirfd is None else dirfd
        named=os.stat(name,dir_fd=parent_fd,follow_symlinks=False)
        held=os.fstat(item.file_fd)
        if (not stat.S_ISREG(named.st_mode) or named.st_nlink!=1 or held.st_nlink!=1
            or _ident(named)!=_ident(held) or _ident(held)!=_ident(item.before)
            or held.st_size!=item.before.st_size or held.st_mtime_ns!=item.before.st_mtime_ns):
            raise RuntimeError('payload name or inode changed')

    def _protect_file(self,item,label):
        self._write('before-'+label+'-chown');self._check();self._verify_name(item)
        os.fchown(item.file_fd,0,0);os.fchmod(item.file_fd,0o640);os.fsync(item.file_fd)
        item.watch.consume(file_mask=0x00000004)
        self._check();self._verify_name(item)
        f=os.fstat(item.file_fd);n=os.stat('payload',dir_fd=item.directory_fd,follow_symlinks=False)
        if (_ident(f)!=_ident(item.before) or _ident(n)!=_ident(f) or f.st_uid!=0 or f.st_gid!=0
            or stat.S_IMODE(f.st_mode)!=0o640 or f.st_size!=item.before.st_size or f.st_mtime_ns!=item.before.st_mtime_ns):
            raise RuntimeError('protected payload differs')
        self._write('before-'+label+'-immutable');self._check()
        flags=array.array('L',[item.flags|_IMMUTABLE]);fcntl.ioctl(item.file_fd,_SETFLAGS,flags,True);os.fsync(item.file_fd)
        item.watch.consume(file_mask=0x00000004)
        if _flags(item.file_fd)!=(item.flags|_IMMUTABLE):raise RuntimeError('payload immutable differs')
        self._check();self._verify_name(item);self._write(label+'-immutable')
    def _protect_parent(self,item,label):
        self._write('before-'+label+'-parent');self._check()
        p=os.fstat(item.directory_fd)
        if _ident(p)!=_ident(item.parent_before) or _acl(item.directory_fd)!=item.parent_acl:
            raise RuntimeError('payload parent baseline differs')
        # Remove daemon traversal before transferring directory ownership.
        os.fchmod(item.directory_fd,0o500);self._verify_name(item)
        os.fchown(item.directory_fd,0,0);os.fchmod(item.directory_fd,0o500);os.fsync(item.directory_fd)
        item.watch.consume(parent_attrib=True)
        p=os.fstat(item.directory_fd)
        if _ident(p)!=_ident(item.parent_before) or (p.st_uid,p.st_gid,stat.S_IMODE(p.st_mode))!=(0,0,0o500):
            raise RuntimeError('payload parent fence differs')
        self._check();self._verify_name(item);self._write(label+'-parent-held')
    def run(self,timeout=900):
        self.marker.recheck()
        generations=self.marker.observer.wait_two(timeout)
        self.marker.recheck()
        self.source.acquire_lease()
        try:
            self.partial.acquire_lease();self._check()
            for item in (self.source,self.partial):
                item.check_original();self._check()
            self.record={'kind':'das-integrated-recovery-v1','intent':self.intent,'source':self.source.snapshot(),
                         'partial':self.partial.snapshot(),'quarantine':list(_ident(os.fstat(self.qfd))),
                         'generations':generations,'phase':'initial'}
            # Refuse replacement of a prior attempt.
            try:os.stat('recovery.json',dir_fd=self.state_fd,follow_symlinks=False)
            except FileNotFoundError:pass
            else:raise RuntimeError('recovery state already exists')
            self._write('prepared')
            self.conn.execute('BEGIN IMMEDIATE')
            try:
                if self.conn.execute("SELECT COUNT(*) FROM sqlite_master WHERE type='trigger'").fetchone()[0] != 0:
                    raise RuntimeError('unexpected SQLite triggers')
                if validate_exact_claim(self.conn,self.intent,journal_present=True)!='active':
                    raise RuntimeError('claim is not active for initial run')
                self._protect_parent(self.source,'source');self._protect_parent(self.partial,'partial')
                self._verify_name(self.source);self._verify_name(self.partial)
                self._protect_file(self.source,'source');self._protect_file(self.partial,'partial')
                # Immutable inode flags and root-only parent fences, not a
                # lease timeout, protect content during potentially stalled I/O.
                for item in (self.source,self.partial):
                    before=os.fstat(item.file_fd);last=[time.monotonic()]
                    def hash_check():
                        if self.marker.broken:raise RuntimeError('lease break observed')
                        if time.monotonic()-last[0]>=2:self._check();last[0]=time.monotonic()
                    if _hash_fd(item.file_fd,hash_check)!=item.row['sha256'] or _finger(os.fstat(item.file_fd))!=_finger(before):
                        raise RuntimeError('protected payload hash differs from retained backup')
                    self._check();self._verify_name(item)
                self._write('before-quarantine');self._check()
                self._verify_name(self.source);self._verify_name(self.partial)
                flags=array.array('L',[self.partial.flags]);fcntl.ioctl(self.partial.file_fd,_SETFLAGS,flags,True);os.fsync(self.partial.file_fd)
                self.partial.watch.consume(file_mask=0x00000004)
                if _flags(self.partial.file_fd)!=self.partial.flags:raise RuntimeError('partial flag clear differs')
                self._check();self._verify_name(self.source);self._verify_name(self.partial)
                if _libc.renameat2(self.partial.directory_fd,b'payload',self.qfd,b'partial',1):
                    raise OSError(ctypes.get_errno(),'quarantine no-replace')
                self.partial.watch.consume(file_mask=0x00000040)
                os.fsync(self.partial.directory_fd);os.fsync(self.qfd)
                q=os.stat('partial',dir_fd=self.qfd,follow_symlinks=False)
                if _ident(q)!=_ident(self.partial.before) or not stat.S_ISREG(q.st_mode):raise RuntimeError('quarantine inode differs')
                self._write('quarantined');self._check()
                self._verify_name(self.source);self._verify_name(self.partial,'partial',self.qfd)
                try:os.stat('payload',dir_fd=self.partial.directory_fd,follow_symlinks=False)
                except FileNotFoundError:pass
                else:raise RuntimeError('destination unexpectedly exists')
                self._write('before-commit');self._check()
                self._verify_name(self.source);self._verify_name(self.partial,'partial',self.qfd)
                try:os.stat('payload',dir_fd=self.partial.directory_fd,follow_symlinks=False)
                except FileNotFoundError:pass
                else:raise RuntimeError('destination unexpectedly exists before commit')
                release_exact_claim(self.conn,self.intent,journal_present=True)
                self.conn.commit()
                self._write('committed');self._check()
            except BaseException:
                if self.conn.in_transaction:self.conn.rollback()
                raise
            return {'state':'committed-marker-retained','generations':generations}
        except BaseException:
            # No automatic rollback of filesystem metadata or marker.  Durable
            # intent and independently preserved copies remain for recovery.
            raise
    def close(self):
        self.source.close();self.partial.close();os.close(self.state_fd);os.close(self.qfd)


def resume_quarantined(marker,backups,source_parent_fd,destination_parent_fd,quarantine_fd,state_fd,conn):
    """Resume only an authenticated post-quarantine checkpoint; keep marker held.

    Caller independently authenticates target, process, named mounts and DB.
    Any pre-quarantine or ambiguous state is deliberately denied.
    """
    from recovered_journal_marker import RecoveredJournalMarker
    if type(marker) is not RecoveredJournalMarker or type(backups) is not VerifiedBackups or conn.in_transaction:
        raise RuntimeError('exact recovered hold types required')
    st=os.fstat(state_fd)
    if not stat.S_ISDIR(st.st_mode) or st.st_uid!=0 or st.st_gid!=0 or stat.S_IMODE(st.st_mode)!=0o700:
        raise RuntimeError('recovery state directory differs')
    try:os.stat('recovery.new',dir_fd=state_fd,follow_symlinks=False)
    except FileNotFoundError:pass
    else:raise RuntimeError('incomplete recovery state write')
    fd=os.open('recovery.json',os.O_RDONLY|os.O_NOFOLLOW|os.O_CLOEXEC,dir_fd=state_fd)
    try:
        before=os.fstat(fd)
        if not stat.S_ISREG(before.st_mode) or before.st_uid!=0 or before.st_gid!=0 or before.st_nlink!=1 or stat.S_IMODE(before.st_mode)!=0o600 or before.st_size>131072:
            raise RuntimeError('recovery state custody differs')
        raw=os.read(fd,131073);after=os.fstat(fd)
        if _finger(before)!=_finger(after):raise RuntimeError('recovery state changed')
    finally:os.close(fd)
    record=json.loads(raw)
    if set(record)!={'kind','intent','source','partial','quarantine','generations','phase'} or record['kind']!='das-integrated-recovery-v1' or record['phase'] not in ('before-quarantine','quarantined','before-commit','committed'):
        raise RuntimeError('not a post-quarantine checkpoint')
    generations=record['generations']
    if (type(generations) is not list or len(generations)!=2 or any(type(x) is not dict or set(x)!={'identity','sha256'} or type(x['identity']) is not list or len(x['identity'])!=2 or type(x['sha256']) is not str or len(x['sha256'])!=64 for x in generations)
        or generations[0]['identity']==generations[1]['identity']):raise RuntimeError('report evidence differs')
    intent=record['intent'];backups.recheck();marker.recheck()
    operation=json.loads(os.pread(marker.journal_fd,131073,0))
    if (operation.get('schema')!='dasobjectstore.storage_assurance.operation.v1' or operation.get('phase')!='claimed' or operation.get('action')!='rotate'
        or operation.get('candidate')!=intent.get('candidate') or operation.get('claim_owner')!=intent['claim']['owner_id']
        or operation.get('destination_disk_id')!=intent['claim']['disk_id'] or operation.get('destination_relative_path')!=intent['candidate']['relative_path']):
        raise RuntimeError('recovery intent differs from journal')
    for name,dirfd in (('source',source_parent_fd),('partial',destination_parent_fd)):
        p=os.fstat(dirfd);original=record[name]['parent']
        if (p.st_dev,p.st_ino,p.st_uid,p.st_gid,stat.S_IMODE(p.st_mode))!=(original['device'],original['inode'],0,0,0o500):
            raise RuntimeError('protected payload parent differs')
    q=os.fstat(quarantine_fd)
    if (not stat.S_ISDIR(q.st_mode) or (q.st_dev,q.st_ino)!=tuple(record['quarantine']) or q.st_uid!=0 or q.st_gid!=0 or stat.S_IMODE(q.st_mode)!=0o700):
        raise RuntimeError('quarantine custody differs')
    try:os.stat('payload',dir_fd=destination_parent_fd,follow_symlinks=False)
    except FileNotFoundError:pass
    else:raise RuntimeError('destination unexpectedly exists')
    opened=[]
    try:
        for name,dirfd,entry,immutable in (('source',source_parent_fd,'payload',True),('partial',quarantine_fd,'partial',False)):
            fd=os.open(entry,os.O_RDONLY|os.O_NOFOLLOW|os.O_CLOEXEC,dir_fd=dirfd);opened.append(fd)
            current=os.fstat(fd);saved=record[name]['file'];row=backups.rows[name]
            if (not stat.S_ISREG(current.st_mode) or current.st_nlink!=1 or (current.st_dev,current.st_ino,current.st_size)!=(row['device'],row['inode'],row['size'])
                or (current.st_dev,current.st_ino,current.st_size)!=(saved['device'],saved['inode'],saved['size'])
                or (current.st_uid,current.st_gid,stat.S_IMODE(current.st_mode))!=(0,0,0o640) or current.st_mtime_ns!=saved['mtime_ns']
                or _flags(fd)!=(saved['flags']|(_IMMUTABLE if immutable else 0)) or saved['sha256']!=row['sha256']):
                raise RuntimeError('protected payload differs')
            last=[time.monotonic()]
            def hash_check():
                if marker.broken:raise RuntimeError('marker lease break observed')
                if time.monotonic()-last[0]>=2:
                    marker.recheck();last[0]=time.monotonic()
            if _hash_fd(fd,hash_check)!=row['sha256'] or _finger(os.fstat(fd))!=_finger(current):
                raise RuntimeError('protected payload bytes changed')
        backups.recheck();marker.recheck()
        # A crash can occur after rename but before either directory fsync.
        # The exact absent/present inode checks above make both directories
        # safe to sync again before recording a pending database commit.
        os.fsync(destination_parent_fd);os.fsync(quarantine_fd)
        marker.recheck();backups.recheck()
        previous_phase=record['phase']
        conn.execute('BEGIN IMMEDIATE')
        try:
            if conn.execute("SELECT COUNT(*) FROM sqlite_master WHERE type='trigger'").fetchone()[0] != 0:
                raise RuntimeError('unexpected SQLite triggers')
            observed=validate_exact_claim(conn,intent,journal_present=True)
            if previous_phase=='committed' and observed!='released':raise RuntimeError('committed claim reverted')
            if previous_phase in ('before-quarantine','quarantined') and observed!='active':
                raise RuntimeError('claim advanced before commit checkpoint')
            if previous_phase!='committed':
                record['phase']='before-commit';_write_recovery_record(state_fd,record)
            marker.recheck();backups.recheck()
            release_exact_claim(conn,intent,journal_present=True)
            conn.commit()
        except BaseException:
            if conn.in_transaction:conn.rollback()
            raise
        record['phase']='committed';_write_recovery_record(state_fd,record)
        marker.recheck();backups.recheck()
        return 'committed-marker-retained'
    finally:
        for fd in opened:os.close(fd)


def _write_recovery_record(state_fd,record):
    raw=(json.dumps(record,sort_keys=True)+'\n').encode()
    fd=os.open('recovery.new',os.O_WRONLY|os.O_CREAT|os.O_EXCL|os.O_NOFOLLOW|os.O_CLOEXEC,0o600,dir_fd=state_fd)
    try:
        if os.write(fd,raw)!=len(raw):raise RuntimeError('short recovery state')
        os.fsync(fd)
    finally:os.close(fd)
    os.rename('recovery.new','recovery.json',src_dir_fd=state_fd,dst_dir_fd=state_fd);os.fsync(state_fd)


_EARLY_PHASES = frozenset(('prepared','before-source-parent','source-parent-held',
    'before-partial-parent','partial-parent-held','before-source-chown',
    'before-source-immutable','source-immutable','before-partial-chown',
    'before-partial-immutable','partial-immutable','before-quarantine'))


def _acl_entries(text):
    return tuple(line.split('#',1)[0].strip() for line in text.splitlines() if line.strip())


def _acl_for_mode(text,mode):
    entries=list(_acl_entries(text));has_mask=any(x.startswith('mask::') for x in entries)
    def perms(bits):return ''.join(c if bits & b else '-' for c,b in (('r',4),('w',2),('x',1)))
    out=[]
    for entry in entries:
        if entry.startswith('default:'):out.append(entry)
        elif entry.startswith('user::'):out.append('user::'+perms((mode>>6)&7))
        elif entry.startswith('mask::'):out.append('mask::'+perms((mode>>3)&7))
        elif entry.startswith('group::') and not has_mask:out.append('group::'+perms((mode>>3)&7))
        elif entry.startswith('other::'):out.append('other::'+perms(mode&7))
        else:out.append(entry)
    return tuple(out)


def _load_recovery_record(state_fd):
    st=os.fstat(state_fd)
    if not stat.S_ISDIR(st.st_mode) or (st.st_uid,st.st_gid,stat.S_IMODE(st.st_mode))!=(0,0,0o700):
        raise RuntimeError('recovery state directory differs')
    try:os.stat('recovery.new',dir_fd=state_fd,follow_symlinks=False)
    except FileNotFoundError:pass
    else:raise RuntimeError('incomplete recovery state write')
    fd=os.open('recovery.json',os.O_RDONLY|os.O_NOFOLLOW|os.O_CLOEXEC,dir_fd=state_fd)
    try:
        before=os.fstat(fd)
        if not stat.S_ISREG(before.st_mode) or before.st_uid!=0 or before.st_gid!=0 or before.st_nlink!=1 or stat.S_IMODE(before.st_mode)!=0o600 or before.st_size>131072:
            raise RuntimeError('recovery state custody differs')
        raw=os.read(fd,131073)
        if _finger(os.fstat(fd))!=_finger(before):raise RuntimeError('recovery state changed')
    finally:os.close(fd)
    record=json.loads(raw)
    if set(record)!={'kind','intent','source','partial','quarantine','generations','phase'} or record['kind']!='das-integrated-recovery-v1':
        raise RuntimeError('recovery record differs')
    return record


def resume_early(marker,backups,source_parent_fd,destination_parent_fd,quarantine_fd,state_fd,conn):
    """Requalify recognized pre-quarantine stages, then complete under lock.

    Named paths, mount ancestry, process/worker continuity and client admission
    are independently held by the caller. Ambiguous states retain the marker.
    """
    from recovered_journal_marker import RecoveredJournalMarker
    if type(marker) is not RecoveredJournalMarker or type(backups) is not VerifiedBackups or conn.in_transaction:
        raise RuntimeError('exact recovered hold types required')
    record=_load_recovery_record(state_fd);phase=record['phase']
    if phase not in _EARLY_PHASES and phase not in ('quarantined','before-commit','committed'):
        raise RuntimeError('unknown recovery checkpoint')
    q=os.fstat(quarantine_fd)
    if not stat.S_ISDIR(q.st_mode) or (q.st_dev,q.st_ino)!=tuple(record['quarantine']) or (q.st_uid,q.st_gid,stat.S_IMODE(q.st_mode))!=(0,0,0o700):
        raise RuntimeError('quarantine custody differs')
    def exists(fd,name):
        try:os.stat(name,dir_fd=fd,follow_symlinks=False);return True
        except FileNotFoundError:return False
    original_present=exists(destination_parent_fd,'payload');quarantined=exists(quarantine_fd,'partial')
    if quarantined and not original_present:
        if phase not in ('before-quarantine','quarantined','before-commit','committed'):
            raise RuntimeError('quarantine conflicts with recorded phase')
        return resume_quarantined(marker,backups,source_parent_fd,destination_parent_fd,quarantine_fd,state_fd,conn)
    if not original_present or quarantined or phase not in _EARLY_PHASES:
        raise RuntimeError('ambiguous partial pathname state')
    generations=record['generations']
    if (type(generations) is not list or len(generations)!=2 or any(type(x) is not dict or set(x)!={'identity','sha256'} or type(x['identity']) is not list or len(x['identity'])!=2 or type(x['sha256']) is not str or len(x['sha256'])!=64 for x in generations)
        or generations[0]['identity']==generations[1]['identity']):raise RuntimeError('report evidence differs')
    intent=record['intent'];backups.recheck();marker.recheck()
    operation=json.loads(os.pread(marker.journal_fd,131073,0))
    if (operation.get('schema')!='dasobjectstore.storage_assurance.operation.v1' or operation.get('phase')!='claimed' or operation.get('action')!='rotate'
        or operation.get('candidate')!=intent.get('candidate') or operation.get('claim_owner')!=intent['claim']['owner_id']
        or operation.get('destination_disk_id')!=intent['claim']['disk_id'] or operation.get('destination_relative_path')!=intent['candidate']['relative_path']):
        raise RuntimeError('recovery intent differs from journal')
    items=[]
    try:
        for name,dfd in (('source',source_parent_fd),('partial',destination_parent_fd)):
            saved=record[name];old=saved['file'];par=saved['parent'];row=backups.rows[name]
            fs=intent['filesystem'][name]
            if ((old['device'],old['inode'],old['size'],old['sha256'])!=(row['device'],row['inode'],row['size'],row['sha256'])
                or (par['device'],par['inode'],par['uid'],par['gid'],par['mode'],par['acl'])!=(fs['parent_device'],fs['parent_inode'],fs['parent_uid'],fs['parent_gid'],fs['parent_mode'],fs['parent_acl'])
                or (old['uid'],old['gid'],old['mode'],old['acl'],old['flags'])!=(fs['file_uid'],fs['file_gid'],fs['file_mode'],fs['file_acl'],fs['file_flags'])
                or os.fstat(backups.files[name]).st_dev in (record['source']['file']['device'],record['partial']['file']['device'])):
                raise RuntimeError('authenticated recovery baseline differs')
            watch=PayloadNameWatch(dfd)
            try:fd=os.open('payload',os.O_RDONLY|os.O_NOFOLLOW|os.O_CLOEXEC,dir_fd=dfd)
            except BaseException:watch.close();raise
            item={'name':name,'dirfd':dfd,'fd':fd,'watch':watch,'file':old,'parent':par,'leased':False}
            items.append(item)
        def check_item(item):
            p=os.fstat(item['dirfd']);f=os.fstat(item['fd']);n=os.stat('payload',dir_fd=item['dirfd'],follow_symlinks=False);old=item['file'];par=item['parent']
            if (not stat.S_ISDIR(p.st_mode) or (p.st_dev,p.st_ino)!=(par['device'],par['inode'])
                or not stat.S_ISREG(f.st_mode) or f.st_nlink!=1 or not stat.S_ISREG(n.st_mode) or n.st_nlink!=1
                or (f.st_dev,f.st_ino,f.st_size,f.st_mtime_ns)!=(old['device'],old['inode'],old['size'],old['mtime_ns'])
                or _ident(n)!=_ident(f)):
                raise RuntimeError('recovery payload identity differs')
            pmode=stat.S_IMODE(p.st_mode);fmode=stat.S_IMODE(f.st_mode)
            pshape=(p.st_uid,p.st_gid,pmode)
            if pshape==(par['uid'],par['gid'],par['mode']):pacl=_acl_entries(par['acl'])
            elif pshape in ((par['uid'],par['gid'],0o500),(0,0,0o500)):pacl=_acl_for_mode(par['acl'],0o500)
            else:raise RuntimeError('ambiguous recovery parent state')
            if _acl_entries(_acl(item['dirfd']))!=pacl:raise RuntimeError('recovery parent ACL differs')
            fshape=(f.st_uid,f.st_gid,fmode);flags=_flags(item['fd'])
            if fshape==(old['uid'],old['gid'],old['mode']) and flags==old['flags']:
                facl=_acl_entries(old['acl'])
            elif fshape==(0,0,old['mode']) and flags==old['flags']:
                facl=_acl_entries(old['acl'])
            elif fshape==(0,0,0o640) and flags in (old['flags'],old['flags']|_IMMUTABLE):
                facl=_acl_for_mode(old['acl'],0o640)
            else:raise RuntimeError('ambiguous recovery file state')
            if _acl_entries(_acl(item['fd']))!=facl:raise RuntimeError('recovery file ACL differs')
            return p,f,flags
        def check_all():
            marker.recheck();backups.recheck()
            for item in items:
                if not item['leased'] or fcntl.fcntl(item['fd'],fcntl.F_GETLEASE)!=fcntl.F_RDLCK:
                    raise RuntimeError('recovery payload lease lost')
                item['watch'].consume();check_item(item)
        def shape(item):
            p,f,flags=check_item(item);par=item['parent'];old=item['file']
            if (p.st_uid,p.st_gid,stat.S_IMODE(p.st_mode))==(par['uid'],par['gid'],par['mode']): ps='original'
            elif (p.st_uid,p.st_gid,stat.S_IMODE(p.st_mode))==(par['uid'],par['gid'],0o500): ps='masked'
            elif (p.st_uid,p.st_gid,stat.S_IMODE(p.st_mode))==(0,0,0o500): ps='fenced'
            else:raise RuntimeError('unknown parent state')
            if (f.st_uid,f.st_gid)==(old['uid'],old['gid']): fs='original'
            elif flags&_IMMUTABLE:fs='immutable'
            elif stat.S_IMODE(f.st_mode)!=0o640:fs='chowned'
            else:fs='root'
            return ps,fs
        O={'original'};P={'original','masked','fenced'};F={'fenced'};C={'original','chowned','root'};R={'root','immutable'};I={'immutable'}
        shapes={
            'prepared':(O,O,O,O),
            'before-source-parent':(P,O,O,O),
            'source-parent-held':(F,O,O,O),
            'before-partial-parent':(F,P,O,O),
            'partial-parent-held':(F,F,O,O),
            'before-source-chown':(F,F,C,O),
            'before-source-immutable':(F,F,R,O),
            'source-immutable':(F,F,I,O),
            'before-partial-chown':(F,F,I,C),
            'before-partial-immutable':(F,F,I,R),
            'partial-immutable':(F,F,I,I),
            'before-quarantine':(F,F,I,R),
        }
        source_shape=shape(items[0]);partial_shape=shape(items[1])
        observed=(source_shape[0],partial_shape[0],source_shape[1],partial_shape[1])
        if any(value not in allowed for value,allowed in zip(observed,shapes[phase])):
            raise RuntimeError('checkpoint metadata order differs')
        for item in items:
            fcntl.fcntl(item['fd'],fcntl.F_SETLEASE,fcntl.F_RDLCK);item['leased']=True
        check_all()
        conn.execute('BEGIN IMMEDIATE')
        try:
            if conn.execute("SELECT COUNT(*) FROM sqlite_master WHERE type='trigger'").fetchone()[0] != 0:
                raise RuntimeError('unexpected SQLite triggers')
            if validate_exact_claim(conn,intent,journal_present=True)!='active':raise RuntimeError('early checkpoint claim is not active')
            ordered=('prepared','before-source-parent','source-parent-held','before-partial-parent','partial-parent-held','before-source-chown','before-source-immutable','source-immutable','before-partial-chown','before-partial-immutable','partial-immutable','before-quarantine','quarantined','before-commit','committed')
            def advance(next_phase):
                if ordered.index(next_phase)>=ordered.index(record['phase']):
                    record['phase']=next_phase;_write_recovery_record(state_fd,record)
            for item in items:
                name=item['name'];p,_,_=check_item(item)
                if (p.st_uid,p.st_gid,stat.S_IMODE(p.st_mode))!=(0,0,0o500):
                    advance('before-'+name+'-parent');check_all()
                    os.fchmod(item['dirfd'],0o500);os.fchown(item['dirfd'],0,0);os.fchmod(item['dirfd'],0o500);os.fsync(item['dirfd'])
                    item['watch'].consume(parent_attrib=True);check_all()
                advance(name+'-parent-held');check_all()
            for item in items:
                name=item['name'];_,f,flags=check_item(item)
                if (f.st_uid,f.st_gid,stat.S_IMODE(f.st_mode))!=(0,0,0o640):
                    if flags&_IMMUTABLE:raise RuntimeError('immutable unprotected payload state')
                    advance('before-'+name+'-chown');check_all()
                    os.fchown(item['fd'],0,0);os.fchmod(item['fd'],0o640);os.fsync(item['fd'])
                    item['watch'].consume(file_mask=0x00000004);check_all()
                _,_,flags=check_item(item)
                if not flags&_IMMUTABLE:
                    advance('before-'+name+'-immutable');check_all()
                    value=array.array('L',[item['file']['flags']|_IMMUTABLE]);fcntl.ioctl(item['fd'],_SETFLAGS,value,True);os.fsync(item['fd'])
                    item['watch'].consume(file_mask=0x00000004);check_all()
                advance(name+'-immutable');check_all()
            for item in items:
                before=os.fstat(item['fd']);last=[time.monotonic()]
                def hash_check():
                    if marker.broken:raise RuntimeError('lease break observed')
                    if time.monotonic()-last[0]>=2:check_all();last[0]=time.monotonic()
                if _hash_fd(item['fd'],hash_check)!=item['file']['sha256'] or _finger(os.fstat(item['fd']))!=_finger(before):
                    raise RuntimeError('protected recovery payload hash differs')
                check_all()
            advance('before-quarantine');check_all()
            partial=items[1];value=array.array('L',[partial['file']['flags']]);fcntl.ioctl(partial['fd'],_SETFLAGS,value,True);os.fsync(partial['fd'])
            partial['watch'].consume(file_mask=0x00000004);check_all()
            if _libc.renameat2(destination_parent_fd,b'payload',quarantine_fd,b'partial',1):raise OSError(ctypes.get_errno(),'quarantine no-replace')
            partial['watch'].consume(file_mask=0x00000040);os.fsync(destination_parent_fd);os.fsync(quarantine_fd)
            named=os.stat('partial',dir_fd=quarantine_fd,follow_symlinks=False)
            if _ident(named)!=_ident(os.fstat(partial['fd'])) or not stat.S_ISREG(named.st_mode):raise RuntimeError('quarantine inode differs')
            advance('quarantined')
            marker.recheck();backups.recheck()
            # Source remains at its protected exact pathname; destination is absent.
            check_item(items[0]);items[0]['watch'].consume()
            if exists(destination_parent_fd,'payload'):raise RuntimeError('destination unexpectedly exists')
            advance('before-commit')
            marker.recheck();backups.recheck()
            for current in items:
                if fcntl.fcntl(current['fd'],fcntl.F_GETLEASE)!=fcntl.F_RDLCK:raise RuntimeError('recovery payload lease lost')
                current['watch'].consume()
            check_item(items[0])
            named=os.stat('partial',dir_fd=quarantine_fd,follow_symlinks=False)
            if _ident(named)!=_ident(os.fstat(items[1]['fd'])) or exists(destination_parent_fd,'payload'):
                raise RuntimeError('quarantine or destination changed before commit')
            release_exact_claim(conn,intent,journal_present=True)
            conn.commit()
        except BaseException:
            if conn.in_transaction:conn.rollback()
            raise
        advance('committed');marker.recheck();backups.recheck()
        return 'committed-marker-retained'
    finally:
        for item in items:
            if item['leased']:
                try:fcntl.fcntl(item['fd'],fcntl.F_SETLEASE,fcntl.F_UNLCK)
                except OSError:pass
            item['watch'].close();os.close(item['fd'])
