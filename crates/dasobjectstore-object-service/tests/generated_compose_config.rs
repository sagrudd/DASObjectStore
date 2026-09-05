use dasobjectstore_core::ids::StoreId;
use dasobjectstore_core::store::{StoreClass, StorePolicy};
use dasobjectstore_object_service::{
    plan_store_service_layout, render_compose, ComposeRenderRequest, ComposeServiceConfig,
    ObjectServiceProviderId, StoreServiceDefinition,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[test]
fn generated_compose_is_accepted_by_local_docker_compose_when_available() {
    if !docker_compose_available() {
        eprintln!("skipping generated Compose config test: docker compose is unavailable");
        return;
    }

    let root = temp_root("generated-compose-config");
    fs::create_dir_all(&root).expect("create temp root");
    let compose_file = root.join("compose.yaml");
    let rendered = render_sample_compose(&root);
    fs::write(&compose_file, rendered).expect("write compose file");

    let output = Command::new("docker")
        .args([
            "compose",
            "-f",
            compose_file.to_str().expect("utf8 compose path"),
            "config",
        ])
        .output()
        .expect("run docker compose config");

    fs::remove_dir_all(root).expect("cleanup temp root");

    assert!(
        output.status.success(),
        "docker compose config failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn docker_compose_available() -> bool {
    Command::new("docker")
        .args(["compose", "version"])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn render_sample_compose(root: &Path) -> String {
    let layout = plan_store_service_layout(&[StoreServiceDefinition {
        store_id: StoreId::new("generated").expect("store id"),
        policy: StorePolicy::defaults_for(StoreClass::GeneratedData),
        bucket_name: None,
        reader_group: None,
        writer_group: None,
        public: false,
    }])
    .expect("layout planned");
    let request = ComposeRenderRequest {
        project_name: "dasobjectstore-compose-test".to_string(),
        ssd_metadata_path: root.join("ssd-meta").to_string_lossy().to_string(),
        hdd_data_path: root.join("hdd-data").to_string_lossy().to_string(),
        store_bindings: layout.bucket_bindings,
    };
    let service = ComposeServiceConfig::new(
        ObjectServiceProviderId::Garage,
        "object-service",
        "busybox:latest",
        3900,
    );

    render_compose(&request, &service)
        .expect("compose renders")
        .compose_yaml
}

fn temp_root(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "dasobjectstore-{name}-{}-{}",
        std::process::id(),
        unique_suffix()
    ))
}

fn unique_suffix() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system time after epoch")
        .as_nanos()
}
