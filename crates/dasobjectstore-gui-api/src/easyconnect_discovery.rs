//! EasyConnect discovery at standalone and Pistis public boundaries.

use super::*;

pub(super) async fn easyconnect_discovery(
    State(state): State<StandaloneEasyconnectRouteState>,
) -> Json<RemoteEasyconnectDiscoveryResponse> {
    Json(standalone_easyconnect_discovery_payload(
        &state.public_base_url,
        &state.appliance_id,
    ))
}

pub(super) async fn pistis_easyconnect_discovery(
    State(state): State<EasyconnectPublicRouteState>,
) -> Result<Json<RemoteEasyconnectDiscoveryResponse>, (StatusCode, Json<AuthRouteError>)> {
    let public_base_url = state.public_base_url.ok_or_else(|| {
        route_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "easyconnect_public_origin_unavailable",
            "the appliance has not configured a valid authoritative public HTTPS origin",
        )
    })?;
    let mut discovery =
        standalone_easyconnect_discovery_payload(&public_base_url, &state.appliance_id);
    discovery.display_name = "DASObjectStore through Pistis".to_string();
    discovery.auth_providers = vec![RemoteEasyconnectAuthProvider::Pistis];
    Ok(Json(discovery))
}

pub(super) fn standalone_easyconnect_discovery_payload(
    public_base_url: &str,
    appliance_id: &str,
) -> RemoteEasyconnectDiscoveryResponse {
    let api_base_url = format!(
        "{}/products/dasobjectstore/api",
        public_base_url.trim_end_matches('/')
    );

    RemoteEasyconnectDiscoveryResponse {
        appliance_id: appliance_id.to_string(),
        product_id: "dasobjectstore".to_string(),
        display_name: "DASObjectStore standalone appliance".to_string(),
        pairing_create_url: format!("{api_base_url}/v1/remote/easyconnect/pairings"),
        pairing_exchange_url: format!("{api_base_url}/v1/remote/easyconnect/pairings/exchange"),
        session_revoke_url_template: format!(
            "{api_base_url}/v1/remote/easyconnect/sessions/{{session_id}}"
        ),
        session_renew_url_template: format!(
            "{api_base_url}/v1/remote/easyconnect/sessions/{{session_id}}/renew"
        ),
        default_session_lifetime_seconds:
            dasobjectstore_daemon::REMOTE_EASYCONNECT_DEFAULT_SESSION_LIFETIME_SECONDS,
        session_policy: RemoteEasyconnectSessionPolicy::default(),
        auth_providers: vec![RemoteEasyconnectAuthProvider::StandaloneLocalUser],
        descriptor_schema_version: "dasobjectstore.remote_descriptor.v1".to_string(),
        server_version: dasobjectstore_core::VERSION.to_string(),
        api_schema_versions: vec![
            "dasobjectstore.remote_auth.v1".to_string(),
            "dasobjectstore.remote_control.v1".to_string(),
            "dasobjectstore.remote_config.v2".to_string(),
        ],
        capabilities: vec![
            "remote_resync_v1".to_string(),
            "authoritative_s3_endpoint_v1".to_string(),
            "stable_appliance_identity".to_string(),
            "trust_repair_v1".to_string(),
            "temporary_s3_session".to_string(),
            "store_readiness".to_string(),
        ],
        remote_client_protocol_min: dasobjectstore_daemon::REMOTE_CLIENT_PROTOCOL_MIN,
        remote_client_protocol_max: dasobjectstore_daemon::REMOTE_CLIENT_PROTOCOL_MAX,
        component_builds: dasobjectstore_daemon::RemoteEasyconnectComponentBuilds {
            server: dasobjectstore_core::VERSION.to_string(),
            daemon: dasobjectstore_core::VERSION.to_string(),
            s3_gateway: dasobjectstore_core::VERSION.to_string(),
        },
    }
}
