#!/bin/bash
# Public-only preparation. Invoke under an outer 900-second timeout in a
# mount-free, capability-free disposable ARM64 container. NEVER boots a VM.
set -euo pipefail
export LC_ALL=C DEBIAN_FRONTEND=noninteractive
umask 022
readonly out=/opt/custody-systemd-vm
test ! -e "$out"
for tool in apt-get dpkg dpkg-deb tar python3 gpg gpgv sha256sum timeout readlink; do
    command -v "$tool" >/dev/null
done
test "$(dpkg --print-architecture)" = arm64
test "$(readlink /bin)" = usr/bin
test "$(readlink /sbin)" = usr/sbin
test "$(readlink /lib)" = usr/lib
mkdir -p "$out/evidence" "$out/public" "$out/apt/sources" "$out/apt/lists/partial" "$out/apt/archives/partial"
printf '%s\n' \
 'deb [check-valid-until=no] https://snapshot.debian.org/archive/debian/20260907T000000Z/ bookworm main' \
 'deb [check-valid-until=no] https://snapshot.debian.org/archive/debian-security/20260907T000000Z/ bookworm-security main' \
 > "$out/apt/sources.list"
printf '%s\n' '#clear DPkg::Post-Invoke;' '#clear APT::Update::Post-Invoke;' '#clear APT::Update::Post-Invoke-Success;' > "$out/apt/config"
# The entire preparation container has no capabilities; keep downloads under
# its existing root UID rather than requesting setgroups/setuid capability.
apt_options=(-c "$out/apt/config" -o APT::Sandbox::User=root -o "Dir::Etc::sourcelist=$out/apt/sources.list" -o "Dir::Etc::sourceparts=$out/apt/sources"
 -o "Dir::State::lists=$out/apt/lists" -o "Dir::Cache::archives=$out/apt/archives"
 -o Acquire::Retries=0 -o Acquire::https::Timeout=60)
apt-get "${apt_options[@]}" update
apt-get "${apt_options[@]}" --print-uris --download-only --no-install-recommends --yes install \
 qemu-system-arm qemu-utils qemu-efi-aarch64 genisoimage > "$out/evidence/package-uris.txt"
python3 - "$out" <<'PY'
import pathlib, shlex, sys
p=pathlib.Path(sys.argv[1]); total=sum(f.stat().st_size for f in (p/'apt/lists').rglob('*') if f.is_file())
for line in (p/'evidence/package-uris.txt').read_text().splitlines():
    if line.startswith("'"):
        fields=shlex.split(line); total+=int(fields[2])
assert total < 1024**3, 'tool payload exceeds 1 GiB sub-budget'
PY
apt-get "${apt_options[@]}" --download-only --no-install-recommends --yes install \
 qemu-system-arm qemu-utils qemu-efi-aarch64 genisoimage
