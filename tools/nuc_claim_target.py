"""Read-only identity binding for the retained NUC repair.

This verifies identity, not quiescence. Callers must separately acquire the
reviewed worker and filesystem protections before any database mutation.
"""
from contextlib import ExitStack
import hashlib
import json
import os
from pathlib import Path
import sqlite3
import stat
import subprocess

MACHINE_SHA256 = "f2edb8b0dcc2b721a167cacd271f24f54c7608de0b352cb9271cf7433adab5fb"
SCHEMA_SHA256 = "d7274e4c151af7c80d0f6e237d369f79dc3c9f0a65c0efc85ebabf973ed924c1"
DATABASE = "/srv/dasobjectstore/ssd/.dasobjectstore/live.sqlite"
RELATIVE = "objects/d3/alleleanchor_mvp%2Fderived%2Fallele%2Fhg003%2Freference-transform%2F20d72f7f7230795a%2Foutputs%2Ftarget.full.bam"
SOURCE_PARENT = "/srv/dasobjectstore/hdd/qnap-1061/" + RELATIVE
DESTINATION_PARENT = "/srv/dasobjectstore/hdd/qnap-1058/" + RELATIVE
JOURNAL_PARENT = "/var/lib/dasobjectstore/disk_housekeeping"
BACKUP_PARENT = "/var/lib/int07-das-repair-preservation-20261113-v1"
MOUNTS = {
    "/": "1840c6d9-5d11-4368-b56a-55189bba4be3",
    "/srv/dasobjectstore/ssd": "a7e205e0-6b91-4ae2-948e-6b39cb1a778d",
    "/srv/dasobjectstore/hdd/qnap-1061": "fa144a19-3f1e-4b9a-89da-802aee27b322",
    "/srv/dasobjectstore/hdd/qnap-1058": "d02c099d-9d51-4d3e-abc6-b92a0b0fcdf7",
}
DAEMON_PID = 2042692
DAEMON_INVOCATION = "488e898ccab04596aeafea982ef3acfb"
DAEMON_CGROUP = "/dasobjectstore.slice/dasobjectstore-storage.slice/dasobjectstored.service"

PROCESSES = {
    "dasobjectstored.service": (DAEMON_PID, DAEMON_INVOCATION,
        "8f5f01bf4a757e2bf7845bb6887f8afa04470489e2d3f22870a9301573c8dc72"),
    "mnemosyne-expedition-basecamp-vault-migration-verifier.service":
        (2973, "6ce7b0630e6347f18af2294379865b92",
         "b02eeb524960b4d3fd8de709a6f9b2a644b481c3a2dd6e5a910744249e243fc4"),
}


def open_absolute(path, *, directory=False):
    """Walk each component through held descriptors, refusing symbolic links."""
    parts = Path(path).parts
    if not parts or parts[0] != "/" or ".." in parts:
        raise ValueError("absolute canonical path required")
    fd = os.open("/", os.O_RDONLY | os.O_DIRECTORY | os.O_CLOEXEC)
    try:
        for index, part in enumerate(parts[1:]):
            flags = os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC | os.O_NONBLOCK
            if directory or index < len(parts) - 2:
                flags |= os.O_DIRECTORY
            child = os.open(part, flags, dir_fd=fd)
            os.close(fd)
            fd = child
        result = fd
        fd = None
        return result
    finally:
        if fd is not None:
            os.close(fd)


def schema_digest(connection):
    rows = connection.execute(
        "SELECT type,name,tbl_name,sql FROM sqlite_master "
        "WHERE name NOT LIKE 'sqlite_%' ORDER BY type,name").fetchall()
    return hashlib.sha256(json.dumps(rows, separators=(",", ":"),
                                     ensure_ascii=True).encode()).hexdigest()


