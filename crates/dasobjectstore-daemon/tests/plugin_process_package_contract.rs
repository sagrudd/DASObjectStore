const BUILD: &str = include_str!("../../../packaging/debian/build-plugin-process-deb.sh");
const VALIDATE: &str = include_str!("../../../packaging/debian/validate-plugin-process-package.sh");
const PROVENANCE: &str = include_str!("../../../packaging/plugin-process-package-provenance.sh");
const PREPARE_WEB_DIST: &str = include_str!("../../../packaging/web/prepare-web-dist.sh");

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
        "CARGO_NET_OFFLINE=true",
        "F05_PROSOPIKON_REVISION=f09749273ef382c1b42bf04a77d96189dd7361b3",
        "requires the exact Prosopikon lock revision",
        "rejects an ambient Prosopikon sibling",
        "does not accept a prebuilt web distribution",
        "does not permit the developer fallback",
    ] {
        assert!(
            PREPARE_WEB_DIST.contains(required),
            "missing staged closure contract: {required}"
        );
    }
}
