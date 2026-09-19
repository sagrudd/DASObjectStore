const BUILD: &str = include_str!("../../../packaging/debian/build-plugin-process-deb.sh");
const ATTEMPT: &str =
    include_str!("../../../packaging/debian/run-plugin-process-package-attempt.sh");
const VALIDATE: &str = include_str!("../../../packaging/debian/validate-plugin-process-package.sh");
const PROVENANCE: &str = include_str!("../../../packaging/plugin-process-package-provenance.sh");
const PREPARE_WEB_DIST: &str = include_str!("../../../packaging/web/prepare-web-dist.sh");

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

#[test]
fn plugin_process_recipe_is_a_linux_amd64_component_only_fixture() {
    for required in [
        "Package: $package_name",
        "Architecture: amd64",
        "umask 022",
        "install -m 0755 \"$server\" \"$root/usr/bin/dasobjectstore-server\"",
        "find \"$root/opt/dasobjectstore/web\" -type d -exec chmod 0755 {} +",
        "find \"$root/opt/dasobjectstore/web\" -type f -exec chmod 0644 {} +",
        "plugin-process-descriptor.json",
        "\"$root/opt/dasobjectstore/web/\"",
        "das_plugin_process_write_provenance",
        "validate-plugin-process-package.sh",
    ] {
        assert!(
            BUILD.contains(required),
            "missing fixture contract: {required}"
        );
    }
    for forbidden in [
        "dasobjectstored",
        "lib/systemd",
        "DEBIAN/postinst",
        "DEBIAN/prerm",
        "DEBIAN/postrm",
    ] {
        assert!(
            !BUILD.contains(forbidden),
            "forbidden appliance surface: {forbidden}"
        );
    }
}

#[test]
fn plugin_package_version_is_derived_from_copied_cargo_metadata() {
    for required in [
        "metadata --locked --no-deps --format-version 1 --manifest-path \"$repo_root/Cargo.toml\"",
        "select(.name == \"dasobjectstore-cli\") | .version",
        "das_plugin_process_strict_semver \"$version\"",
        "strict DASObjectStore SemVer from copied sealed Cargo metadata",
    ] {
        assert!(
            BUILD.contains(required),
            "missing copied-Cargo version contract: {required}"
        );
    }
    assert!(
        !BUILD.contains("DASObjectStore 0.186.3"),
        "package validation must not retain a fixed stale version gate"
    );
}

#[test]
fn copied_metadata_semver_accepts_the_patch_and_rejects_malformed_values() {
    let source_script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../packaging/plugin-process-package-provenance.sh");
    for version in ["0.186.6", "1.0.0"] {
        let status = Command::new("bash")
            .args([
                "-c",
                "source \"$1\"; das_plugin_process_strict_semver \"$2\"",
                "fixture",
            ])
            .arg(&source_script)
            .arg(version)
            .status()
            .expect("run accepted SemVer regression");
        assert!(status.success(), "strict SemVer {version} must be accepted");
    }
    for version in ["", "01.186.6", "0.186", "0.186.6-beta"] {
        let status = Command::new("bash")
            .args([
                "-c",
                "source \"$1\"; das_plugin_process_strict_semver \"$2\"",
                "fixture",
            ])
            .arg(&source_script)
            .arg(version)
            .status()
            .expect("run malformed SemVer regression");
        assert!(
            !status.success(),
            "malformed SemVer {version:?} must be rejected"
        );
    }
}

#[test]
fn plugin_package_candidate_source_binding_rejects_genuine_mismatch() {
    for required in [
        "candidate source revision to match the source-tree witness",
        "candidate_revision=\"$(sed -n 's/^source_revision",
        "source_tree_revision=\"$(sed -n 's/^revision=",
    ] {
        assert!(
            BUILD.contains(required) || PROVENANCE.contains(required),
            "missing candidate source rejection: {required}"
        );
    }
}

#[test]
fn candidate_source_binding_accepts_the_patch_and_rejects_genuine_mismatch() {
    let temp = std::env::temp_dir().join(format!(
        "dasobjectstore-plugin-process-candidate-source-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time")
            .as_nanos()
    ));
    fs::create_dir(&temp).expect("temporary candidate-source root");
    let stage = staged_fixture(&temp);
    let source_script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../packaging/plugin-process-package-provenance.sh");
    let accepted = Command::new("bash")
        .args([
            "-c",
            "source \"$1\"; das_plugin_process_f05_require_staged_closure \"$2/source\"",
            "fixture",
        ])
        .arg(&source_script)
        .arg(&stage)
        .env("DASOBJECTSTORE_F05_STAGED_CLOSURE_ROOT", &stage)
        .status()
        .expect("read staged candidate source binding");
    assert!(
        accepted.success(),
        "accept matching candidate source binding"
    );
    write(
        stage.join("inputs/source-tree"),
        "revision=bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\n",
    );
    write_f05_manifest(&stage);
    let mismatched = Command::new("bash")
        .args([
            "-c",
            "source \"$1\"; das_plugin_process_f05_require_staged_closure \"$2/source\"",
            "fixture",
        ])
        .arg(&source_script)
        .arg(&stage)
        .env("DASOBJECTSTORE_F05_STAGED_CLOSURE_ROOT", &stage)
        .status()
        .expect("run candidate source mismatch rejection");
    assert!(
        !mismatched.success(),
        "candidate source revision mismatch must be rejected"
    );
    fs::remove_dir_all(temp).expect("remove temporary candidate-source root");
}

