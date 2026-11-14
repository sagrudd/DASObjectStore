"""Exact NUC claimed-repair coordinator. No APT or daemon stop.

On any failure after the client fence begins, masks and journal are retained
for attended reconciliation. This is not an independently authorized caller;
source and inputs must be protected and reviewed before execution.
"""
import datetime
import json
import os
import stat
import time
from contextlib import contextmanager
from pathlib import Path
from das_unit_start_fence import DasClientStartFence
from nuc_claim_target import HeldTarget, SOURCE_PARENT, DESTINATION_PARENT, JOURNAL_PARENT, BACKUP_PARENT
from sticky_journal_parent import StickyJournalParent, _acl
from housekeeping_generation_observer import ReportGenerationObserver
from original_journal_marker import OriginalJournalMarker, _flags
from integrated_claim_recovery import VerifiedBackups, Payload, IntegratedClaimRecovery
from claimed_recovery_transaction import COLUMNS, validate_exact_claim

STATE_PARENT='/var/lib/mnemosyne-terraform'
STATE_NAME='int07-claimed-recovery-20261114-v1'
QUARANTINE_NAME='.int07-claimed-recovery-20261114-v1'
JOURNAL_SHA='57568bee1c28385f25798edf25fe30093de25473f5f91a863f8408472b15295d'


class HeldRecovery:
    """In-process custody; the context owns every referenced descriptor."""
    def __init__(self, target, fence, marker, backups, state_fd, quarantine_fd,
                 source_parent_fd, destination_parent_fd, result, restored):
        self.target=target; self.fence=fence; self.marker=marker; self.backups=backups
        self.state_fd=state_fd; self.quarantine_fd=quarantine_fd
        self.source_parent_fd=source_parent_fd; self.destination_parent_fd=destination_parent_fd
        self.result=result; self.restored=restored
        self.source_identity=self._identity(source_parent_fd,'payload')
        self.partial_identity=self._identity(quarantine_fd,'partial')
        self.archive_identity=self._identity(state_fd,'original-journal') if restored else None
        self.recheck()

    @staticmethod
    def _identity(parent,name):
        entry=os.stat(name,dir_fd=parent,follow_symlinks=False)
        if not stat.S_ISREG(entry.st_mode) or entry.st_nlink!=1:
            raise RuntimeError('held file identity differs')
        return (entry.st_dev,entry.st_ino,entry.st_size,entry.st_mtime_ns)

    def recheck(self):
        self.target.recheck(); self.fence.recheck(); self.backups.recheck()
        if self._identity(self.source_parent_fd,'payload')!=self.source_identity or self._identity(self.quarantine_fd,'partial')!=self.partial_identity:
            raise RuntimeError('retained payload changed')
        try: os.stat('payload',dir_fd=self.destination_parent_fd,follow_symlinks=False)
        except FileNotFoundError: pass
        else: raise RuntimeError('destination was recreated')
        if self.restored:
            if self._identity(self.state_fd,'original-journal')!=self.archive_identity:
                raise RuntimeError('archived journal changed')
        else:
            self.marker.recheck()

    def begin_apt(self, audited_units_fd):
        """Retain the verified running image across the package pathname swap."""
        if not self.restored: raise RuntimeError('recovery restoration is required')
        self.recheck()
        self.fence.bind_apt_verifier(self,audited_units_fd)
        self.recheck()


def _mkdir_private(parent_fd,name):
    os.mkdir(name,0o700,dir_fd=parent_fd); os.fsync(parent_fd)
    fd=os.open(name,os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW|os.O_CLOEXEC,dir_fd=parent_fd)
    s=os.fstat(fd)
    if (s.st_uid,s.st_gid,stat.S_IMODE(s.st_mode))!=(0,0,0o700):
        os.close(fd); raise RuntimeError('private directory custody differs')
    return fd


