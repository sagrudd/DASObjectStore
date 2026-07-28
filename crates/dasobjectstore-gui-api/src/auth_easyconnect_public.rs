use super::*;

pub(super) fn absolute_public_easyconnect_url(
    public_base_url: &str,
    daemon_route: &str,
) -> Option<reqwest::Url> {
    if !daemon_route.starts_with('/') || daemon_route.starts_with("//") {
        return None;
    }
    let origin = reqwest::Url::parse(public_base_url).ok()?;
    let mounted_route = if daemon_route.starts_with("/products/dasobjectstore/") {
        daemon_route.to_string()
    } else {
        format!("/products/dasobjectstore{daemon_route}")
    };
    let candidate = origin.join(&mounted_route).ok()?;
    if candidate.origin() != origin.origin()
        || !candidate.username().is_empty()
        || candidate.password().is_some()
        || candidate.fragment().is_some()
    {
        return None;
    }
    Some(candidate)
}

pub(super) fn standalone_easyconnect_discovery_payload(
    public_base_url: &str,
) -> RemoteEasyconnectDiscoveryResponse {
    let api_base_url = format!(
        "{}/products/dasobjectstore/api",
        public_base_url.trim_end_matches('/')
    );

    RemoteEasyconnectDiscoveryResponse {
        appliance_id: "standalone-dasobjectstore".to_string(),
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
    }
}

#[cfg(test)]
mod public_origin_tests {
    use super::absolute_public_easyconnect_url;

    #[test]
    fn mounts_daemon_routes_exactly_once_on_the_configured_public_origin() {
        let origin = "https://das.customer.example:8448";
        for (daemon_route, expected) in [
            (
                "/products/dasobjectstore/remote/easyconnect/login?pairing_id=pair-1",
                "https://das.customer.example:8448/products/dasobjectstore/remote/easyconnect/login?pairing_id=pair-1",
            ),
            (
                "/api/v1/remote/easyconnect/pairings/pair-1?wait=1",
                "https://das.customer.example:8448/products/dasobjectstore/api/v1/remote/easyconnect/pairings/pair-1?wait=1",
            ),
        ] {
            let route =
                absolute_public_easyconnect_url(origin, daemon_route).expect("relative daemon route");
            assert_eq!(route.as_str(), expected);
            assert_eq!(route.origin().ascii_serialization(), origin);
        }
    }

    #[test]
    fn rejects_cross_origin_or_fragmented_daemon_routes() {
        let origin = "https://das.customer.example:8448";
        for substituted in [
            "https://attacker.example/remote/easyconnect/login",
            "//attacker.example/remote/easyconnect/login",
            "/remote/easyconnect/login#fragment",
        ] {
            assert!(absolute_public_easyconnect_url(origin, substituted).is_none());
        }
    }
}
