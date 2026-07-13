#!/usr/bin/env python3
"""Export one managed Garage credential as an AlleleAnchor config pair.

The managed credential registry is private JSON owned by DASObjectStore. This
helper writes a separate mode-0600 secret file and a mode-0600 adapter config;
it never prints credential values.
"""

from __future__ import annotations

import argparse
import json
import os
import stat
from pathlib import Path


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--registry", type=Path, required=True)
    parser.add_argument("--store-id", required=True)
    parser.add_argument("--endpoint", required=True)
    parser.add_argument("--prefix", default="")
    parser.add_argument("--output", type=Path, required=True)
    return parser.parse_args()


def write_private(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    flags = os.O_WRONLY | os.O_CREAT | os.O_TRUNC
    fd = os.open(path, flags, 0o600)
    try:
        os.fchmod(fd, stat.S_IRUSR | stat.S_IWUSR)
        with os.fdopen(fd, "w", encoding="utf-8") as stream:
            stream.write(text)
        fd = -1
    finally:
        if fd != -1:
            os.close(fd)


def main() -> int:
    args = parse_args()
    registry = json.loads(args.registry.read_text(encoding="utf-8"))
    records = [
        record
        for record in registry.get("credentials", [])
        if record.get("store_id") == args.store_id
    ]
    if len(records) != 1:
        raise SystemExit(
            f"expected exactly one credential record for {args.store_id}, found {len(records)}"
        )

    record = records[0]
    bucket = record.get("bucket_name", "")
    access_key = record.get("access_key_id", "")
    secret_key = record.get("secret_access_key", "")
    if not bucket or not access_key or not secret_key:
        raise SystemExit("credential record is incomplete")

    secret_path = args.output.with_name(f"{args.output.stem}.credentials.toml")
    config_path = args.output
    write_private(
        secret_path,
        f'access_key = "{access_key}"\nsecret_key = "{secret_key}"\n',
    )
    prefix_line = f'prefix = "{args.prefix}"\n' if args.prefix else ""
    write_private(
        config_path,
        "\n".join(
            [
                f'endpoint = "{args.endpoint}"',
                'region = "garage"',
                f'bucket = "{bucket}"',
                prefix_line.rstrip("\n"),
                "[credential_source]",
                'kind = "file"',
                f'path = "{secret_path}"',
                "",
            ]
        ),
    )
    print(f"AlleleAnchor config: {config_path}")
    print(f"Credential file: {secret_path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
