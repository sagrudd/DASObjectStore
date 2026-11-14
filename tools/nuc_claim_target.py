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
MOUNTS = {
    "/": "1840c6d9-5d11-4368-b56a-55189bba4be3",
    "/srv/dasobjectstore/ssd": "a7e205e0-6b91-4ae2-948e-6b39cb1a778d",
    "/srv/dasobjectstore/hdd/qnap-1061": "fa144a19-3f1e-4b9a-89da-802aee27b322",
    "/srv/dasobjectstore/hdd/qnap-1058": "d02c099d-9d51-4d3e-abc6-b92a0b0fcdf7",
}
PROCESSES = {
    "dasobjectstored.service": (8342, "3bd2db3105e84fbbb5945f0effa1a3c4",
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
