use crate::endpoints::EndpointInventoryView;
use crate::endpoints_registry::{default_endpoints_registry_path, read_endpoint_inventory};
use crate::workspaces::EndpointsWorkspaceView;

pub fn live_endpoint_inventory() -> EndpointInventoryView {
    read_endpoint_inventory(&default_endpoints_registry_path()).inventory
}

pub fn live_endpoints_workspace() -> EndpointsWorkspaceView {
    EndpointsWorkspaceView {
        inventory: live_endpoint_inventory(),
    }
}

#[cfg(test)]
mod tests {
    use crate::endpoints_registry::read_endpoint_inventory;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn endpoint_registry_snapshot_feeds_workspace_inventory() {
        let root = temp_root("endpoint-aggregator");
        let path = root.join("endpoints.json");
        fs::write(
            &path,
            r#"{"endpoints":[{"endpoint_id":"endpoint-a","display_name":"Endpoint A","kind":"dasobjectstore_das","object_service_url":"https://127.0.0.1:9443","validation":{"state":"validated"}}]}"#,
        )
        .expect("registry write");

        let inventory = read_endpoint_inventory(&path).inventory;

        assert_eq!(inventory.endpoint_count, 1);
        assert_eq!(inventory.endpoints[0].display_name, "Endpoint A");

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    fn temp_root(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("dos-gui-{label}-{unique}"));
        fs::create_dir_all(&root).expect("temp root");
        root
    }
}