def verify_daemon_cgroup():
    """Bind the fixed observed manager cgroup and the same process membership."""
    raw=subprocess.check_output(['/usr/bin/systemctl','show','--property=ControlGroup',
                                 '--','dasobjectstored.service'],text=True,timeout=10).splitlines()
    if raw != ['ControlGroup='+DAEMON_CGROUP]:
        raise ValueError('daemon manager cgroup differs')
    if Path(f'/proc/{DAEMON_PID}/cgroup').read_text().splitlines() != ['0::'+DAEMON_CGROUP]:
        raise ValueError('daemon process cgroup differs')


def require_stopped_cgroup_empty(cgroup):
    """Check the held unit's actual cgroup, including descendants."""
    cgroup=Path(cgroup); parent=cgroup.parent
    if not parent.is_dir() or parent.is_symlink() or cgroup.is_symlink():
        raise RuntimeError('daemon cgroup ancestry differs')
    if not cgroup.exists():
        return  # systemd may remove the empty stopped unit cgroup
    def failed(error):
        raise RuntimeError('daemon cgroup walk failed') from error
    for directory, _, files in os.walk(cgroup,onerror=failed,followlinks=False):
        if 'cgroup.procs' not in files or Path(directory,'cgroup.procs').read_text().strip():
            raise RuntimeError('daemon cgroup is not empty')


def verify_read_only():
    """Reject a different machine, mount, schema, daemon or custody verifier."""
    with ExitStack() as stack:
        machine = open_absolute("/etc/machine-id")
        stack.callback(os.close, machine)
        if hashlib.sha256(os.read(machine, 4096)).hexdigest() != MACHINE_SHA256:
            raise ValueError("NUC machine differs")
        for target, uuid in MOUNTS.items():
            raw = subprocess.check_output(
                ["/usr/bin/findmnt", "--json", "--target", target,
                 "--output", "TARGET,UUID,FSTYPE"], timeout=10)
            rows = json.loads(raw)["filesystems"]
            if len(rows) != 1 or rows[0] != {
                    "target": target, "uuid": uuid, "fstype": "ext4"}:
                raise ValueError("NUC filesystem differs")
        for unit, (pid, invocation, digest) in PROCESSES.items():
            raw = subprocess.check_output(
                ["/usr/bin/systemctl", "show", "--property=MainPID",
                 "--property=InvocationID", "--property=ActiveState", "--", unit],
                text=True, timeout=10)
            fields = dict(line.split("=", 1) for line in raw.splitlines())
            if fields != {"MainPID": str(pid), "InvocationID": invocation,
                          "ActiveState": "active"}:
                raise ValueError("NUC owner process differs")
            with open(f"/proc/{pid}/exe", "rb") as stream:
                if hashlib.file_digest(stream, "sha256").hexdigest() != digest:
                    raise ValueError("NUC executable differs")
        verify_daemon_cgroup()
        dbfd = open_absolute(DATABASE)
        stack.callback(os.close, dbfd)
        before = os.fstat(dbfd)
        if not stat.S_ISREG(before.st_mode) or before.st_nlink != 1:
            raise ValueError("catalogue file differs")
        connection = sqlite3.connect(f"file:{DATABASE}?mode=ro", uri=True)
        stack.callback(connection.close)
        connection.execute("BEGIN")
        if schema_digest(connection) != SCHEMA_SHA256:
            raise ValueError("catalogue schema differs")
        current = os.stat(DATABASE, follow_symlinks=False)
        if (before.st_dev, before.st_ino) != (current.st_dev, current.st_ino):
            raise ValueError("catalogue path changed")
        return {"machine": MACHINE_SHA256, "schema": SCHEMA_SHA256,
                "catalogue_identity": [before.st_dev, before.st_ino]}