#[test]
fn package_validation_rejects_appliance_content_and_requires_descriptor_contract() {
    for required in [
        "dasobjectstore-plugin-process",
        "plugin server must be a Linux amd64 ELF executable",
        "mnemosyne.plugin-process-descriptor/v1",
        "plugin package is missing web index",
        "plugin package is missing WebAssembly assets",
        "plugin package is missing JavaScript assets",
        "plugin package contains forbidden appliance path",
        "plugin package must not contain symlinks",
    ] {
        assert!(
            VALIDATE.contains(required),
            "missing validation contract: {required}"
        );
    }
}

#[test]
fn provenance_binds_every_plugin_only_build_input_and_output() {
    for required in [
        "plugin-process-package-provenance.v1",
        "source_revision",
        "cargo_toml_sha256",
        "cargo_lock_sha256",
        "descriptor_source_sha256",
        "web_assets_sha256",
        "server_sha256",
        "package_sha256",
        "f05_staged_inputs_manifest_sha256",
        "f05_component_candidate_input_sha256",
        "f05_source_tree_sha256",
        "f05_dependency_witness_sha256",
        "f05_package_recipe_sha256",
        "f05_vendor_tree_sha256",
        "f05_vendor_config_sha256",
        "f05_cargo_sha256",
        "f05_rustc_sha256",
        "f05_trunk_sha256",
        "f05_wasm_target_sha256",
        "f05_toolchain_image_sha256",
        "f05_cargo_version",
        "f05_rustc_version",
        "f05_trunk_version",
    ] {
        assert!(
            PROVENANCE.contains(required),
            "missing provenance witness: {required}"
        );
    }
}

#[test]
fn staged_web_closure_is_exact_vendored_and_rejects_ambient_siblings() {
    for required in [
        "DASOBJECTSTORE_F05_STAGED_CLOSURE_ROOT",
        "requires an absolute DASOBJECTSTORE_F05_STAGED_CLOSURE_ROOT",
        "requires stage/source to be the repository root",
        "f05-inputs.sha256",
        "requires a staged vendor tree and vendor config",
        "requires absolute, non-symlink staged cargo, rustc, and trunk tools",
        "requires a staged wasm32-unknown-unknown target",
        "wasm-bindgen-0.2.128/wasm-bindgen",
        "wasm-opt-version_123/wasm-opt",
        "XDG_CACHE_HOME=\"$isolated_xdg_cache\"",
        "wasm-bindgen-0.2.128/wasm-bindgen",
        "wasm-opt-version_123/bin/wasm-opt",
        "requires hash-verified staged wasm-bindgen cache input",
        "requires hash-verified staged wasm-opt cache input",
        "requires f05-inputs.sha256 to bind",
        "requires a non-empty staged vendor tree",
        "requires a non-empty staged wasm32-unknown-unknown target",
        "CARGO_NET_OFFLINE=true",
        "RUSTC=\"$staged_rustc\"",
        "\"$staged_trunk\" build --release",
        "F05_PROSOPIKON_REVISION=f09749273ef382c1b42bf04a77d96189dd7361b3",
        "requires the exact Prosopikon lock revision",
        "rejects an ambient Prosopikon sibling",
        "does not accept a prebuilt web distribution",
        "does not permit the developer fallback",
        "DASOBJECTSTORE_F05_ATTEMPT_ROOT",
        "requires an absolute, non-symlink F05 attempt root",
    ] {
        assert!(
            PREPARE_WEB_DIST.contains(required),
            "missing staged closure contract: {required}"
        );
    }
}

#[test]
fn package_attempt_requires_real_network_namespace_isolation() {
    for required in [
        "/usr/bin/bwrap --unshare-net --ro-bind / / --ro-bind \"$sealed_root\" /opt --bind \"$attempt_root\" /var/tmp --bind \"$diagnostic_root\" /var/cache --proc /proc --dev /dev",
        "--sealed-root /opt --attempt-root /var/tmp --diagnostic-root /var/cache",
        "requires /usr/bin/bwrap network isolation",
        "requires loopback-only network interfaces",
        "requires empty IPv4 routes",
    ] {
        assert!(
            ATTEMPT.contains(required),
            "missing network-isolation contract: {required}"
        );
    }
    assert!(
        !ATTEMPT.contains("--bind /tmp"),
        "the Bubblewrap runner must not make host /tmp writable"
    );
}

#[test]
fn staged_web_build_binds_writable_attempt_tmp_to_rust_and_trunk() {
    for required in [
        "requires a writable per-attempt temporary directory",
        "XDG_CACHE_HOME=\"$isolated_xdg_cache\" TMPDIR=\"$attempt_root/tmp\" TMP=\"$attempt_root/tmp\" TEMP=\"$attempt_root/tmp\"",
    ] {
        assert!(
            PREPARE_WEB_DIST.contains(required),
            "missing staged Trunk temporary-storage contract: {required}"
        );
    }
}

