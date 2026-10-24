#!/usr/bin/env python3
"""Check one exact, expiring inherited module-size record; never enlarge a budget."""
import datetime
import hashlib
import json
import re
import sys
from pathlib import Path


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError('duplicate baseline field')
        result[key] = value
    return result


def accepted(path, lines, document, today):
    if (not isinstance(document, dict)
            or type(document.get('schema_version')) is not int
            or document['schema_version'] != 1):
        return False
    records = document['records']
    if not isinstance(records, list) or not all(isinstance(row, dict) for row in records):
        return False
    paths = [row['path'] for row in records]
    if len(paths) != len(set(paths)):
        return False
    for row in records:
        candidate = Path(row['path'])
        if (candidate.is_absolute() or '..' in candidate.parts
                or not str(candidate).startswith('crates/')
                or not re.fullmatch(r'[0-9a-f]{64}', row['sha256'])
                or type(row['max_production_lines']) is not int
                or row['max_production_lines'] <= 1000
                or not row['owner'] or not row['remediation']):
            return False
        expiry = datetime.date.fromisoformat(row['expires_before_utc'])
        if today >= expiry:
            return False
    matches = [row for row in records if row['path'] == path]
    if len(matches) != 1:
        return False
    row = matches[0]
    return (1000 < lines <= row['max_production_lines']
            and hashlib.sha256(Path(path).read_bytes()).hexdigest() == row['sha256'])


def main():
    try:
        path, raw_lines = sys.argv[1:]
        document = json.loads(Path('tools/rust-module-size-baselines.json').read_text(), object_pairs_hook=unique_object)
        if not accepted(path, int(raw_lines), document, datetime.datetime.now(datetime.timezone.utc).date()):
            return 1
        expiry = next(row['expires_before_utc'] for row in document['records'] if row['path'] == path)
        print(f'reviewed exact temporary baseline: {path} ({raw_lines} production lines; expires before {expiry} UTC)')
        return 0
    except (OSError, ValueError, KeyError, TypeError):
        return 1


if __name__ == '__main__':
    sys.exit(main())