class HeldPath:
    """Hold every named component and reject changed directory or mount identity."""
    def __init__(self, path, directory=False):
        parts = Path(path).parts
        if not parts or parts[0] != '/' or '..' in parts:
            raise ValueError('absolute canonical path required')
        self.fds = []
        self.names = parts[1:]
        try:
            self.fds.append(os.open('/', os.O_RDONLY | os.O_DIRECTORY | os.O_CLOEXEC))
            for index, part in enumerate(self.names):
                flags = os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC | os.O_NONBLOCK
                if directory or index < len(self.names) - 1:
                    flags |= os.O_DIRECTORY
                self.fds.append(os.open(part, flags, dir_fd=self.fds[-1]))
            self.identities = [self._identity(fd) for fd in self.fds]
            self.recheck()
        except BaseException:
            self.close()
            raise

    @staticmethod
    def _identity(fd):
        info = os.fstat(fd)
        with open(f'/proc/self/fdinfo/{fd}', encoding='ascii') as stream:
            matches = [line.split(':', 1)[1].strip() for line in stream if line.startswith('mnt_id:')]
        if len(matches) != 1:
            raise RuntimeError('mount identity unavailable')
        return (info.st_dev, info.st_ino, stat.S_IFMT(info.st_mode), int(matches[0]))

    def recheck(self):
        for index, fd in enumerate(self.fds):
            if self._identity(fd) != self.identities[index]:
                raise RuntimeError('held path identity changed')
            if index:
                current = os.stat(self.names[index - 1], dir_fd=self.fds[index - 1], follow_symlinks=False)
                if (current.st_dev, current.st_ino, stat.S_IFMT(current.st_mode)) != self.identities[index][:3]:
                    raise RuntimeError('named path identity changed')
        # A new mount can cover the held inode while its underlying name remains.
        reopened = open_absolute('/' + '/'.join(self.names), directory=stat.S_ISDIR(os.fstat(self.fds[-1]).st_mode))
        try:
            if self._identity(reopened) != self.identities[-1]:
                raise RuntimeError('named mount changed')
        finally:
            os.close(reopened)

    def close(self):
        while self.fds:
            os.close(self.fds.pop())