#[test]
fn external_attempt_harness_confines_writes_to_a_copied_closure() {
    for required in [
        "--sealed-root",
        "--attempt-root",
        "--diagnostic-root",
        "reject_symlink_ancestry \"$sealed_root\" 'sealed closure root'",
        "reject_symlink_ancestry \"$attempt_root\" 'external attempt root'",
        "reject_symlink_ancestry \"$diagnostic_root\" 'external diagnostic root'",
        "must not pass through a symlink",
        "sealed closure must not contain symlinks",
        "external attempt root must be empty",
        "external diagnostic root must be empty",
        "external diagnostic root and external attempt root must not overlap",
        "preflight.log",
        "attempt_root_empty=PASS",
        "preflight_failure=$*",
        "cp -a \"$sealed_root\" \"$copied_closure\"",
        "chmod -R u+w \"$copied_closure\"",
        "copied_manifest=\"$copied_source/Cargo.toml\"",
        "copied_lock=\"$copied_source/Cargo.lock\"",
        "copied closure requires a physical non-symlink",
        "build --manifest-path \"$copied_manifest\"",
        "DASOBJECTSTORE_F05_ATTEMPT_ROOT=\"$attempt_root\"",
        "export TMPDIR=\"$attempt_root/tmp\"",
        "diagnostic_status=\"$diagnostic_root/terminal-status\"",
        "printf 'exit_code=%s\\n' \"$status\" > \"$status_file\"",
        "umask 022",
    ] {
        assert!(
            ATTEMPT.contains(required),
            "missing external attempt harness contract: {required}"
        );
    }
}

