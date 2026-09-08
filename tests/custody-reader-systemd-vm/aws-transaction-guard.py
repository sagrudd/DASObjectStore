#!/usr/bin/python3
"""Strict fixture-only DNF5 stored transaction guard; never a dependency solver.

Format source: dnf5 5.4.2.0 libdnf5/transaction/transaction_sr.cpp.
The actual public QEMU plan confirms version1.0/package_path/repo_id forms.
Unknown actions/fields or existing package names deny, not get interpreted.
"""
import hashlib
import json
import pathlib
import re
import subprocess
import sys

FIELDS = {'nevra', 'action', 'reason', 'repo_id', 'package_path'}


def closed_pairs(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError('duplicate JSON field')
        result[key] = value
    return result


def package_identity(path):
    value = subprocess.run(['/usr/bin/rpm', '-qp', '--qf',
        '%{NAME}\t%{EPOCHNUM}\t%{VERSION}\t%{RELEASE}\t%{ARCH}\n', str(path)],
        check=True, capture_output=True, text=True, timeout=15).stdout.strip()
    parts = value.split('\t')
    if len(parts) != 5:
        raise ValueError('unexpected RPM identity')
    name, epoch, version, release, arch = parts
    evr = (epoch + ':' if epoch != '0' else '') + version + '-' + release
    return name + '-' + evr + '.' + arch, value, name


def validate(plan, installed_names, packages):
    if set(plan) != {'version', 'rpms'} or plan['version'] != '1.0':
        raise ValueError('unselected stored transaction format')
    if not isinstance(plan['rpms'], list) or not 1 <= len(plan['rpms']) <= 71:
        raise ValueError('invalid transaction cardinality')
    selected = []
    names = set()
    for item in plan['rpms']:
        if not isinstance(item, dict) or set(item) != FIELDS:
            raise ValueError('unselected package fields')
        if item['action'] != 'Install' or item['reason'] not in {'User', 'Dependency'}:
            raise ValueError('only new installs admitted')
        if item['repo_id'] != '@stored_transaction(das-vm-aws)':
            raise ValueError('foreign repository')
        path = item['package_path']
        if not isinstance(path, str) or not re.fullmatch(r'\./packages/[A-Za-z0-9_.+%-]+\.rpm', path):
            raise ValueError('unexpected payload path')
        filename = path.removeprefix('./packages/')
        if filename not in packages:
            raise ValueError('payload not in signed closure')
        nevra, row, name = packages[filename]
        if item['nevra'] != nevra or name in installed_names or name in names:
            raise ValueError('identity mismatch or existing package change')
        names.add(name)
        selected.append((path, row))
    if 'awscli2' not in names:
        raise ValueError('AWS package is not selected')
    return selected


def main():
    if len(sys.argv) != 5:
        raise ValueError('expected plan-dir closure-dir before-file output-dir')
    plan_dir, closure, before, output = map(pathlib.Path, sys.argv[1:])
    # The caller has already checked the fixed complete manifest SHA and every
    # signature against the exact approved Fedora44 key in an isolated key DB.
    hashes = {}
    for line in (closure / 'evidence/sha256.txt').read_text().splitlines():
        digest, raw_path = line.split('  ')
        path = pathlib.Path(raw_path)
        if path.parent != pathlib.Path('/opt/das-aws-fedora44/rpms'):
            raise ValueError('foreign closure path')
        if not re.fullmatch('[0-9a-f]{64}', digest) or path.name in hashes:
            raise ValueError('duplicate or malformed closure digest')
        hashes[path.name] = digest
    if len(hashes) != 71:
        raise ValueError('closure is not the reviewed 71 packages')
    packages = {}
    for filename, digest in hashes.items():
        path = closure / 'rpms' / filename
        if path.is_symlink() or not path.is_file() or hashlib.sha256(path.read_bytes()).hexdigest() != digest:
            raise ValueError('payload bytes changed')
        packages[filename] = package_identity(path)
    before_lines = set(before.read_text().splitlines())
    names = {line.split('\t')[0] for line in before_lines}
    plan = json.loads((plan_dir / 'transaction.json').read_text(), object_pairs_hook=closed_pairs)
    selected = validate(plan, names, packages)
    for relative, _ in selected:
        path = plan_dir / relative
        if path.is_symlink() or not path.is_file() or hashlib.sha256(path.read_bytes()).hexdigest() != hashes[path.name]:
            raise ValueError('stored payload bytes changed')
    expected = before_lines | {row for _, row in selected}
    (output / 'expected-installed.tsv').write_text(''.join(row + '\n' for row in sorted(expected)))
    (output / 'selected-rpms.txt').write_text(''.join(str(plan_dir / path) + '\n' for path, _ in selected))


if __name__ == '__main__':
    main()