class HeldTarget:
    """Held NUC identities and the one SQLite connection used by recovery.

    This does not establish a writer fence. The caller must hold reviewed owner
    exclusion and repeat recheck around each effect; errors must preserve state.
    """
    def __init__(self, *, stopped=False):
        self.stack = ExitStack()
        self.connection = None
        self.daemon_stopped = stopped
        try:
            if stopped:
                machine=open_absolute('/etc/machine-id')
                try:
                    if hashlib.sha256(os.read(machine,4096)).hexdigest()!=MACHINE_SHA256:
                        raise RuntimeError('NUC machine differs')
                finally: os.close(machine)
                for mount,uuid in MOUNTS.items():
                    data=json.loads(subprocess.check_output(
                        ['/usr/bin/findmnt','--json','--target',mount,'--output','TARGET,UUID,FSTYPE'],timeout=10))['filesystems']
                    if data != [{'target':mount,'uuid':uuid,'fstype':'ext4'}]:
                        raise RuntimeError('NUC filesystem differs')
            else:
                verify_read_only()
            self.paths = {}
            for path in (*MOUNTS, str(Path(DATABASE).parent), DATABASE, SOURCE_PARENT,
                         DESTINATION_PARENT, JOURNAL_PARENT, BACKUP_PARENT):
                if path not in self.paths:
                    obj = HeldPath(path, directory=path != DATABASE)
                    self.stack.callback(obj.close)
                    self.paths[path] = obj
            db = self.paths[DATABASE].fds[-1]
            info = os.fstat(db)
            if not stat.S_ISREG(info.st_mode) or info.st_nlink != 1 or (info.st_dev, info.st_ino) != (2097, 59768864):
                raise RuntimeError('catalogue identity differs')
            self.processes = {}
            for unit, (pid, invocation, digest) in PROCESSES.items():
                if stopped and unit=='dasobjectstored.service':
                    self.processes[unit]=(None,None,None,None)
                    continue
                pidfd = os.pidfd_open(pid)
                self.stack.callback(os.close, pidfd)
                exe = os.open(f'/proc/{pid}/exe', os.O_RDONLY | os.O_CLOEXEC)
                self.stack.callback(os.close, exe)
                with os.fdopen(os.dup(exe), 'rb') as stream:
                    if hashlib.file_digest(stream, 'sha256').hexdigest() != digest:
                        raise RuntimeError('owner executable digest differs')
                self.processes[unit] = (pidfd, exe, self._starttime(pid), os.fstat(exe))
            parent = self.paths[str(Path(DATABASE).parent)].fds[-1]
            before = self._db_fds(info)
            self.connection = sqlite3.connect(f'file:/proc/self/fd/{parent}/live.sqlite?mode=rw', uri=True, isolation_level=None)
            self.stack.callback(self.connection.close)
            opened = self._db_fds(info) - before
            if len(opened) != 1:
                raise RuntimeError('SQLite database descriptor is ambiguous')
            self.sqlite_fd = opened.pop()
            self.recheck()
            if schema_digest(self.connection) != SCHEMA_SHA256:
                raise RuntimeError('catalogue schema differs')
        except BaseException:
            self.close()
            raise

    @staticmethod
    def _db_fds(info):
        found = set()
        for name in os.listdir('/proc/self/fd'):
            try:
                fd = int(name)
                current = os.fstat(fd)
                if (current.st_dev, current.st_ino) == (info.st_dev, info.st_ino):
                    found.add(fd)
            except (ValueError, OSError):
                continue
        return found

    @staticmethod
    def _starttime(pid):
        raw = Path(f'/proc/{pid}/stat').read_text()
        return raw[raw.rfind(')') + 2:].split()[19]

    def recheck(self):
        import select
        for path in self.paths.values():
            path.recheck()
        if not self.daemon_stopped:
            daemon_status = Path(f'/proc/{DAEMON_PID}/status').read_text()
            daemon_fields = dict(line.split(':', 1) for line in daemon_status.splitlines() if ':' in line)
            if daemon_fields.get('Uid', '').split() != ['994'] * 4 or daemon_fields.get('Gid', '').split() != ['972'] * 4 or int(daemon_fields.get('CapEff', '-1').strip(), 16) != 0:
                raise RuntimeError('daemon credentials changed')
            verify_daemon_cgroup()
        for unit, (pid, invocation, digest) in PROCESSES.items():
            pidfd, exe, start, original = self.processes[unit]
            if unit == 'dasobjectstored.service' and self.daemon_stopped:
                if pidfd is None and os.path.exists(f'/proc/{pid}'):
                    raise RuntimeError('original daemon PID remains present')
                if pidfd is not None and not select.select([pidfd], [], [], 0)[0]:
                    raise RuntimeError('original daemon still alive')
                fields = dict(line.split('=', 1) for line in subprocess.check_output(
                    ['/usr/bin/systemctl','show','--property=MainPID','--property=ActiveState',
                     '--property=SubState','--',unit], text=True, timeout=10).splitlines())
                if fields != {'MainPID':'0','ActiveState':'inactive','SubState':'dead'}:
                    raise RuntimeError('daemon stopped state changed')
                require_stopped_cgroup_empty(Path('/sys/fs/cgroup' + DAEMON_CGROUP))
                continue
            if select.select([pidfd], [], [], 0)[0] or self._starttime(pid) != start:
                raise RuntimeError('owner process exited or changed')
            current = os.stat(f'/proc/{pid}/exe')
            if (current.st_dev, current.st_ino) != (original.st_dev, original.st_ino):
                raise RuntimeError('owner executable changed')
            fields = dict(line.split('=', 1) for line in subprocess.check_output(
                ['/usr/bin/systemctl','show','--property=MainPID','--property=InvocationID',
                 '--property=ActiveState','--',unit], text=True, timeout=10).splitlines())
            if fields != {'MainPID':str(pid),'InvocationID':invocation,'ActiveState':'active'}:
                raise RuntimeError('owner service changed')
        if self.connection is not None:
            db = os.fstat(self.paths[DATABASE].fds[-1])
            actual = os.fstat(self.sqlite_fd)
            if (actual.st_dev, actual.st_ino) != (db.st_dev, db.st_ino):
                raise RuntimeError('SQLite database descriptor changed')
            if schema_digest(self.connection) != SCHEMA_SHA256:
                raise RuntimeError('catalogue schema changed')

    def observe_stopped(self):
        """Switch only after a separately fenced normal stop; never stop a unit."""
        self.daemon_stopped = True
        try:
            self.recheck()
        except BaseException:
            self.daemon_stopped = False
            raise

    def recheck_verifier(self):
        """Check the retained running image even after its pathname is replaced."""
        import select
        unit='mnemosyne-expedition-basecamp-vault-migration-verifier.service'
        pid, invocation, _ = PROCESSES[unit]
        pidfd, exe, start, original = self.processes[unit]
        if select.select([pidfd], [], [], 0)[0] or self._starttime(pid) != start:
            raise RuntimeError('attended verifier exited or changed')
        current=os.stat(f'/proc/{pid}/exe')
        if (current.st_dev,current.st_ino)!=(original.st_dev,original.st_ino):
            raise RuntimeError('attended verifier executable changed')
        fields=dict(line.split('=',1) for line in subprocess.check_output(
            ['/usr/bin/systemctl','show','--property=MainPID','--property=InvocationID',
             '--property=ActiveState','--',unit],text=True,timeout=10).splitlines())
        if fields!={'MainPID':str(pid),'InvocationID':invocation,'ActiveState':'active'}:
            raise RuntimeError('attended verifier service changed')

    def close(self):
        self.stack.close()
        self.connection = None