#[cfg(target_os = "linux")]
#[test]
fn external_attempt_harness_copies_sealed_inputs_and_retains_real_failure_status() {
    let temp = fs::canonicalize(std::env::temp_dir())
        .expect("canonical temporary directory")
        .join(format!(
            "dasobjectstore-plugin-process-attempt-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time")
                .as_nanos()
        ));
    fs::create_dir(&temp).expect("temporary harness root");
    let sealed = staged_fixture(&temp);
    let staged_cargo = sealed.join("toolchain/bin/cargo");
    write(
        &staged_cargo,
        "#!/bin/sh\nprintf 'cwd=%s\\n' \"$PWD\" > \"$DASOBJECTSTORE_F05_ATTEMPT_ROOT/cargo-invocation.log\"\nprintf 'argv=%s\\n' \"$*\" >> \"$DASOBJECTSTORE_F05_ATTEMPT_ROOT/cargo-invocation.log\"\nprintf 'tmpdir=%s\\n' \"$TMPDIR\" >> \"$DASOBJECTSTORE_F05_ATTEMPT_ROOT/cargo-invocation.log\"\nprintf 'umask=%s\\n' \"$(umask)\" >> \"$DASOBJECTSTORE_F05_ATTEMPT_ROOT/cargo-invocation.log\"\ntest \"$TMPDIR\" = \"$DASOBJECTSTORE_F05_ATTEMPT_ROOT/tmp\" && test -d \"$TMPDIR\" && test -w \"$TMPDIR\" || exit 72\nprintf 'tmp_writable=PASS\\n' >> \"$DASOBJECTSTORE_F05_ATTEMPT_ROOT/cargo-invocation.log\"\nexit 71\n",
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&staged_cargo, fs::Permissions::from_mode(0o755))
            .expect("make capturing staged Cargo executable");
    }
    write_f05_manifest(&sealed);
    let manifest = fs::read(sealed.join("f05-inputs.sha256")).expect("read sealed manifest");
    let attempt = temp.join("attempt");
    fs::create_dir(&attempt).expect("create external attempt root");
    let diagnostic = temp.join("diagnostic");
    fs::create_dir(&diagnostic).expect("create external diagnostic root");
    let non_workspace_cwd = temp.join("outside-workspace");
    fs::create_dir(&non_workspace_cwd).expect("create non-workspace cwd");
    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../packaging/debian/run-plugin-process-package-attempt.sh");

    let status = Command::new("bash")
        .args(["-c", "umask 077; exec \"$@\"", "fixture"])
        .arg(&script)
        .args(["--sealed-root"])
        .arg(&sealed)
        .args(["--attempt-root"])
        .arg(&attempt)
        .args(["--diagnostic-root"])
        .arg(&diagnostic)
        .current_dir(&non_workspace_cwd)
        .status()
        .expect("run forced-failure package attempt");
    assert!(
        !status.success(),
        "stub staged Cargo must force a real failure"
    );
    let cargo_invocation = fs::read_to_string(attempt.join("cargo-invocation.log"))
        .expect("captured staged Cargo invocation");
    assert!(
        cargo_invocation.contains("cwd=/var/tmp/closure/source"),
        "Cargo must run from the stable copied-source path rather than the caller cwd"
    );
    assert!(
        cargo_invocation.contains("--manifest-path /var/tmp/closure/source/Cargo.toml"),
        "Cargo must receive the explicit copied manifest path"
    );
    assert!(
        cargo_invocation
            .find("argv=build --manifest-path")
            .is_some(),
        "Cargo must receive the build subcommand before its manifest argument"
    );
    assert!(
        cargo_invocation.contains("tmpdir=/var/tmp/tmp")
            && cargo_invocation.contains("umask=0022")
            && cargo_invocation.contains("tmp_writable=PASS"),
        "the Bubblewrap-isolated staged Rust invocation must override a restrictive caller umask while using stable paths and writable temporary storage"
    );
    assert_eq!(
        fs::read_to_string(diagnostic.join("terminal-status")).expect("read terminal status"),
        format!("exit_code={}\n", status.code().expect("exit code")),
        "attempt must retain the actual nonzero terminal status"
    );
    assert_eq!(
        fs::read(sealed.join("f05-inputs.sha256")).expect("read sealed manifest after attempt"),
        manifest,
        "attempt must not modify the sealed original"
    );
    assert!(
        attempt.join("closure/source").is_dir(),
        "attempt has a work copy"
    );
    assert!(
        fs::read_to_string(attempt.join("closure/source/.cargo/f05-vendor-config.toml"))
            .expect("read copied vendor config")
            .contains("directory = \"/var/tmp/closure/source/vendor\""),
        "the copied Cargo vendor path must use the stable Bubblewrap mount"
    );
    assert!(
        fs::read_to_string(diagnostic.join("preflight.log"))
            .expect("read external preflight log")
            .contains("attempt_root_empty=PASS"),
        "diagnostics must prove the attempt root was empty at runner entry"
    );
    assert!(
        !attempt.join("preflight.log").exists() && !attempt.join("terminal-status").exists(),
        "diagnostics must remain outside the supplied attempt root"
    );
    assert!(
        Command::new("shasum")
            .args(["-a", "256", "-c", "f05-inputs.sha256"])
            .current_dir(attempt.join("closure"))
            .status()
            .expect("verify copied manifest")
            .success(),
        "copied closure manifest must bind the copied vendor configuration"
    );

    let escape = temp.join("escape");
    fs::create_dir(&escape).expect("create symlink escape target");
    let symlink_attempt = temp.join("attempt-link");
    let symlink_diagnostic = temp.join("symlink-diagnostic");
    fs::create_dir(&symlink_diagnostic).expect("create symlink diagnostic root");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&escape, &symlink_attempt).expect("create attempt symlink");
    let escaped = Command::new("bash")
        .arg(&script)
        .args(["--sealed-root"])
        .arg(&sealed)
        .args(["--attempt-root"])
        .arg(&symlink_attempt)
        .args(["--diagnostic-root"])
        .arg(&symlink_diagnostic)
        .status()
        .expect("run symlink escape attempt");
    assert!(
        !escaped.success(),
        "harness must reject an attempt-root symlink"
    );
    assert!(
        !escape.join("terminal-status").exists(),
        "harness must not write through the symlink escape"
    );

    let symlink_parent_target = temp.join("symlink-parent-target");
    fs::create_dir(&symlink_parent_target).expect("create symlink parent target");
    let symlink_parent = temp.join("symlink-parent");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&symlink_parent_target, &symlink_parent)
        .expect("create symlinked attempt parent");
    let attempt_below_symlink = symlink_parent.join("attempt");
    let parent_diagnostic = temp.join("parent-diagnostic");
    fs::create_dir(&parent_diagnostic).expect("create parent diagnostic root");
    let ancestry_rejected = Command::new("bash")
        .arg(&script)
        .args(["--sealed-root"])
        .arg(&sealed)
        .args(["--attempt-root"])
        .arg(&attempt_below_symlink)
        .args(["--diagnostic-root"])
        .arg(&parent_diagnostic)
        .status()
        .expect("run symlink-parent attempt");
    assert!(
        !ancestry_rejected.success(),
        "harness must reject an attempt root below a symlinked parent"
    );
    assert!(
        !symlink_parent_target.join("attempt").exists(),
        "harness must not create a closure or terminal status through a symlinked parent"
    );

    let nonempty_attempt = temp.join("nonempty-attempt");
    fs::create_dir(&nonempty_attempt).expect("create nonempty attempt root");
    write(
        nonempty_attempt.join("launcher.log"),
        "external diagnostic\n",
    );
    let preflight_diagnostic = temp.join("preflight-diagnostic");
    fs::create_dir(&preflight_diagnostic).expect("create preflight diagnostic root");
    let preflight_rejected = Command::new("bash")
        .arg(&script)
        .args(["--sealed-root"])
        .arg(&sealed)
        .args(["--attempt-root"])
        .arg(&nonempty_attempt)
        .args(["--diagnostic-root"])
        .arg(&preflight_diagnostic)
        .status()
        .expect("run nonempty attempt-root rejection");
    assert!(
        !preflight_rejected.success(),
        "harness must reject a nonempty attempt root before Bubblewrap"
    );
    assert!(
        fs::read_to_string(preflight_diagnostic.join("preflight.log"))
            .expect("read external preflight failure")
            .contains("preflight_failure=external attempt root must be empty"),
        "preflight failure must be retained outside the rejected attempt root"
    );
    assert!(
        !nonempty_attempt.join("terminal-status").exists(),
        "runner must not write a status into the rejected attempt root"
    );
    fs::remove_dir_all(temp).expect("remove temporary harness root");
}