def _open_private(parent_fd,name):
    fd=os.open(name,os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW|os.O_CLOEXEC,dir_fd=parent_fd)
    s=os.fstat(fd)
    if (s.st_uid,s.st_gid,stat.S_IMODE(s.st_mode))!=(0,0,0o700):
        os.close(fd);raise RuntimeError('existing private directory custody differs')
    return fd


def _make_intent(target,backups,journal_fd,sp,dp,sfd,dfd):
    raw=os.pread(journal_fd,131073,0)
    import hashlib
    if hashlib.sha256(raw).hexdigest()!=JOURNAL_SHA: raise RuntimeError('journal differs')
    operation=json.loads(raw)
    conn=target.connection
    conn.execute('BEGIN')
    try:
        rows=conn.execute('SELECT '+','.join(COLUMNS)+',state,released_at_utc,lease_owner,lease_expires_at_utc,updated_at_utc FROM disk_capacity_claims WHERE claim_kind=? AND owner_id=?',('repair',operation['claim_owner'])).fetchall()
        active=[row for row in rows if row[-5]=='active' and row[-4] is None]
        if len(active)!=1: raise RuntimeError('exact active claim unavailable')
        row=active[0]
        intent={'candidate':operation['candidate'],'claim':dict(zip(COLUMNS,row[:len(COLUMNS)])),
                'source_sha256':backups.rows['source']['sha256'], 'original_lease':list(row[-3:-1]),
                'original_updated_at':row[-1],
                'release_time':datetime.datetime.now(datetime.timezone.utc).strftime('%Y-%m-%dT%H:%M:%SZ'), 'filesystem':{}}
        for name,dirfd,filefd in (('source',sp,sfd),('partial',dp,dfd)):
            d=os.fstat(dirfd);f=os.fstat(filefd)
            intent['filesystem'][name]={'parent_device':d.st_dev,'parent_inode':d.st_ino,'parent_uid':d.st_uid,'parent_gid':d.st_gid,'parent_mode':stat.S_IMODE(d.st_mode),'parent_acl':_acl(dirfd),'file_uid':f.st_uid,'file_gid':f.st_gid,'file_mode':stat.S_IMODE(f.st_mode),'file_acl':_acl(filefd),'file_flags':_flags(filefd)}
        if validate_exact_claim(conn,intent,journal_present=True)!='active':raise RuntimeError('claim differs')
        conn.rollback()
    except BaseException:
        if conn.in_transaction: conn.rollback()
        raise
    return intent


def _promoted_hold_exists():
    """Inspect only the fixed protected fence journal; never modify it."""
    from das_unit_start_fence import STATE
    from nuc_claim_target import HeldPath
    try: directory=HeldPath(str(STATE),directory=True)
    except FileNotFoundError: return False
    try:
        parent=os.fstat(directory.fds[-1])
        if (parent.st_uid,parent.st_gid,stat.S_IMODE(parent.st_mode))!=(0,0,0o700):
            raise RuntimeError('fence journal parent differs')
        try: fd=os.open('journal.json',os.O_RDONLY|os.O_NOFOLLOW|os.O_CLOEXEC,dir_fd=directory.fds[-1])
        except FileNotFoundError:return False
        try:
            st=os.fstat(fd)
            if not stat.S_ISREG(st.st_mode) or st.st_uid!=0 or st.st_gid!=0 or st.st_nlink!=1 or st.st_mode&0o077 or st.st_size>131072:
                raise RuntimeError('fence journal custody differs')
            data=json.loads(os.read(fd,131073))
        finally:os.close(fd)
        if data.get('kind')=='das-promoted-client-fence-v1':
            if data.get('phase')!='held':raise RuntimeError('partial promotion requires attended reconciliation')
            return True
        return False
    finally:directory.close()


def _authorization_guard(authorization_end_ns,authorize):
    if type(authorization_end_ns) is not int or not callable(authorize):
        raise RuntimeError('signed authorization deadline required')
    last_full_check=[0]
    def current(*, force=False):
        now=time.monotonic_ns()
        if now>=authorization_end_ns:raise RuntimeError('signed authorization deadline expired')
        if force or now-last_full_check[0]>=10_000_000_000:
            authorize()
            now=time.monotonic_ns()
            if now>=authorization_end_ns:raise RuntimeError('signed authorization deadline expired')
            last_full_check[0]=now
    return current


