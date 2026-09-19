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
        "install -m 0755 \"$server\" \"$root/usr/bin/dasobjectstore-server\"",
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
fn external_attempt_harness_confines_writes_to_a_copied_closure() {
    for required in [
        "--sealed-root",
        "--attempt-root",
        "reject_symlink_ancestry \"$sealed_root\" 'sealed closure root'",
        "reject_symlink_ancestry \"$attempt_root\" 'external attempt root'",
        "must not pass through a symlink",
        "sealed closure must not contain symlinks",
        "external attempt root must be empty",
        "cp -a \"$sealed_root\" \"$copied_closure\"",
        "chmod -R u+w \"$copied_closure\"",
        "copied_manifest=\"$copied_source/Cargo.toml\"",
        "copied_lock=\"$copied_source/Cargo.lock\"",
        "copied closure requires a physical non-symlink",
        "build --manifest-path \"$copied_manifest\"",
        "DASOBJECTSTORE_F05_ATTEMPT_ROOT=\"$attempt_root\"",
        "terminal-status",
        "printf 'exit_code=%s\\n' \"$status\" > \"$status_file\"",
    ] {
        assert!(
            ATTEMPT.contains(required),
            "missing external attempt harness contract: {required}"
        );
    }
}

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
        "#!/bin/sh\nprintf 'cwd=%s\\n' \"$PWD\" > \"$DASOBJECTSTORE_F05_ATTEMPT_ROOT/cargo-invocation.log\"\nprintf 'argv=%s\\n' \"$*\" >> \"$DASOBJECTSTORE_F05_ATTEMPT_ROOT/cargo-invocation.log\"\nexit 71\n",
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
    let non_workspace_cwd = temp.join("outside-workspace");
    fs::create_dir(&non_workspace_cwd).expect("create non-workspace cwd");
    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../packaging/debian/run-plugin-process-package-attempt.sh");

    let status = Command::new("bash")
        .arg(&script)
        .args(["--sealed-root"])
        .arg(&sealed)
        .args(["--attempt-root"])
        .arg(&attempt)
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
        cargo_invocation.contains(&format!("cwd={}", attempt.join("closure/source").display())),
        "Cargo must run from the copied source rather than the caller cwd"
    );
    assert!(
        cargo_invocation.contains(&format!(
            "--manifest-path {}",
            attempt.join("closure/source/Cargo.toml").display()
        )),
        "Cargo must receive the explicit copied manifest path"
    );
    assert!(
        cargo_invocation
            .find("argv=build --manifest-path")
            .is_some(),
        "Cargo must receive the build subcommand before its manifest argument"
    );
    assert_eq!(
        fs::read_to_string(attempt.join("terminal-status")).expect("read terminal status"),
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
    #[cfg(unix)]
    std::os::unix::fs::symlink(&escape, &symlink_attempt).expect("create attempt symlink");
    let escaped = Command::new("bash")
        .arg(&script)
        .args(["--sealed-root"])
        .arg(&sealed)
        .args(["--attempt-root"])
        .arg(&symlink_attempt)
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
    let ancestry_rejected = Command::new("bash")
        .arg(&script)
        .args(["--sealed-root"])
        .arg(&sealed)
        .args(["--attempt-root"])
        .arg(&attempt_below_symlink)
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
    fs::remove_dir_all(temp).expect("remove temporary harness root");
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
        "toolchain_image = \"docker.io/library/rust@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"\ntoolchain_image_sha256 = \"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"\n",
    );
    write(stage.join("inputs/source-tree"), "source tree witness\n");
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
        .args(["-c", "source \"$1\"; das_plugin_process_write_provenance \"$2/artifact.deb\" \"$2/source\" 0.186.3 amd64 \"$2/web\" \"$2/server\"", "fixture"])
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
