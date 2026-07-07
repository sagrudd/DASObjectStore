use dasobjectstore_daemon::{
    DaemonRuntimeConfig, DEFAULT_DAEMON_GROUP, DEFAULT_DAEMON_SERVICE_USER,
    LINUX_DAEMON_CONFIG_PATH, LINUX_DAEMON_LOG_DIR, LINUX_DAEMON_RUNTIME_DIR,
    LINUX_DAEMON_STATE_DIR,
};

const SERVICE: &str = include_str!("../../../packaging/linux/systemd/dasobjectstored.service");
const SYSUSERS: &str = include_str!("../../../packaging/linux/sysusers.d/dasobjectstore.conf");
const TMPFILES: &str = include_str!("../../../packaging/linux/tmpfiles.d/dasobjectstore.conf");
const DAEMON_CONFIG: &str = include_str!("../../../packaging/linux/etc/dasobjectstore/daemon.json");

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

fn assert_contains(haystack: &str, needle: &str) {
    assert!(
        haystack.contains(needle),
        "expected package asset to contain `{needle}`"
    );
}