find "$out/apt/lists" -maxdepth 1 -type f ! -name lock -print0 | sort -z | xargs -0 sha256sum > "$out/evidence/index-sha256.txt"
for package in "$out"/apt/archives/*.deb; do
    sha256sum "$package" >> "$out/evidence/package-inputs.txt"
    dpkg-deb --field "$package" Package Version Architecture >> "$out/evidence/package-inputs.txt"
    dpkg-deb --fsys-tarfile "$package" | tar --no-same-owner --keep-directory-symlink -xf - -C /
    test "$(readlink /bin)" = usr/bin
    test "$(readlink /sbin)" = usr/sbin
    test "$(readlink /lib)" = usr/lib
done
ldconfig
qemu-system-aarch64 --version > "$out/evidence/qemu-version.txt"
qemu-img --version >> "$out/evidence/qemu-version.txt"
genisoimage --version >> "$out/evidence/qemu-version.txt" 2>&1
sha256sum /usr/bin/qemu-system-aarch64 /usr/bin/qemu-img /usr/bin/genisoimage > "$out/evidence/tool-sha256.txt"
python3 - "$out" <<'PY'
import pathlib, sys, urllib.request
p=pathlib.Path(sys.argv[1]); limit=4*1024**3
total=sum(f.stat().st_size for f in (p/'apt').rglob('*') if f.is_file())
items=[('fedora.gpg','https://fedoraproject.org/fedora.gpg',2*1024**2),
('CHECKSUM','https://dl.fedoraproject.org/pub/fedora/linux/releases/44/Cloud/aarch64/images/Fedora-Cloud-44-1.7-aarch64-CHECKSUM',1024**2)]
for name,url,cap in items:
    size=0
    with urllib.request.urlopen(url,timeout=60) as response, (p/'public'/name).open('xb') as target:
        while chunk:=response.read(1024**2):
            size+=len(chunk); total+=len(chunk)
            if size>cap or total>limit: raise RuntimeError('public download limit')
            target.write(chunk)
PY
mkdir -m 700 "$out/gnupg"
gpg --batch --homedir "$out/gnupg" --import "$out/public/fedora.gpg"
gpg --batch --homedir "$out/gnupg" --export 36F612DCF27F7D1A48A835E4DBFCF71C6D9F90A6 > "$out/public/fedora44.gpg"
test -s "$out/public/fedora44.gpg"
gpgv --status-fd 1 --keyring "$out/public/fedora44.gpg" "$out/public/CHECKSUM" > "$out/evidence/checksum-signature.txt"
python3 - "$out" <<'PY'
import hashlib, pathlib, re, sys, urllib.request
p=pathlib.Path(sys.argv[1]); fingerprint='36F612DCF27F7D1A48A835E4DBFCF71C6D9F90A6'
valid=[l.split() for l in (p/'evidence/checksum-signature.txt').read_text().splitlines() if l.startswith('[GNUPG:] VALIDSIG ')]
assert len(valid)==1 and (valid[0][2]==fingerprint or valid[0][-1]==fingerprint), 'unexpected signing key'
name='Fedora-Cloud-Base-Generic-44-1.7.aarch64.qcow2'
matches=re.findall(r'^SHA256 \('+re.escape(name)+r'\) = ([0-9a-f]{64})$',(p/'public/CHECKSUM').read_text(),re.M)
assert len(matches)==1, 'ambiguous image checksum'
assert matches[0]=='55c60a3b80d3616a08705afd0459e75fe9f03c54aba7a46e4002a41a72fa0d5b'
total=sum(f.stat().st_size for f in p.rglob('*') if f.is_file()); limit=4*1024**3
url='https://download.fedoraproject.org/pub/fedora/linux/releases/44/Cloud/aarch64/images/'+name
h=hashlib.sha256()
with urllib.request.urlopen(url,timeout=60) as response, (p/'public'/name).open('xb') as target:
    while chunk:=response.read(1024**2):
        total+=len(chunk)
        if total>limit: raise RuntimeError('aggregate 4 GiB public download limit')
        h.update(chunk); target.write(chunk)
assert h.hexdigest()==matches[0], 'image checksum mismatch'
(p/'evidence/image-sha256.txt').write_text(h.hexdigest()+'  '+name+'\n')
PY
qemu-img info --output=json "$out/public/Fedora-Cloud-Base-Generic-44-1.7.aarch64.qcow2" > "$out/evidence/image-info.json"
python3 - "$out/evidence/image-info.json" <<'PY'
import json,sys
x=json.load(open(sys.argv[1])); assert x['format']=='qcow2' and 0<x['virtual-size']<=24*1024**3
assert not x.get('backing-filename') and not x.get('full-backing-filename')
PY
find "$out/public" -maxdepth 1 -type f -print0 | sort -z | xargs -0 sha256sum > "$out/evidence/public-sha256.txt"
printf '%s\n' 'PUBLIC_PREPARATION_PASS; VM NOT BOOTED; NO GUEST KEYS GENERATED'
