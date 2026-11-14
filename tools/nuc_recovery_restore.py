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
