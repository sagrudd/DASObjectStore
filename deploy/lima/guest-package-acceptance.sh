#!/usr/bin/env bash
set -euo pipefail

root=/var/tmp/dasobjectstore-acceptance
workspace=/opt/dasobjectstore-acceptance/workspace
web_dist=/opt/dasobjectstore-acceptance/web-dist
state_file=/var/tmp/dasobjectstore-package-acceptance.env
evidence="$root/evidence.txt"
phase="${1:?phase is required}"
distro="${2:?distribution is required}"

install_build_dependencies() {
  case "$distro" in
    ubuntu)
      export DEBIAN_FRONTEND=noninteractive
      apt-get update
      apt-get install -y build-essential clang libclang-dev libpam0g-dev \
        pkg-config libssl-dev curl ca-certificates acl udisks2 docker.io \
        docker-buildx openssl dpkg-dev unzip
      ;;
    alma)
      dnf install -y epel-release dnf-plugins-core
      dnf config-manager --add-repo \
        https://download.docker.com/linux/centos/docker-ce.repo
      dnf groupinstall -y "Development Tools"
      dnf install -y clang clang-devel pam-devel pkgconf-pkg-config \
        openssl-devel curl ca-certificates acl udisks2 rpm-build openssl \
        docker-ce-cli docker-buildx-plugin unzip
      ;;
    *)
      printf 'unsupported distribution: %s\n' "$distro" >&2
      exit 2
      ;;
  esac
}

install_aws_cli() {
  export PATH="/usr/local/bin:$PATH"
  if command -v aws >/dev/null 2>&1; then
    return 0
  fi
  curl --proto '=https' --tlsv1.2 -fsSL \
    https://awscli.amazonaws.com/awscli-exe-linux-aarch64.zip \
    -o /tmp/awscliv2.zip
  rm -rf /tmp/aws
  unzip -q /tmp/awscliv2.zip -d /tmp
  /tmp/aws/install
  aws --version
}

install_rust() {
  if [[ ! -x /root/.cargo/bin/cargo ]]; then
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
      | sh -s -- -y --profile minimal --default-toolchain 1.88.0
  fi
  export PATH="/root/.cargo/bin:$PATH"
}

prepare_workspace() {
  rm -rf /opt/dasobjectstore-acceptance
  install -d "$workspace/DASObjectStore" "$workspace/prosopikon" "$web_dist"
  tar -C "$workspace/DASObjectStore" -xzf "$root/DASObjectStore.tar.gz"
  tar -C "$workspace/prosopikon" -xzf "$root/prosopikon.tar.gz"
  tar -C "$web_dist" -xzf "$root/web-dist.tar.gz"
}

build_package() {
  export PATH="/root/.cargo/bin:$PATH"
  export CARGO_TARGET_DIR="$workspace/DASObjectStore/target"
  export DASOBJECTSTORE_PREBUILT_WEB_DIST="$web_dist"
  case "$distro" in
    ubuntu)
      package="$(bash "$workspace/DASObjectStore/packaging/debian/build-deb.sh" | tail -1)"
      ;;
    alma)
      package="$(bash "$workspace/DASObjectStore/packaging/rpm/build-rpm.sh" | tail -1)"
      ;;
  esac
  [[ -f "$package" ]]
  printf 'PACKAGE=%q\n' "$package" >"$state_file"
}

install_package() {
  source "$state_file"
  case "$distro" in
    ubuntu) apt-get install -y "$PACKAGE" ;;
    alma) dnf install -y "$PACKAGE" ;;
  esac
}

install_local_test_certificate() {
  install -d -o root -g dasobjectstore -m 0750 /opt/dasobjectstore/tls
  openssl req -x509 -newkey rsa:2048 -nodes -days 2 \
    -subj /CN=localhost \
    -keyout /opt/dasobjectstore/tls/server.key \
    -out /opt/dasobjectstore/tls/server.crt >/dev/null 2>&1
  chown root:dasobjectstore /opt/dasobjectstore/tls/server.key \
    /opt/dasobjectstore/tls/server.crt
  chmod 0640 /opt/dasobjectstore/tls/server.key \
    /opt/dasobjectstore/tls/server.crt
  systemctl restart dasobjectstored.service dasobjectstore-server.service
}

assert_services_and_resources() {
  systemctl is-active --quiet dasobjectstored.service
  systemctl is-active --quiet dasobjectstore-server.service
  [[ "$(systemctl show dasobjectstore-server.service -p Slice --value)" \
    == dasobjectstore-control.slice ]]
  [[ "$(systemctl show dasobjectstored.service -p Slice --value)" \
    == dasobjectstore-storage.slice ]]
  [[ "$(systemctl show dasobjectstore-control.slice -p CPUWeight --value)" == 200 ]]
  [[ "$(systemctl show dasobjectstore-control.slice -p IOWeight --value)" == 200 ]]
  [[ "$(systemctl show dasobjectstore-control.slice -p MemoryLow --value)" != 0 ]]
  [[ "$(systemctl show dasobjectstore-storage.slice -p CPUWeight --value)" == 80 ]]
  [[ "$(systemctl show dasobjectstore-storage.slice -p IOWeight --value)" == 80 ]]
  [[ "$(systemctl show dasobjectstore-storage.slice -p MemoryHigh --value)" != infinity ]]
}

reinstall_package() {
  source "$state_file"
  case "$distro" in
    ubuntu) dpkg -i "$PACKAGE" ;;
    alma) rpm -Uvh --replacepkgs "$PACKAGE" ;;
  esac
}

remove_package() {
  case "$distro" in
    ubuntu) apt-get remove -y dasobjectstore ;;
    alma) dnf remove -y dasobjectstore ;;
  esac
}

case "$phase" in
  initial)
    install_build_dependencies
    install_aws_cli
    install_rust
    prepare_workspace
    build_package
    install_package
    install_local_test_certificate
    assert_services_and_resources
    install -d -o dasobjectstore -g dasobjectstore -m 0750 \
      /var/lib/dasobjectstore /srv/dasobjectstore
    install -o dasobjectstore -g dasobjectstore -m 0640 /dev/null \
      /var/lib/dasobjectstore/lima-acceptance-sentinel
    install -o dasobjectstore -g dasobjectstore -m 0640 /dev/null \
      /srv/dasobjectstore/lima-acceptance-sentinel
    reinstall_package
    assert_services_and_resources
    printf 'distribution=%s\narchitecture=%s\ninstall=passed\nupgrade=passed\n' \
      "$distro" "$(uname -m)" >"$evidence"
    ;;
  post-reboot)
    source "$state_file"
    assert_services_and_resources
    [[ -f /var/lib/dasobjectstore/lima-acceptance-sentinel ]]
    [[ -f /srv/dasobjectstore/lima-acceptance-sentinel ]]
    remove_package
    ! command -v dasobjectstored >/dev/null 2>&1
    ! systemctl is-active --quiet dasobjectstored.service
    ! systemctl is-active --quiet dasobjectstore-server.service
    [[ -f /var/lib/dasobjectstore/lima-acceptance-sentinel ]]
    [[ -f /srv/dasobjectstore/lima-acceptance-sentinel ]]
    printf 'reboot=passed\nuninstall=passed\npersistent_state_retained=yes\n' >>"$evidence"
    ;;
  *)
    printf 'unsupported phase: %s\n' "$phase" >&2
    exit 2
    ;;
esac
