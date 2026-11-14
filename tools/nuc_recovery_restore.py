"""Post-stop restoration for the exact retained NUC repair.

No entry point: caller retains the reviewed promoted start fence. Any failure
leaves its masks and the durable restore record for explicit reconciliation.
"""
import array
import ctypes
import fcntl
import json
import os
import stat
from nuc_claim_target import RecoveryStopProof
from integrated_claim_recovery import _load_recovery_record, _flags, _IMMUTABLE
from sticky_journal_parent import _acl, _set_acl


def restore_after_stop(proof, held):
    from das_unit_start_fence import DasClientStartFence
    if type(proof) is not RecoveryStopProof or type(held) is not DasClientStartFence:
        raise RuntimeError('exact protected recovery and fence required')
    if not proof.target.daemon_stopped or held.journal_kind != 'das-promoted-client-fence-v1':
        raise RuntimeError('stopped promoted hold required')
    held.recheck(); proof.recheck()
    record = _load_recovery_record(proof.state_fd)
    if record['phase'] != 'committed': raise RuntimeError('recovery not committed')
    journal = getattr(proof.marker, 'journal_fd', None)
    if journal is None: journal = proof.marker.parent.journal_fd
    marker_parent = getattr(proof.marker, 'parent_fd', None)
    if marker_parent is None: marker_parent = proof.marker.parent.parent_fd
    private = os.fstat(proof.state_fd); parent = os.fstat(marker_parent)
    if private.st_dev != parent.st_dev: raise RuntimeError('journal archive must share filesystem')
    try: os.stat('restore.json', dir_fd=proof.state_fd, follow_symlinks=False)
    except FileNotFoundError: pass
    else: raise RuntimeError('existing restore requires reconciliation')
    def stage(name):
        raw=(json.dumps({'kind':'nuc-poststop-restore-v1','phase':name},sort_keys=True)+'\n').encode()
        fd=os.open('restore.new',os.O_WRONLY|os.O_CREAT|os.O_EXCL|os.O_NOFOLLOW|os.O_CLOEXEC,0o600,dir_fd=proof.state_fd)
        try:
            if os.write(fd,raw)!=len(raw): raise RuntimeError('short restore record')
            os.fsync(fd)
        finally: os.close(fd)
        os.rename('restore.new','restore.json',src_dir_fd=proof.state_fd,dst_dir_fd=proof.state_fd)
        os.fsync(proof.state_fd)
    def guard():
        proof.target.recheck(); held.recheck(); proof.backups.recheck()
    source=os.open('payload',os.O_RDONLY|os.O_NOFOLLOW|os.O_CLOEXEC,dir_fd=proof.source_parent_fd)
    try:
        source_saved=record['source']['file']; sp_saved=record['source']['parent']; dp_saved=record['partial']['parent']
        stage('before-journal-archive'); guard(); proof.marker.recheck()
        marker_stat=os.fstat(journal)
        flags=array.array('L',[proof.marker.intent['original_flags'] if hasattr(proof.marker,'intent') else proof.marker.original_flags])
        fcntl.ioctl(journal,0x40086602,flags,True); os.fsync(journal)
        libc=ctypes.CDLL(None,use_errno=True)
        libc.renameat2.argtypes=(ctypes.c_int,ctypes.c_char_p,ctypes.c_int,ctypes.c_char_p,ctypes.c_uint)
        if libc.renameat2(marker_parent,b'operation.json',proof.state_fd,b'original-journal',1):
            raise OSError(ctypes.get_errno(),'archive journal no-replace')
        os.fsync(marker_parent); os.fsync(proof.state_fd)
        archived=os.stat('original-journal',dir_fd=proof.state_fd,follow_symlinks=False)
        if (archived.st_dev,archived.st_ino)!=(marker_stat.st_dev,marker_stat.st_ino):raise RuntimeError('archived journal identity differs')
        stage('journal-archived'); guard()
        flags=array.array('L',[source_saved['flags']]); fcntl.ioctl(source,0x40086602,flags,True); os.fsync(source)
        os.fchown(source,source_saved['uid'],source_saved['gid']); os.fchmod(source,source_saved['mode']); _set_acl(source,source_saved['acl']); os.fsync(source)
        sf=os.fstat(source)
        if ((sf.st_uid,sf.st_gid,stat.S_IMODE(sf.st_mode),_acl(source),_flags(source)) !=
            (source_saved['uid'],source_saved['gid'],source_saved['mode'],source_saved['acl'],source_saved['flags'])):
            raise RuntimeError('source restoration differs')
        stage('source-restored'); guard()
        for fd,saved in ((proof.source_parent_fd,sp_saved),(proof.destination_parent_fd,dp_saved)):
            os.fchown(fd,saved['uid'],saved['gid']); os.fchmod(fd,saved['mode']); _set_acl(fd,saved['acl']); os.fsync(fd)
            current=os.fstat(fd)
            if (current.st_uid,current.st_gid,stat.S_IMODE(current.st_mode),_acl(fd)) != (saved['uid'],saved['gid'],saved['mode'],saved['acl']):
                raise RuntimeError('payload parent restoration differs')
        stage('payload-parents-restored'); guard()
        intent_fd=os.open('parent-intent.json',os.O_RDONLY|os.O_NOFOLLOW|os.O_CLOEXEC,dir_fd=proof.state_fd)
        try: parent_intent=json.loads(os.read(intent_fd,16385))
        finally: os.close(intent_fd)
        if parent_intent.get('schema')!='sticky-journal-parent-v1' or parent_intent.get('parent')!=[parent.st_dev,parent.st_ino]:
            raise RuntimeError('journal parent intent differs')
        os.fchown(marker_parent,parent_intent['uid'],parent_intent['gid']); os.fchmod(marker_parent,parent_intent['mode']); _set_acl(marker_parent,parent_intent['acl']); os.fsync(marker_parent)
        current=os.fstat(marker_parent)
        if (current.st_uid,current.st_gid,stat.S_IMODE(current.st_mode),_acl(marker_parent)) != (parent_intent['uid'],parent_intent['gid'],parent_intent['mode'],parent_intent['acl']):
            raise RuntimeError('journal parent restoration differs')
        stage('restored-daemon-still-fenced'); guard()
        return 'restored-daemon-still-fenced'
    finally: os.close(source)


