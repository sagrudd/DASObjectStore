use dasobjectstore_daemon::{
    DaemonRuntimeConfig, DEFAULT_DAEMON_GROUP, DEFAULT_DAEMON_SERVICE_USER,
    LINUX_DAEMON_CONFIG_PATH, LINUX_DAEMON_LOG_DIR, LINUX_DAEMON_RUNTIME_DIR,
    LINUX_DAEMON_STATE_DIR,
};

const SERVICE: &str = include_str!("../../../packaging/linux/systemd/dasobjectstored.service");
const SYSUSERS: &str = include_str!("../../../packaging/linux/sysusers.d/dasobjectstore.conf");
const TMPFILES: &str = include_str!("../../../packaging/linux/tmpfiles.d/dasobjectstore.conf");
const DAEMON_CONFIG: &str = include_str!("../../../packaging/linux/etc/dasobjectstore/daemon.json");
const BUILD_DEB: &str = include_str!("../../../packaging/debian/build-deb.sh");
const BUILD_RPM: &str = include_str!("../../../packaging/rpm/build-rpm.sh");
const POSTINST: &str = include_str!("../../../packaging/debian/postinst");
const MAKEFILE: &str = include_str!("../../../Makefile");

#[test]
fn package_daemon_config_matches_runtime_defaults() {
    let config: DaemonRuntimeConfig =
        serde_json::from_str(DAEMON_CONFIG).expect("daemon config parses");

    assert_eq!(config, DaemonRuntimeConfig::linux_packaged());
    config.validate().expect("packaged config is valid");
}

#[test]
fn systemd_service_uses_packaged_identity_and_paths() {
    assert_contains(SERVICE, &format!("User={DEFAULT_DAEMON_SERVICE_USER}"));
    assert_contains(SERVICE, &format!("Group={DEFAULT_DAEMON_GROUP}"));
    assert_contains(SERVICE, "RuntimeDirectory=dasobjectstore");
    assert_contains(SERVICE, "StateDirectory=dasobjectstore");
    assert_contains(SERVICE, "LogsDirectory=dasobjectstore");
    assert_contains(
        SERVICE,
        &format!("ExecStart=/usr/bin/dasobjectstored --config {LINUX_DAEMON_CONFIG_PATH}"),
    );
    assert_contains(SERVICE, LINUX_DAEMON_RUNTIME_DIR);
    assert_contains(SERVICE, LINUX_DAEMON_STATE_DIR);
    assert_contains(SERVICE, LINUX_DAEMON_LOG_DIR);
    assert_contains(SERVICE, "ProtectHome=read-only");
}

#[test]
fn sysusers_declares_packaged_service_identity() {
    assert_contains(SYSUSERS, &format!("u {DEFAULT_DAEMON_SERVICE_USER} "));
    assert_contains(SYSUSERS, &format!("g {DEFAULT_DAEMON_GROUP} "));
}

#[test]
fn tmpfiles_declares_daemon_runtime_and_state_directories() {
    assert_contains(
        TMPFILES,
        &format!("d {LINUX_DAEMON_RUNTIME_DIR} 0750 {DEFAULT_DAEMON_SERVICE_USER} {DEFAULT_DAEMON_GROUP} -"),
    );
    assert_contains(
        TMPFILES,
        &format!("d {LINUX_DAEMON_STATE_DIR} 0750 {DEFAULT_DAEMON_SERVICE_USER} {DEFAULT_DAEMON_GROUP} -"),
    );
    assert_contains(
        TMPFILES,
        &format!(
            "d {LINUX_DAEMON_LOG_DIR} 0750 {DEFAULT_DAEMON_SERVICE_USER} {DEFAULT_DAEMON_GROUP} -"
        ),
    );
}