#[cfg(target_os = "linux")]
#[test]
fn external_attempt_harness_binds_writable_tmp_for_staged_trunk() {
    use std::os::unix::fs::PermissionsExt;

    let temp = fs::canonicalize(std::env::temp_dir())
        .expect("canonical temporary directory")
        .join(format!(
            "dasobjectstore-plugin-process-trunk-tmp-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time")
                .as_nanos()
        ));
    fs::create_dir(&temp).expect("temporary harness root");
    let sealed = staged_fixture(&temp);
    let attempt = temp.join("attempt");
    let diagnostic = temp.join("diagnostic");
    fs::create_dir(&attempt).expect("create external attempt root");
    fs::create_dir(&diagnostic).expect("create external diagnostic root");
    let staged_cargo = sealed.join("toolchain/bin/cargo");
    write(&staged_cargo, "#!/bin/sh\nexit 0\n");
    fs::set_permissions(&staged_cargo, fs::Permissions::from_mode(0o755))
        .expect("make staged Cargo executable");

    let network_download = attempt.join("network-download");
    let staged_curl = sealed.join("network-denied-bin/curl");
    write(
        &staged_curl,
        &format!(
            "#!/bin/sh\nprintf 'network_request=UNEXPECTED\\n' > \"{}\"\nexit 75\n",
            network_download.display()
        ),
    );
    fs::set_permissions(&staged_curl, fs::Permissions::from_mode(0o755))
        .expect("make staged downloader denial executable");

    let staged_trunk = sealed.join("toolchain/bin/trunk");
    write(
        &staged_trunk,
        &format!(
            "#!/bin/sh\nset -eu\ntest \"$TMPDIR\" = \"/var/tmp/tmp\"\ntest \"$TMP\" = \"$TMPDIR\"\ntest \"$TEMP\" = \"$TMPDIR\"\ntest \"$XDG_CACHE_HOME\" = \"/var/tmp/xdg-cache\"\ntest -d \"$TMPDIR\" && test -w \"$TMPDIR\"\nbindgen=\"$XDG_CACHE_HOME/trunk/wasm-bindgen-0.2.128/wasm-bindgen\"\nwasm_opt=\"$XDG_CACHE_HOME/trunk/wasm-opt-version_123/bin/wasm-opt\"\nif ! test -x \"$bindgen\" || ! test -x \"$wasm_opt\"; then\n  curl https://example.invalid/trunk-tool\nfi\nprintf 'tmpdir=%s\\ntmp=%s\\ntemp=%s\\nxdg_cache=%s\\ncached_wasm_bindgen=%s\\ncached_wasm_opt=%s\\ndownloader=NOT_INVOKED\\n' \"$TMPDIR\" \"$TMP\" \"$TEMP\" \"$XDG_CACHE_HOME\" \"$bindgen\" \"$wasm_opt\" > \"$TMPDIR/trunk-env.log\"\n: > \"$TMPDIR/trunk-temp-proof\"\nif test -w /tmp; then\n  printf 'host_tmp_writable=UNEXPECTED\\n' >> \"$TMPDIR/trunk-env.log\"\n  exit 74\nfi\nprintf 'host_tmp_writable=DENIED\\n' >> \"$TMPDIR/trunk-env.log\"\nexit 73\n",
        ),
    );
    fs::set_permissions(&staged_trunk, fs::Permissions::from_mode(0o755))
        .expect("make staged Trunk executable");
    let staged_prepare = sealed.join("source/packaging/web/prepare-web-dist.sh");
    write(&staged_prepare, PREPARE_WEB_DIST);
    fs::set_permissions(&staged_prepare, fs::Permissions::from_mode(0o755))
        .expect("make staged web preparer executable");
    fs::create_dir_all(sealed.join("source/crates/dasobjectstore-gui-web"))
        .expect("create staged web root");
    write_f05_manifest(&sealed);
    let manifest = fs::read(sealed.join("f05-inputs.sha256")).expect("read sealed manifest");

    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../packaging/debian/run-plugin-process-package-attempt.sh");
    let status = Command::new("bash")
        .arg(&script)
        .args(["--sealed-root"])
        .arg(&sealed)
        .args(["--attempt-root"])
        .arg(&attempt)
        .args(["--diagnostic-root"])
        .arg(&diagnostic)
        .status()
        .expect("run staged Trunk temporary-storage fixture");
    assert!(
        !status.success(),
        "fake Trunk must stop the fixture after its proof"
    );

    let trunk_environment = fs::read_to_string(attempt.join("tmp/trunk-env.log"))
        .expect("read staged Trunk temporary-storage evidence");
    let expected_tmp = "/var/tmp/tmp";
    let expected_xdg_cache = "/var/tmp/xdg-cache";
    assert!(
        trunk_environment.contains(&format!("tmpdir={expected_tmp}"))
            && trunk_environment.contains(&format!("tmp={expected_tmp}"))
            && trunk_environment.contains(&format!("temp={expected_tmp}"))
            && trunk_environment.contains(&format!("xdg_cache={expected_xdg_cache}"))
            && trunk_environment.contains(&format!(
                "cached_wasm_bindgen={expected_xdg_cache}/trunk/wasm-bindgen-0.2.128/wasm-bindgen"
            ))
            && trunk_environment.contains(&format!(
                "cached_wasm_opt={expected_xdg_cache}/trunk/wasm-opt-version_123/bin/wasm-opt"
            ))
            && trunk_environment.contains("downloader=NOT_INVOKED")
            && trunk_environment.contains("host_tmp_writable=DENIED"),
        "staged Trunk must receive only the external temporary and hash-verified tool cache directories"
    );
    assert!(
        attempt.join("tmp/trunk-temp-proof").is_file()
            && attempt
                .join("xdg-cache/trunk/wasm-bindgen-0.2.128/wasm-bindgen")
                .is_file()
            && attempt
                .join("xdg-cache/trunk/wasm-opt-version_123/bin/wasm-opt")
                .is_file()
            && !network_download.exists()
            && !sealed.join("tmp").exists(),
        "temporary and Trunk-cache writes must remain under the external attempt root without downloader fallback"
    );
    assert_eq!(
        fs::read(sealed.join("f05-inputs.sha256")).expect("read sealed manifest after attempt"),
        manifest,
        "fake Trunk fixture must not alter the sealed inputs"
    );
    fs::remove_dir_all(temp).expect("remove temporary harness root");
}