class RecoveryStopProof:
    """Concrete committed-recovery evidence; it cannot stop or start services."""
    def __init__(self, target, marker, backups, state_fd, source_parent_fd,
                 destination_parent_fd, quarantine_fd):
        from original_journal_marker import OriginalJournalMarker
        from recovered_journal_marker import RecoveredJournalMarker
        from integrated_claim_recovery import VerifiedBackups
        if type(target) is not HeldTarget or type(marker) not in (OriginalJournalMarker, RecoveredJournalMarker) or type(backups) is not VerifiedBackups:
            raise RuntimeError('exact live recovery custody required')
        self.target=target; self.marker=marker; self.backups=backups
        self.state_fd=state_fd; self.source_parent_fd=source_parent_fd
        self.destination_parent_fd=destination_parent_fd; self.quarantine_fd=quarantine_fd
        self.recheck()

    def recheck(self):
        from integrated_claim_recovery import _load_recovery_record, _flags, _IMMUTABLE
        from claimed_recovery_transaction import validate_exact_claim
        self.target.recheck(); self.marker.recheck(); self.backups.recheck()

        record=_load_recovery_record(self.state_fd)
        if record['phase'] != 'committed': raise RuntimeError('claim has not committed')
        for name, parent_fd, filename, immutable in (
            ('source', self.source_parent_fd, 'payload', True),
            ('partial', self.quarantine_fd, 'partial', False)):
            parent=os.fstat(parent_fd); saved=record[name]['file']; row=self.backups.rows[name]
            if name=='source' and (parent.st_dev,parent.st_ino,parent.st_uid,parent.st_gid,stat.S_IMODE(parent.st_mode)) != (record[name]['parent']['device'],record[name]['parent']['inode'],0,0,0o500):
                raise RuntimeError('source parent is not protected')
            fd=os.open(filename,os.O_RDONLY|os.O_NOFOLLOW|os.O_CLOEXEC,dir_fd=parent_fd)
            try:
                f=os.fstat(fd); named=os.stat(filename,dir_fd=parent_fd,follow_symlinks=False)
                if (not stat.S_ISREG(f.st_mode) or f.st_nlink != 1 or (f.st_dev,f.st_ino,f.st_size) != (row['device'],row['inode'],row['size'])
                    or f.st_mtime_ns != saved['mtime_ns'] or saved['sha256'] != row['sha256']
                    or (named.st_dev,named.st_ino)!=(f.st_dev,f.st_ino) or (f.st_uid,f.st_gid,stat.S_IMODE(f.st_mode)) != (0,0,0o640)
                    or _flags(fd)!=(saved['flags']|(_IMMUTABLE if immutable else 0))):
                    raise RuntimeError('protected payload changed')
            finally: os.close(fd)
        q=os.fstat(self.quarantine_fd); dest=os.fstat(self.destination_parent_fd); original=record['partial']['parent']
        if ((q.st_dev,q.st_ino)!=tuple(record['quarantine']) or (q.st_uid,q.st_gid,stat.S_IMODE(q.st_mode))!=(0,0,0o700)
            or (dest.st_dev,dest.st_ino,dest.st_uid,dest.st_gid,stat.S_IMODE(dest.st_mode))!=(original['device'],original['inode'],0,0,0o500)):
            raise RuntimeError('destination custody differs')
        try: os.stat('payload',dir_fd=self.destination_parent_fd,follow_symlinks=False)
        except FileNotFoundError: pass
        else: raise RuntimeError('destination was recreated')
        conn=self.target.connection
        if conn.in_transaction: raise RuntimeError('unexpected open transaction')
        conn.execute('BEGIN IMMEDIATE')
        try:
            if validate_exact_claim(conn,record['intent'],journal_present=True)!='released':
                raise RuntimeError('exact claim not released')
            conn.rollback()
        except BaseException:
            if conn.in_transaction:conn.rollback()
            raise
        self.target.recheck(); self.marker.recheck(); self.backups.recheck()

    def check_quiet(self):
        """Fresh owner status and queue snapshot before a normal service stop."""
        import socket
        import struct
        self.recheck()
        if self.target.daemon_stopped:
            raise RuntimeError('quiet check requires running daemon')
        with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as client:
            client.settimeout(5)
            client.connect('/run/dasobjectstore/dasobjectstored.sock')
            # A complete envelope must precede any denial: the installed
            # daemon exits if a peer closes before receiving its newline.
            client.sendall(b'{"command":"live_status","payload":null}\n')
            if struct.unpack('3i',client.getsockopt(socket.SOL_SOCKET,socket.SO_PEERCRED,12)) != (DAEMON_PID,994,972):
                raise RuntimeError('daemon socket peer changed')
            with client.makefile('rb') as response:
                raw=response.readline(1024*1024+1)
            if len(raw)>1024*1024 or not raw.endswith(b'\n'):
                raise RuntimeError('daemon status response differs')
        result=json.loads(raw)
        payload=result.get('payload',{})
        aggregate=payload.get('aggregate',{})
        keys=('connected_hosts','active_stores','active_ingests','active_hdd_transfers',
              'source_read_bytes_per_second','ssd_write_bytes_per_second','hdd_write_bytes_per_second')
        if (result.get('kind')!='live_status' or payload.get('schema_version')!=1
            or any(type(aggregate.get(key)) is not int or aggregate[key]!=0 for key in keys)
            or payload.get('active')!=[] or payload.get('garbage_collection',{}).get('running') is not False):
            raise RuntimeError('daemon is not quiet')
        conn=self.target.connection
        if conn.in_transaction: raise RuntimeError('unexpected open transaction')
        conn.execute('BEGIN')
        try:
            for table in ('compute_workspace_operations','compute_workspace_materializations','compute_workspace_promotions'):
                if conn.execute('SELECT COUNT(*) FROM '+table).fetchone()[0] != 0:
                    raise RuntimeError('workspace activity exists')
            if conn.execute("SELECT COUNT(*) FROM destage_queue WHERE state!='hdd_copy_verified' OR state IS NULL").fetchone()[0]:
                raise RuntimeError('destage work is not settled')
            if conn.execute("SELECT COUNT(*) FROM scheduler_jobs WHERE state!='completed' OR state IS NULL").fetchone()[0]:
                raise RuntimeError('scheduler work is not settled')
            if conn.execute("SELECT COUNT(*) FROM ingest_jobs WHERE state!='ssd_accepted' OR state IS NULL").fetchone()[0]:
                raise RuntimeError('ingest state differs')
            conn.rollback()
        except BaseException:
            if conn.in_transaction: conn.rollback()
            raise
        self.recheck()
