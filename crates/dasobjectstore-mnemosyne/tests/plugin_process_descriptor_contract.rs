use std::{env, fs, path::PathBuf};

use serde_json::Value;

const DESCRIPTOR_SCHEMA: &str = "mnemosyne.plugin-process-descriptor/v1";

#[test]
fn packaged_descriptor_declares_the_das_process_plugin_surface() {
    let descriptor = read_json(
        repo_root().join("packaging/linux/opt/dasobjectstore/plugin-process-descriptor.json"),
    );

    assert_eq!(descriptor["schema"], DESCRIPTOR_SCHEMA);
    assert_eq!(descriptor["productId"], "dasobjectstore");
    assert_eq!(descriptor["hostProtocolMin"], 1);
    assert_eq!(descriptor["hostProtocolMax"], 1);
    assert_eq!(
        descriptor["upstreamUnixSocket"],
        "/run/dasobjectstore/plugin-process.sock"
    );
    assert_ne!(
        descriptor["upstreamUnixSocket"], "/run/dasobjectstore/dasobjectstored.sock",
        "the plugin surface must not reuse the daemon control socket"
    );
    assert_eq!(descriptor["healthPath"], "/health");
    assert_eq!(descriptor["uiMount"], "/products/dasobjectstore/");
    assert_eq!(descriptor["apiMount"], "/products/dasobjectstore/api/");
    assert_eq!(descriptor["audience"], "monas:dasobjectstore");
    assert_eq!(descriptor["version"], env!("CARGO_PKG_VERSION"));
}

#[test]
fn package_builders_retain_the_descriptor_without_a_monas_dependency() {
    let root = repo_root();
    let descriptor = "plugin-process-descriptor.json";
    for script in [
        "packaging/debian/build-deb.sh",
        "packaging/rpm/build-rpm.sh",
        "packaging/debian/validate-package-assets.sh",
    ] {
        let contents = fs::read_to_string(root.join(script)).expect("package script reads");
        assert!(contents.contains(descriptor), "{script} retains descriptor");
    }
    let cli_manifest = fs::read_to_string(root.join("crates/dasobjectstore-cli/Cargo.toml"))
        .expect("CLI manifest reads");
    assert!(
        !cli_manifest.contains("monas"),
        "DAS descriptor bridge must not add a Monas crate dependency"
    );
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root resolves")
}

fn read_json(path: PathBuf) -> Value {
    let raw = fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!("read {}: {error}", path.display());
    });
    serde_json::from_str(&raw).unwrap_or_else(|error| {
        panic!("parse {}: {error}", path.display());
    })
}