#[cfg(target_os = "linux")]
#[test]
#[ignore = "requires DASOBJECTSTORE_F05_REAL_STAGED_CLOSURE and performs a real staged Trunk build"]
fn real_staged_trunk_uses_hydrated_xdg_cache_without_a_downloader() {
    use std::os::unix::fs::PermissionsExt;

    let source_closure = PathBuf::from(
        std::env::var("DASOBJECTSTORE_F05_REAL_STAGED_CLOSURE")
            .expect("real Trunk regression requires DASOBJECTSTORE_F05_REAL_STAGED_CLOSURE"),
    );
    assert!(
        source_closure.is_absolute()
            && source_closure.is_dir()
            && !fs::symlink_metadata(&source_closure)
                .expect("inspect real staged closure")
                .file_type()
                .is_symlink(),
        "real Trunk regression requires an absolute physical staged closure"
    );
    assert!(
        Command::new("shasum")
            .args(["-a", "256", "-c", "f05-inputs.sha256"])
            .current_dir(&source_closure)
            .status()
            .expect("verify real staged closure before fixture")
            .success(),
        "real staged closure must be intact before the Bubblewrap fixture"
    );

    let real_trunk = source_closure.join("toolchain/bin/trunk");
    let real_trunk_version = Command::new(&real_trunk)
        .arg("--version")
        .output()
        .expect("run staged Trunk version check");
    assert!(
        real_trunk_version.status.success()
            && String::from_utf8_lossy(&real_trunk_version.stdout).contains("trunk 0.21.14"),
        "fixture requires the declared real staged Trunk 0.21.14"
    );

    let temp = fs::canonicalize(std::env::temp_dir())
        .expect("canonical temporary directory")
        .join(format!(
            "dasobjectstore-real-staged-trunk-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time")
                .as_nanos()
        ));
    fs::create_dir(&temp).expect("create real Trunk fixture root");
    let attempt = temp.join("attempt");
    fs::create_dir(&attempt).expect("create real Trunk external attempt root");
    let copied_closure = attempt.join("closure");
    let copy_status = Command::new("cp")
        .args(["-a"])
        .arg(&source_closure)
        .arg(&copied_closure)
        .status()
        .expect("copy real staged closure into the external attempt root");
    assert!(copy_status.success(), "copy real staged closure");
    let writable_copy = Command::new("chmod")
        .args(["-R", "u+w"])
        .arg(&copied_closure)
        .status()
        .expect("make only the copied closure writable");
    assert!(writable_copy.success(), "make copied closure writable");

    let copied_prepare = copied_closure.join("source/packaging/web/prepare-web-dist.sh");
    fs::write(&copied_prepare, PREPARE_WEB_DIST)
        .expect("install tested web preparer in copied closure");
    fs::set_permissions(&copied_prepare, fs::Permissions::from_mode(0o755))
        .expect("make copied web preparer executable");
    let copied_vendor_config = copied_closure.join("source/.cargo/f05-vendor-config.toml");
    let copied_vendor_path = copied_closure.join("source/vendor");
    let vendor_config =
        fs::read_to_string(&copied_vendor_config).expect("read copied vendor config");
    let rewritten_vendor_config = vendor_config
        .lines()
        .map(|line| {
            if line.starts_with("directory = ") {
                format!("directory = \"{}\"", copied_vendor_path.display())
            } else {
                line.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(
        &copied_vendor_config,
        format!("{rewritten_vendor_config}\n"),
    )
    .expect("bind copied vendor config to copied source");
    let copied_manifest = Command::new("bash")
        .arg("-ceu")
        .arg(
            "cd \"$1\"\nfind . -type f ! -name f05-inputs.sha256 -print | LC_ALL=C sort | while IFS= read -r input; do\n  printf '%s  %s\\n' \"$(shasum -a 256 \"$input\" | awk '{print $1}')\" \"${input#./}\"\ndone > f05-inputs.sha256",
        )
        .arg("fixture")
        .arg(&copied_closure)
        .status()
        .expect("rebind copied closure manifest to the tested preparer");
    assert!(copied_manifest.success(), "rebind copied closure manifest");
    fs::create_dir_all(attempt.join("tmp")).expect("create external temporary directory");

    let bwrap_script = "test ! -w /tmp\ntest -d \"$DASOBJECTSTORE_F05_ATTEMPT_ROOT/tmp\" && test -w \"$DASOBJECTSTORE_F05_ATTEMPT_ROOT/tmp\"\nexec bash \"$DASOBJECTSTORE_F05_STAGED_CLOSURE_ROOT/source/packaging/web/prepare-web-dist.sh\"";
    let bwrap_output = Command::new("/usr/bin/bwrap")
        .args(["--unshare-net", "--ro-bind", "/", "/", "--bind"])
        .arg(&attempt)
        .arg(&attempt)
        .args(["--proc", "/proc", "--dev", "/dev"])
        .args(["--setenv", "DASOBJECTSTORE_F05_STAGED_CLOSURE_ROOT"])
        .arg(&copied_closure)
        .args(["--setenv", "DASOBJECTSTORE_F05_ATTEMPT_ROOT"])
        .arg(&attempt)
        .args(["--", "/usr/bin/bash", "-ceu", bwrap_script])
        .output()
        .expect("run real staged Trunk under Bubblewrap");
    let bwrap_log = temp.join("real-staged-trunk-bwrap.log");
    let mut combined_log = bwrap_output.stdout;
    combined_log.extend_from_slice(&bwrap_output.stderr);
    fs::write(&bwrap_log, &combined_log).expect("retain Bubblewrap fixture log");
    assert!(
        bwrap_output.status.success(),
        "real staged Trunk must build from the hydrated cache under network-denied Bubblewrap: {}",
        String::from_utf8_lossy(&combined_log)
    );
    let bwrap_log_text = String::from_utf8_lossy(&combined_log);
    assert!(
        !bwrap_log_text
            .to_ascii_lowercase()
            .contains("downloading wasm-bindgen"),
        "hydrated staged wasm-bindgen cache must avoid a downloader request"
    );
    assert!(
        attempt
            .join("xdg-cache/trunk/wasm-bindgen-0.2.128/wasm-bindgen")
            .is_file()
            && attempt
                .join("xdg-cache/trunk/wasm-opt-version_123/bin/wasm-opt")
                .is_file()
            && attempt
                .join("closure/source/crates/dasobjectstore-gui-web/dist/index.html")
                .is_file(),
        "real staged Trunk must use hydrated external cache and write web output only in the copied closure"
    );
    assert!(
        Command::new("shasum")
            .args(["-a", "256", "-c", "f05-inputs.sha256"])
            .current_dir(&source_closure)
            .status()
            .expect("reverify real staged closure after fixture")
            .success(),
        "real staged Trunk fixture must leave the sealed source closure immutable"
    );
    fs::remove_dir_all(temp).expect("remove external real Trunk fixture root");
}

fn write(path: impl AsRef<Path>, contents: &str) {
    let path = path.as_ref();
    fs::create_dir_all(path.parent().expect("fixture parent")).expect("create fixture parent");
    fs::write(path, contents).expect("write fixture input");
}

fn executable(path: impl AsRef<Path>, version: &str) {
    let path = path.as_ref();
    write(
        path,
        &format!(
            "#!/bin/sh\n[ \"${{1-}}\" = --version ] && {{ echo {version}; exit 0; }}\nexit 1\n"
        ),
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755))
            .expect("make staged tool executable");
    }
}