def _observation_budget(authorization_end_ns):
    # Reserve fifteen minutes for the protected payload checks, recovery,
    # owner preparation and APT. This is a conservative bound, not a promise
    # those stages will finish; the signed deadline is still enforced.
    remaining=(authorization_end_ns-time.monotonic_ns())/1e9
    budget=min(2700.0,remaining-900.0)
    if budget<120.0:raise RuntimeError('insufficient signed time for observation and completion')
    return budget


@contextmanager
def hold_claim_recovery(*, stop_and_restore=False, authorization_end_ns, authorize):
    """Release the claim and optionally stop/fence DAS and restore metadata."""
    if os.geteuid()!=0: raise RuntimeError('root required')
    current=_authorization_guard(authorization_end_ns,authorize)
    current(force=True)
    # This entry point deliberately has no generic caller-supplied paths,
    # adapters, callbacks, or substituted target/owner proof.
    stopped=_promoted_hold_exists()
    target=HeldTarget(stopped=stopped) if stopped else HeldTarget()
    # Every held-target recheck guards a state transition. Backup hashing uses
    # the lightweight cadence above; transitions force the full check.
    target.authorization_check=lambda:current(force=True)
    backups=None; held=None; marker=None; guard=None; observer=None; coord=None; source=None; partial=None
    restored=False
    fds=[]
    try:
        backupfd=os.dup(target.paths[BACKUP_PARENT].fds[-1]);fds.append(backupfd)
        backups=VerifiedBackups(backupfd,check=current)  # whole backup hashes before hold
        target.recheck()
        if not stopped:_observation_budget(authorization_end_ns)
        from nuc_claim_target import HeldPath
        base=HeldPath(STATE_PARENT,directory=True);fds.append(base)
        base_stat=os.fstat(base.fds[-1])
        if base_stat.st_uid!=0 or base_stat.st_gid!=0 or base_stat.st_mode&0o022:
            raise RuntimeError('state parent custody differs')
        root=target.paths['/srv/dasobjectstore/hdd/qnap-1058'].fds[-1]
        try:
            state=_open_private(base.fds[-1],STATE_NAME)
            existing=True
        except FileNotFoundError:
            state=_mkdir_private(base.fds[-1],STATE_NAME)
            existing=False
        fds.append(state)
        quarantine=_open_private(root,QUARANTINE_NAME) if existing else _mkdir_private(root,QUARANTINE_NAME)
        fds.append(quarantine)
        sp=os.dup(target.paths[SOURCE_PARENT].fds[-1]); dp=os.dup(target.paths[DESTINATION_PARENT].fds[-1]); jp=os.dup(target.paths[JOURNAL_PARENT].fds[-1]);fds.extend((sp,dp,jp))
        if stopped:
            if not existing:raise RuntimeError('stopped hold without retained recovery state')
            from nuc_recovery_restore import resume_restoration
            held=DasClientStartFence().recover_promoted(
                handover_guard=lambda: (target.recheck(), backups.recheck()))
            result={'recovery':'already-committed',
                    'restore':resume_restoration(target,held,backups,state,sp,dp,quarantine,jp,
                                                 authorization_end_ns=authorization_end_ns)}
            restored=True
        elif existing:
            from recovered_journal_marker import RecoveredJournalMarker
            from integrated_claim_recovery import resume_early
            for name in ('restore.json','restore.new'):
                try: os.stat(name,dir_fd=state,follow_symlinks=False)
                except FileNotFoundError: pass
                else: raise RuntimeError('post-stop restoration requires attended reconciliation')
            held=DasClientStartFence().recover_client_held()
            def check(): target.recheck(); held.recheck()
            check()
            marker=RecoveredJournalMarker(JOURNAL_PARENT,state)
            try: os.stat('recovery.json',dir_fd=state,follow_symlinks=False)
            except FileNotFoundError:
                try: os.stat('recovery.new',dir_fd=state,follow_symlinks=False)
                except FileNotFoundError: pass
                else: raise RuntimeError('incomplete recovery record requires attended reconciliation')
                if os.listdir(quarantine):raise RuntimeError('pre-observation quarantine is not empty')
                budget=_observation_budget(authorization_end_ns)
                sfd=os.open('payload',os.O_RDONLY|os.O_NOFOLLOW|os.O_CLOEXEC,dir_fd=sp)
                fds.append(sfd)
                dfd=os.open('payload',os.O_RDONLY|os.O_NOFOLLOW|os.O_CLOEXEC,dir_fd=dp)
                fds.append(dfd)
                intent=_make_intent(target,backups,marker.journal_fd,sp,dp,sfd,dfd)
                source=Payload(sp,sfd,backups.rows['source']);partial=Payload(dp,dfd,backups.rows['partial'])
                coord=IntegratedClaimRecovery(marker,backups,source,partial,state,quarantine,target.connection,intent,external_check=check)
                result=coord.run(timeout=budget)
            else:
                result=resume_early(marker,backups,sp,dp,quarantine,state,target.connection,external_check=check)
        else:
            sfd=os.open('payload',os.O_RDONLY|os.O_NOFOLLOW|os.O_CLOEXEC,dir_fd=sp);dfd=os.open('payload',os.O_RDONLY|os.O_NOFOLLOW|os.O_CLOEXEC,dir_fd=dp);fds.extend((sfd,dfd))
            journal_fd=os.open('operation.json',os.O_RDONLY|os.O_NOFOLLOW|os.O_CLOEXEC,dir_fd=jp);fds.append(journal_fd)
            jstat=os.fstat(journal_fd);pstat=os.fstat(jp)
            intent=_make_intent(target,backups,journal_fd,sp,dp,sfd,dfd)
            _observation_budget(authorization_end_ns)
            current(force=True);held=DasClientStartFence().acquire()
            def check(): target.recheck(); held.recheck()
            check()
            guard=StickyJournalParent(jp,state,(pstat.st_dev,pstat.st_ino),(jstat.st_dev,jstat.st_ino),JOURNAL_SHA);guard.acquire()
            observer=ReportGenerationObserver(JOURNAL_PARENT)
            marker=OriginalJournalMarker(guard,observer);marker.install()
            source=Payload(sp,sfd,backups.rows['source']);partial=Payload(dp,dfd,backups.rows['partial'])
            coord=IntegratedClaimRecovery(marker,backups,source,partial,state,quarantine,target.connection,intent,external_check=check)
            result=coord.run(timeout=_observation_budget(authorization_end_ns))
        if stop_and_restore and not restored:
            from nuc_claim_target import RecoveryStopProof
            from nuc_recovery_restore import restore_after_stop
            proof=RecoveryStopProof(target,marker,backups,state,sp,dp,quarantine)
            held.promote_after_recovery(proof)
            result={'recovery':result, 'restore':restore_after_stop(proof,held)}
            restored=True
        # The consumer runs within this scope; its lock and descriptors remain
        # live. On exit masks remain and require an explicit reviewed release.
        yield HeldRecovery(target,held,marker,backups,state,quarantine,sp,dp,result,restored)
    finally:
        if coord is not None: coord.close()
        else:
            if source is not None:source.close()
            if partial is not None:partial.close()
        if marker is not None:marker.close()
        if observer is not None:observer.close()
        if guard is not None:guard.close()
        if held is not None and held.fd is not None:os.close(held.fd);held.fd=None
        if backups is not None:backups.close()
        for fd in reversed(fds):
            if isinstance(fd,int):os.close(fd)
            else:fd.close()
        target.close()

if __name__=='__main__':
    raise SystemExit('requires the reviewed in-process maintenance consumer')
