use super::{
    RemoteConfig, RemoteConfigOverrides, RemoteObjectStoreGrant, RemotePairedAppliance,
    RemoteSessionCredentials, RemoteSessionRenewalMetadata, RemoteUploadSession, REDACTED_SECRET,
    REMOTE_CONFIG_SCHEMA_VERSION,
};
use crate::auth::RemoteAuthAuthority;

#[test]
fn overrides_config_without_losing_unset_values() {
    let config = RemoteConfig {
            schema_version: REMOTE_CONFIG_SCHEMA_VERSION.to_string(),
            generation: 1,
            endpoint_url: "http://old:3900".to_string(),
            region: "garage".to_string(),
            profile: "old".to_string(),
            auth_authority: RemoteAuthAuthority::Mneion,
            username: Some("alice".to_string()),
            credential_helper: Some("helper".to_string()),
            default_appliance_id: Some("appliance-1".to_string()),
            paired_appliances: vec![RemotePairedAppliance {
                appliance_id: "appliance-1".to_string(),
                display_name: "Lab DAS".to_string(),
                appliance_base_url: "https://192.168.1.192:8448".to_string(),
                discovery_url:
                    "https://192.168.1.192:8448/products/dasobjectstore/api/v1/remote/easyconnect/discovery"
                        .to_string(),
                auth_authority: RemoteAuthAuthority::Pistis,
                paired_actor: Some("alice".to_string()),
                default_object_store: Some("generated-data".to_string()),
                session: None,
                object_stores: Vec::new(),
            }],
            s3_profiles: Vec::new(),
            session_bindings: Vec::new(),
        };

    let merged = config.merged_with(RemoteConfigOverrides {
        endpoint_url: Some("https://new:3900"),
        profile: Some("new"),
        ..RemoteConfigOverrides::default()
    });

    assert_eq!(merged.endpoint_url, "https://new:3900");
    assert_eq!(merged.region, "garage");
    assert_eq!(merged.profile, "new");
    assert_eq!(merged.username.as_deref(), Some("alice"));
    assert_eq!(merged.credential_helper.as_deref(), Some("helper"));
    assert_eq!(merged.default_appliance_id.as_deref(), Some("appliance-1"));
    assert_eq!(merged.paired_appliances.len(), 1);
}

#[test]
fn reads_legacy_config_without_pairing_fields() {
    let raw = r#"{
          "endpoint_url": "http://192.168.1.192:3900",
          "region": "garage",
          "profile": "dasobjectstore"
        }"#;

    let config: RemoteConfig = serde_json::from_str(raw).expect("legacy config parses");

    assert_eq!(config.endpoint_url, "http://192.168.1.192:3900");
    assert!(config.default_appliance_id.is_none());
    assert!(config.paired_appliances.is_empty());
}

#[test]
fn rejects_persisted_local_password_authority_without_fallback() {
    let config: RemoteConfig = serde_json::from_str(
        r#"{
          "endpoint_url": "https://dos.example:3900",
          "auth_authority": "local-password"
        }"#,
    )
    .expect("legacy config remains readable for remediation");

    let error = config
        .validate_for_command()
        .expect_err("legacy local-password configuration must fail closed");
    assert!(error
        .to_string()
        .contains("local-password authority is retired"));
}

#[test]
fn redacts_session_credentials_for_display() {
    let config = RemoteConfig {
            schema_version: REMOTE_CONFIG_SCHEMA_VERSION.to_string(),
            generation: 1,
            endpoint_url: "https://192.168.1.192:3900".to_string(),
            region: "garage".to_string(),
            profile: "dasobjectstore".to_string(),
            auth_authority: RemoteAuthAuthority::Pistis,
            username: Some("stephen".to_string()),
            credential_helper: Some("helper".to_string()),
            default_appliance_id: Some("appliance-1".to_string()),
            paired_appliances: vec![RemotePairedAppliance {
                appliance_id: "appliance-1".to_string(),
                display_name: "QNAP TL-D800C".to_string(),
                appliance_base_url: "https://192.168.1.192:8448".to_string(),
                discovery_url:
                    "https://192.168.1.192:8448/products/dasobjectstore/api/v1/remote/easyconnect/discovery"
                        .to_string(),
                auth_authority: RemoteAuthAuthority::Pistis,
                paired_actor: Some("stephen".to_string()),
                default_object_store: Some("zymo_fecal_2025.05".to_string()),
                object_stores: vec![RemoteObjectStoreGrant {
                    object_store: "zymo_fecal_2025.05".to_string(),
                    bucket: "dos-zymo-fecal-2025-05".to_string(),
                    can_read: true,
                    can_write: true,
                    writer_group: Some("mnemosyne".to_string()),
                    object_type: "metagenomics".to_string(),
                }],
                session: Some(RemoteUploadSession {
                    session_id: "SESSIONREFERENCE7890".to_string(),
                    issued_at: "2026-07-09T11:30:00Z".to_string(),
                    expires_at: "2026-07-09T19:30:00Z".to_string(),
                    credentials: RemoteSessionCredentials {
                        access_key_id: "DOSREMOTEACCESSKEY1234".to_string(),
                        secret_access_key: "super-secret".to_string(),
                        session_token: Some("temporary-token".to_string()),
                    },
                    renewal: Some(RemoteSessionRenewalMetadata {
                        renew_url: "https://192.168.1.192:8448/api/renew".to_string(),
                        renew_after: "2026-07-09T18:30:00Z".to_string(),
                        renewal_token: Some("renewal-token-secret".to_string()),
                        last_renewed_at: None,
                    }),
                }),
            }],
            s3_profiles: Vec::new(),
            session_bindings: Vec::new(),
        };

    let redacted = config.redacted();
    let rendered = serde_json::to_string(&redacted).expect("redacted config serializes");

    assert!(rendered.contains("DOSR...1234"));
    assert!(rendered.contains("SESS...7890"));
    assert!(rendered.contains(REDACTED_SECRET));
    assert!(rendered.contains("zymo_fecal_2025.05"));
    assert!(rendered.contains("dos-zymo-fecal-2025-05"));
    assert!(!rendered.contains("SESSIONREFERENCE7890"));
    assert!(!rendered.contains("super-secret"));
    assert!(!rendered.contains("temporary-token"));
    assert!(!rendered.contains("renewal-token-secret"));
}