fn sha256(path: &Path) -> String {
    let output = Command::new("shasum")
        .args(["-a", "256"])
        .arg(path)
        .output()
        .expect("hash fixture input");
    assert!(output.status.success(), "hash fixture input");
    String::from_utf8(output.stdout)
        .expect("hash output")
        .split_whitespace()
        .next()
        .expect("hash value")
        .to_owned()
}

fn copy_tree(from: &Path, to: &Path) {
    let status = Command::new("cp")
        .args(["-a"])
        .arg(from)
        .arg(to)
        .status()
        .expect("copy staged fixture");
    assert!(status.success(), "copy staged fixture");
}

fn write_f05_manifest(stage: &Path) {
    let mut files = Vec::new();
    fn collect(root: &Path, current: &Path, files: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(current).expect("read staged fixture directory") {
            let path = entry.expect("staged fixture entry").path();
            if path.is_dir() {
                collect(root, &path, files);
            } else if path
                .file_name()
                .is_some_and(|name| name != "f05-inputs.sha256")
            {
                files.push(
                    path.strip_prefix(root)
                        .expect("relative staged input")
                        .to_owned(),
                );
            }
        }
    }
    collect(stage, stage, &mut files);
    files.sort();
    let manifest = files
        .iter()
        .map(|relative| format!("{}  {}", sha256(&stage.join(relative)), relative.display()))
        .collect::<Vec<_>>()
        .join("\n");
    write(stage.join("f05-inputs.sha256"), &(manifest + "\n"));
}

