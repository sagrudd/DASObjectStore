#![forbid(unsafe_code)]

use dasobjectstore_gui_api::StandaloneServerConfig;

const CONFIG: &str =
    include_str!("../../../deploy/evaluation/dasobjectstore-pistis-evaluation.json.example");
const SERVICE: &str =
    include_str!("../../../deploy/evaluation/dasobjectstore-pistis-evaluation.service");

#[test]
fn parallel_evaluation_configuration_is_valid_and_tls_only() {
    let rendered = CONFIG
        .replace("REPLACE_HOSTNAME", "pistis-evaluation.example")
        .replace("REPLACE_USER", "operator");
    let config: StandaloneServerConfig = serde_json::from_str(&rendered).unwrap();
    config.validate().unwrap();
    assert_eq!(config.bind_address, "127.0.0.1");
    assert_eq!(config.https_port, 8740);
    assert_eq!(config.s3_ingress.port, 3943);
    assert_eq!(
        config.s3_ingress.public_endpoint_url.as_deref(),
        Some("https://pistis-evaluation.example:3943")
    );
    assert_eq!(
        config.s3_ingress.legacy_upstream_endpoint,
        "http://127.0.0.1:3901"
    );
}

#[test]
fn parallel_service_is_attended_static_and_cannot_replace_live_ports() {
    assert!(SERVICE.contains("After=network-online.target dasobjectstored.service"));
    assert!(SERVICE.contains("Restart=no"));
    assert!(SERVICE.contains("NoNewPrivileges=true"));
    assert!(SERVICE.contains("CapabilityBoundingSet="));
    assert!(SERVICE.contains("ProtectSystem=strict"));
    assert!(SERVICE.contains("ProtectHome=read-only"));
    assert!(!SERVICE.contains("\n[Install]\n"));
    assert!(!SERVICE.contains("WantedBy="));
    assert!(!CONFIG.contains("\"https_port\": 8448"));
    assert!(!CONFIG.contains("\"port\": 3900"));
}
