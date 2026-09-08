#!/bin/bash
# Public-only download preparation in the pinned Fedora44/aarch64 container.
# Independent outer controller: 900-second wall clock + exact container cleanup.
# No mounts/caps/host namespaces; the in-container watchdog is defense in depth.
# Never boots a guest or installs any package. The guest separately verifies/install.
set -euo pipefail
export LC_ALL=C
umask 022
trap 'exit 124' TERM
( sleep 900; kill -TERM "$$" ) &
aws_watchdog_pid=$!
trap 'kill "$aws_watchdog_pid" 2>/dev/null || true' EXIT
readonly out=/opt/das-aws-fedora44
readonly fingerprint=36F612DCF27F7D1A48A835E4DBFCF71C6D9F90A6
readonly maximum=1073741824
test ! -e "$out"
test "$(uname -m)" = aarch64
test "$(rpm -E '%fedora')" = 44
for tool in dnf5 rpm rpmkeys gpg curl sha256sum stat awk; do command -v "$tool" >/dev/null; done
mkdir -p "$out/rpms" "$out/evidence" "$out/keydb" "$out/gnupg"
chmod 700 "$out/gnupg"
key=/etc/pki/rpm-gpg/RPM-GPG-KEY-fedora-44-aarch64
test -f "$key"
gpg --batch --homedir "$out/gnupg" --show-keys --with-colons "$key" > "$out/evidence/key.txt"
awk -F: -v expected="$fingerprint" '
 $1=="pub" { pubs++; want=1 }
 $1=="fpr" && want { if ($10!=expected) exit 1; found++; want=0 }
 END { if (pubs!=1 || found!=1) exit 1 }
' "$out/evidence/key.txt"
cp "$key" "$out/fedora44.asc"
# No trust in another RPM key from the container's pre-existing package database.
rpmkeys --define '_keyring rpmdb' --dbpath "$out/keydb" --import "$out/fedora44.asc"
rpmkeys --define '_keyring rpmdb' --dbpath "$out/keydb" --list > "$out/evidence/rpm-keyring.txt"
dnf5 --version > "$out/evidence/dnf-version.txt"
rpm --version > "$out/evidence/rpm-version.txt"
# The official download solver includes dependencies even if already installed.
# No package transaction is performed. Exact resolved URLs are retained, not re-solved later.
dnf5 --quiet --releasever=44 --setopt=retries=0 --setopt=timeout=60 \
 --setopt=install_weak_deps=False --disable-repo='*' --enable-repo=fedora \
 download --resolve --alldeps --arch=aarch64 --arch=noarch --url --urlprotocol=https awscli2 \
 > "$out/evidence/urls.txt"
count=0
total=0
while IFS= read -r url; do
    test -n "$url"
    case "$url" in https://*.rpm) ;; *) exit 1;; esac
    count=$((count+1)); test "$count" -le 1024
    filename=${url##*/}
    case "$filename" in *[!a-zA-Z0-9._+~%-]*|'') exit 1;; esac
    destination="$out/rpms/$filename"
    test ! -e "$destination"
    remaining=$((maximum-total)); test "$remaining" -gt 0
    curl --silent --show-error --fail --location --proto '=https' --proto-redir '=https' --connect-timeout 15 \
      --max-time 60 --retry 0 --max-filesize "$remaining" --output "$destination" "$url"
    size=$(stat -c %s "$destination")
    total=$((total+size)); test "$total" -le "$maximum"
    rpmkeys --define '_keyring rpmdb' --define '_pkgverify_level signature' --dbpath "$out/keydb" --checksig --verbose "$destination" \
       >> "$out/evidence/signatures.txt"
    sha256sum "$destination" >> "$out/evidence/sha256.txt"
    rpm -qp --qf '%{NAME} %{EPOCHNUM}:%{VERSION}-%{RELEASE} %{ARCH}\n' "$destination" \
       >> "$out/evidence/nevra.txt"
done < "$out/evidence/urls.txt"
test "$count" -gt 0
grep -q '^awscli2 ' "$out/evidence/nevra.txt"
printf 'packages=%s\nbytes=%s\n' "$count" "$total" > "$out/evidence/bounds.txt"
# Retain the exact public metadata consulted by the solver where present.
find /var/cache/libdnf5 -type f -print0 | sort -z | xargs -0 -r sha256sum > "$out/evidence/metadata-sha256.txt"
echo PUBLIC_FEDORA_AWS_PREPARATION_COMPLETE