fn staged_fixture(root: &Path) -> PathBuf {
    let stage = root.join("closure");
    let source = stage.join("source");
    write(
        source.join("Cargo.toml"),
        "[workspace]\nresolver = \"2\"\n\n[workspace.dependencies]\nprosopikon-core = { git = \"https://github.com/sagrudd/prosopikon.git\", rev = \"f09749273ef382c1b42bf04a77d96189dd7361b3\" }\nprosopikon-yew = { git = \"https://github.com/sagrudd/prosopikon.git\", rev = \"f09749273ef382c1b42bf04a77d96189dd7361b3\" }\n",
    );
    write(
        source.join("Cargo.lock"),
        "version = 4\nsource = \"git+https://github.com/sagrudd/prosopikon.git?rev=f09749273ef382c1b42bf04a77d96189dd7361b3#f09749273ef382c1b42bf04a77d96189dd7361b3\"\n",
    );
    write(
        source.join(".cargo/f05-vendor-config.toml"),
        &format!(
            "[source.vendored-sources]\ndirectory = \"{}\"\n",
            source.join("vendor").display()
        ),
    );
    write(source.join("vendor/fixture.crate"), "vendored dependency\n");
    write(
        stage.join("inputs/component-candidate-input.toml"),
        "source_revision = \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"\ntoolchain_image = \"docker.io/library/rust@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"\ntoolchain_image_sha256 = \"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"\n",
    );
    write(
        stage.join("inputs/source-tree"),
        "revision=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n",
    );
    write(
        stage.join("inputs/compiled-dependency-witness.json"),
        "{\"dependencies\":[]}\n",
    );
    write(
        stage.join("inputs/package-recipe.json"),
        "{\"package\":\"fixture\"}\n",
    );
    executable(stage.join("toolchain/bin/cargo"), "cargo fixture 1.0.0");
    executable(stage.join("toolchain/bin/rustc"), "rustc fixture 1.0.0");
    executable(stage.join("toolchain/bin/trunk"), "trunk fixture 1.0.0");
    executable(
        stage.join("toolchain/trunk-tools/wasm-bindgen-0.2.128/wasm-bindgen"),
        "wasm-bindgen 0.2.128",
    );
    executable(
        stage.join("toolchain/trunk-tools/wasm-opt-version_123/wasm-opt"),
        "wasm-opt version_123",
    );
    write(
        stage.join("toolchain/lib/rustlib/wasm32-unknown-unknown/libfixture.rlib"),
        "wasm target\n",
    );
    fs::create_dir_all(stage.join("staging-home")).expect("create staged home");
    fs::create_dir_all(stage.join("network-denied-bin")).expect("create network denial path");
    write(stage.join("web/index.html"), "<html></html>\n");
    write(stage.join("server"), "server fixture\n");
    write(stage.join("artifact.deb"), "package fixture\n");
    write(
        source.join("packaging/linux/opt/dasobjectstore/plugin-process-descriptor.json"),
        "{\"schema\":\"fixture\"}\n",
    );
    write_f05_manifest(&stage);
    stage
}

fn staged_provenance(stage: &Path) -> std::process::ExitStatus {
    let source_script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../packaging/plugin-process-package-provenance.sh");
    Command::new("bash")
        .args(["-c", "source \"$1\"; das_plugin_process_write_provenance \"$2/artifact.deb\" \"$2/source\" 0.186.6 amd64 \"$2/web\" \"$2/server\"", "fixture"])
        .arg(source_script)
        .arg(stage)
        .env("DASOBJECTSTORE_F05_STAGED_CLOSURE_ROOT", stage)
        .env(
            "DASOBJECTSTORE_SOURCE_REVISION",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        )
        .env("SOURCE_DATE_EPOCH", "1789751689")
        .status()
        .expect("run staged provenance fixture")
}

#[test]
fn staged_provenance_is_json_and_denies_tampered_closure_inputs() {
    let temp = std::env::temp_dir().join(format!(
        "dasobjectstore-plugin-process-contract-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time")
            .as_nanos()
    ));
    fs::create_dir(&temp).expect("temporary staged closure root");
    let stage = staged_fixture(&temp);
    assert!(
        staged_provenance(&stage).success(),
        "accept exact staged closure"
    );
    let provenance = stage.join("artifact.deb.provenance.json");
    assert!(
        Command::new("jq")
            .arg("empty")
            .arg(&provenance)
            .status()
            .expect("parse staged provenance JSON")
            .success(),
        "staged provenance sidecar must be JSON"
    );

    for (name, path, replacement) in [
        ("missing-cargo", "toolchain/bin/cargo", None),
        ("altered-rustc", "toolchain/bin/rustc", Some("altered rustc\n")),
        ("decoy-trunk", "toolchain/bin/trunk", Some("#!/bin/sh\necho decoy\n")),
        (
            "missing-wasm",
            "toolchain/lib/rustlib/wasm32-unknown-unknown/libfixture.rlib",
            None,
        ),
        ("altered-vendor", "source/vendor/fixture.crate", Some("altered vendor\n")),
        ("missing-config", "source/.cargo/f05-vendor-config.toml", None),
        (
            "altered-candidate",
            "inputs/component-candidate-input.toml",
            Some("toolchain_image = \"docker.io/library/rust@sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\"\ntoolchain_image_sha256 = \"sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\"\n"),
        ),
        (
            "altered-image",
            "inputs/component-candidate-input.toml",
            Some("toolchain_image = \"docker.io/library/rust@sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc\"\ntoolchain_image_sha256 = \"sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc\"\n"),
        ),
    ] {
        let variant_root = temp.join(name);
        fs::create_dir(&variant_root).expect("create staged closure variant root");
        copy_tree(&stage, &variant_root);
        let variant = variant_root.join("closure");
        let target = variant.join(path);
        if let Some(replacement) = replacement {
            write(&target, replacement);
            if path.starts_with("toolchain/bin/") {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    fs::set_permissions(&target, fs::Permissions::from_mode(0o755))
                        .expect("make decoy staged tool executable");
                }
            }
        } else {
            fs::remove_file(&target).expect("remove staged input");
        }
        assert!(
            !staged_provenance(&variant).success(),
            "staged provenance accepted {name} closure input"
        );
    }
    fs::remove_dir_all(temp).expect("remove temporary staged closure root");
}