def resume_restoration(target, held, backups, state_fd, source_parent_fd,
                       destination_parent_fd, quarantine_fd, journal_parent_fd):
    """Resume exact post-stop metadata restoration without restarting DAS."""
    from nuc_claim_target import HeldTarget
    from das_unit_start_fence import DasClientStartFence
    from recovered_journal_marker import _read_intent
    from integrated_claim_recovery import VerifiedBackups, _hash_fd, EXPECTED_ORIGINAL_JOURNAL_SHA256
    from claimed_recovery_transaction import validate_exact_claim
    import hashlib
    if type(target) is not HeldTarget or not target.daemon_stopped or type(held) is not DasClientStartFence or type(backups) is not VerifiedBackups:
        raise RuntimeError('exact stopped recovery custody required')
    def guard(): target.recheck(); held.recheck(); backups.recheck()
    guard()
    for name in ('restore.new','recovery.new'):
        try: os.stat(name,dir_fd=state_fd,follow_symlinks=False)
        except FileNotFoundError: pass
        else: raise RuntimeError('incomplete durable record requires attended reconciliation')
    try: prior=_read_intent(state_fd,'restore.json')
    except FileNotFoundError: prior=None
    if prior is not None and (set(prior)!={'kind','phase'} or prior['kind']!='nuc-poststop-restore-v1'
        or prior['phase'] not in ('before-journal-archive','journal-archived','source-restored',
                                  'payload-parents-restored','restored-daemon-still-fenced')):
        raise RuntimeError('restoration record differs')
    record=_load_recovery_record(state_fd)
    if record['phase']!='committed': raise RuntimeError('claim not committed')
    journal_intent=_read_intent(state_fd,'original-marker-intent.json')
    parent_intent=_read_intent(state_fd,'parent-intent.json')
    if (journal_intent.get('journal_sha256')!=EXPECTED_ORIGINAL_JOURNAL_SHA256
        or parent_intent.get('journal')!=journal_intent.get('journal') or parent_intent.get('parent')!=journal_intent.get('parent')):
        raise RuntimeError('retained journal intent differs')
    conn=target.connection
    if conn.in_transaction: raise RuntimeError('unexpected open transaction')
    conn.execute('BEGIN IMMEDIATE')
    try:
        if validate_exact_claim(conn,record['intent'],journal_present=True)!='released': raise RuntimeError('claim is not released')
        conn.rollback()
    except BaseException:
        if conn.in_transaction: conn.rollback()
        raise
    q=os.fstat(quarantine_fd); jp=os.fstat(journal_parent_fd)
    if ((q.st_dev,q.st_ino)!=tuple(record['quarantine']) or (q.st_uid,q.st_gid,stat.S_IMODE(q.st_mode))!=(0,0,0o700)
        or [jp.st_dev,jp.st_ino]!=journal_intent['parent'] or os.fstat(state_fd).st_dev!=jp.st_dev):
        raise RuntimeError('restoration directory identity differs')
    try: os.stat('payload',dir_fd=destination_parent_fd,follow_symlinks=False)
    except FileNotFoundError: pass
    else: raise RuntimeError('destination was recreated')
    source=os.open('payload',os.O_RDONLY|os.O_NOFOLLOW|os.O_CLOEXEC,dir_fd=source_parent_fd)
    try: partial=os.open('partial',os.O_RDONLY|os.O_NOFOLLOW|os.O_CLOEXEC,dir_fd=quarantine_fd)
    except BaseException:
        os.close(source)
        raise
    try:
        for name,fd in (('source',source),('partial',partial)):
            st=os.fstat(fd); old=record[name]['file']; row=backups.rows[name]
            if (not stat.S_ISREG(st.st_mode) or st.st_nlink!=1 or (st.st_dev,st.st_ino,st.st_size,st.st_mtime_ns)!=(row['device'],row['inode'],row['size'],old['mtime_ns'])
                or old['sha256']!=row['sha256'] or _hash_fd(fd,guard)!=row['sha256']):
                raise RuntimeError('restoration payload differs')
            if name=='partial' and (st.st_uid,st.st_gid,stat.S_IMODE(st.st_mode),_flags(fd))!=(0,0,0o640,old['flags']):
                raise RuntimeError('quarantined payload custody differs')
            if name=='source' and ((st.st_uid,st.st_gid) not in ((0,0),(old['uid'],old['gid']))
                or stat.S_IMODE(st.st_mode) not in (0o640,old['mode'])
                or _flags(fd) not in (old['flags'],old['flags']|_IMMUTABLE)):
                raise RuntimeError('source restoration state differs')
        def exists(fd,name):
            try:os.stat(name,dir_fd=fd,follow_symlinks=False);return True
            except FileNotFoundError:return False
        named=exists(journal_parent_fd,'operation.json'); archived=exists(state_fd,'original-journal')
        if named==archived: raise RuntimeError('ambiguous journal archive state')
        jfd=os.open('operation.json' if named else 'original-journal',os.O_RDONLY|os.O_NOFOLLOW|os.O_CLOEXEC,dir_fd=journal_parent_fd if named else state_fd)
        try:
            js=os.fstat(jfd)
            if (not stat.S_ISREG(js.st_mode) or js.st_nlink!=1 or [js.st_dev,js.st_ino]!=journal_intent['journal']
                or (js.st_uid,js.st_gid,stat.S_IMODE(js.st_mode))!=(0,0,0o600)
                or hashlib.sha256(os.pread(jfd,131073,0)).hexdigest()!=journal_intent['journal_sha256']):
                raise RuntimeError('original journal differs')
            if named:
                guard()
                flags=_flags(jfd)
                if flags not in (journal_intent['original_flags'],journal_intent['original_flags']|_IMMUTABLE):
                    raise RuntimeError('journal flags differ')
                value=array.array('L',[journal_intent['original_flags']]);fcntl.ioctl(jfd,0x40086602,value,True);os.fsync(jfd)
                libc=ctypes.CDLL(None,use_errno=True)
                libc.renameat2.argtypes=(ctypes.c_int,ctypes.c_char_p,ctypes.c_int,ctypes.c_char_p,ctypes.c_uint)
                if libc.renameat2(journal_parent_fd,b'operation.json',state_fd,b'original-journal',1):raise OSError(ctypes.get_errno(),'archive journal no-replace')
                os.fsync(journal_parent_fd);os.fsync(state_fd)
        finally: os.close(jfd)
        guard()
        saved=record['source']['file'];flags=_flags(source)
        if flags not in (saved['flags'],saved['flags']|_IMMUTABLE):raise RuntimeError('source flags differ')
        value=array.array('L',[saved['flags']]);fcntl.ioctl(source,0x40086602,value,True);os.fsync(source)
        os.fchown(source,saved['uid'],saved['gid']);os.fchmod(source,saved['mode']);_set_acl(source,saved['acl']);os.fsync(source)
        for fd,saved in ((source_parent_fd,record['source']['parent']),(destination_parent_fd,record['partial']['parent'])):
            guard(); current=os.fstat(fd)
            if (current.st_dev,current.st_ino)!=(saved['device'],saved['inode']):raise RuntimeError('payload parent identity differs')
            os.fchown(fd,saved['uid'],saved['gid']);os.fchmod(fd,saved['mode']);_set_acl(fd,saved['acl']);os.fsync(fd)
        guard();os.fchown(journal_parent_fd,parent_intent['uid'],parent_intent['gid']);os.fchmod(journal_parent_fd,parent_intent['mode']);_set_acl(journal_parent_fd,parent_intent['acl']);os.fsync(journal_parent_fd)
        source_saved=record['source']['file']; current=os.fstat(source)
        if ((current.st_uid,current.st_gid,stat.S_IMODE(current.st_mode),_flags(source),_acl(source)) !=
            (source_saved['uid'],source_saved['gid'],source_saved['mode'],source_saved['flags'],source_saved['acl'])):
            raise RuntimeError('source restoration differs')
        for fd,saved in ((source_parent_fd,record['source']['parent']),(destination_parent_fd,record['partial']['parent'])):
            current=os.fstat(fd)
            if ((current.st_dev,current.st_ino,current.st_uid,current.st_gid,stat.S_IMODE(current.st_mode),_acl(fd)) !=
                (saved['device'],saved['inode'],saved['uid'],saved['gid'],saved['mode'],saved['acl'])):
                raise RuntimeError('payload parent restoration differs')
        current=os.fstat(journal_parent_fd)
        if ((current.st_uid,current.st_gid,stat.S_IMODE(current.st_mode),_acl(journal_parent_fd)) !=
            (parent_intent['uid'],parent_intent['gid'],parent_intent['mode'],parent_intent['acl'])):
            raise RuntimeError('journal parent restoration differs')
        raw=(json.dumps({'kind':'nuc-poststop-restore-v1','phase':'restored-daemon-still-fenced'},sort_keys=True)+'\n').encode()
        fd=os.open('restore.new',os.O_WRONLY|os.O_CREAT|os.O_EXCL|os.O_NOFOLLOW|os.O_CLOEXEC,0o600,dir_fd=state_fd)
        try:
            if os.write(fd,raw)!=len(raw):raise RuntimeError('short restoration record')
            os.fsync(fd)
        finally:os.close(fd)
        os.rename('restore.new','restore.json',src_dir_fd=state_fd,dst_dir_fd=state_fd);os.fsync(state_fd)
        guard()
        return 'restored-daemon-still-fenced'
    finally:
        os.close(source);os.close(partial)