#[test]
fn deb_build_installs_daemon_boundary_assets() {
    assert_contains(BUILD_DEB, "cargo build --release -p dasobjectstore-daemon");
    assert_contains(BUILD_DEB, "cargo build --release -p dasobjectstore-remote");
    assert_contains(
        BUILD_DEB,
        "dpkg-deb is required to build the DASObjectStore Debian package.",
    );
    assert_contains(BUILD_DEB, "target/release/dasobjectstored");
    assert_contains(BUILD_DEB, "target/release/dasobjectstore-remote");
    assert_contains(BUILD_DEB, "usr/bin/dasobjectstore-remote");
    assert_contains(BUILD_DEB, "lib/systemd/system/dasobjectstored.service");
    assert_contains(BUILD_DEB, "usr/lib/sysusers.d/dasobjectstore.conf");
    assert_contains(BUILD_DEB, "usr/lib/tmpfiles.d/dasobjectstore.conf");
    assert_contains(BUILD_DEB, "DEBIAN/postinst");
    assert_contains(BUILD_DEB, "Depends: ca-certificates, acl");
}

#[test]
fn rpm_build_installs_daemon_boundary_assets() {
    assert_contains(BUILD_RPM, "rpmbuild");
    assert_contains(BUILD_RPM, "cargo build --release -p dasobjectstore-daemon");
    assert_contains(BUILD_RPM, "cargo build --release -p dasobjectstore-remote");
    assert_contains(BUILD_RPM, "target/release/dasobjectstored");
    assert_contains(BUILD_RPM, "target/release/dasobjectstore-remote");
    assert_contains(BUILD_RPM, "/usr/bin/dasobjectstore-remote");
    assert_contains(BUILD_RPM, "usr/lib/systemd/system/dasobjectstored.service");
    assert_contains(BUILD_RPM, "usr/lib/sysusers.d/dasobjectstore.conf");
    assert_contains(BUILD_RPM, "usr/lib/tmpfiles.d/dasobjectstore.conf");
    assert_contains(
        BUILD_RPM,
        "systemd-sysusers /usr/lib/sysusers.d/dasobjectstore.conf",
    );
    assert_contains(
        BUILD_RPM,
        "systemd-tmpfiles --create /usr/lib/tmpfiles.d/dasobjectstore.conf",
    );
    assert_contains(BUILD_RPM, "Requires:       ca-certificates");
    assert_contains(BUILD_RPM, "Requires:       acl");
}

#[test]
fn makefile_exposes_distribution_targets() {
    assert_contains(MAKEFILE, "build:");
    assert_contains(MAKEFILE, "cargo build --release --workspace");
    assert_contains(MAKEFILE, "deb:");
    assert_contains(MAKEFILE, "bash packaging/debian/build-deb.sh");
    assert_contains(MAKEFILE, "rpm:");
    assert_contains(MAKEFILE, "bash packaging/rpm/build-rpm.sh");
    assert_contains(MAKEFILE, "package: deb rpm");
    assert_contains(MAKEFILE, "distclean: clean");
}

#[test]
fn deb_postinst_rejects_user_owned_managed_root() {
    assert_contains(POSTINST, "service_user=\"dasobjectstore\"");
    assert_contains(POSTINST, "service_group=\"dasobjectstore\"");
    assert_contains(POSTINST, "managed_root=\"/srv/dasobjectstore\"");
    assert_contains(POSTINST, "reject_user_owned_managed_root \"$managed_root\"");
    assert_contains(
        POSTINST,
        "Managed DAS roots must be owned by $service_user:$service_group",
    );
}

#[test]
fn deb_postinst_repairs_existing_managed_member_roots() {
    assert_contains(POSTINST, "repair_managed_tree()");
    assert_contains(POSTINST, "chown \"$service_user:$service_group\" \"$root\"");
    assert_contains(POSTINST, "chmod 0750 \"$root\"");
    assert_contains(POSTINST, "-path \"$root/lost+found\" -prune");
    assert_contains(
        POSTINST,
        "-exec chown \"$service_user:$service_group\" {} +",
    );
    assert_contains(POSTINST, "-type d -exec chmod 0750 {} +");
    assert_contains(POSTINST, "-type f -exec chmod 0640 {} +");
    assert_contains(POSTINST, "repair_managed_tree \"$managed_root/ssd\"");
    assert_contains(POSTINST, "for root in \"$managed_root\"/hdd/*; do");
    assert_contains(POSTINST, "repair_managed_tree \"$root\"");
}

fn assert_contains(haystack: &str, needle: &str) {
    assert!(
        haystack.contains(needle),
        "expected package asset to contain `{needle}`"
    );
}
