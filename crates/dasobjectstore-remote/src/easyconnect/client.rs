use super::*;
use dasobjectstore_daemon::{
    RemoteEasyconnectCreatePairingRequest, RemoteEasyconnectCreatePairingResponse,
    RemoteEasyconnectDiscoveryResponse, RemoteEasyconnectExchangeConnectionResponse,
    RemoteEasyconnectExchangePairingRequest, RemoteEasyconnectPairingState,
    RemoteEasyconnectPairingStatusResponse,
};
use reqwest::blocking::Client;
use serde::Serialize;
use std::net::ToSocketAddrs;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteEasyconnectCompletedPairing {
    pub https_port: u16,
    pub contract: RemoteEasyconnectContract,
    pub discovery: RemoteEasyconnectDiscoveryResponse,
    pub pairing: RemoteEasyconnectCreatePairingResponse,
    pub exchange: RemoteEasyconnectExchangeConnectionResponse,
}

pub fn run_complete_easyconnect_pairing_with_ready<F>(
    options: RemoteEasyconnectPairingOptions,
    launcher: &impl BrowserLauncher,
    ready: F,
) -> Result<RemoteEasyconnectCompletedPairing, RemoteEasyconnectPairingError>
where
    F: FnOnce(
        &RemoteEasyconnectContract,
        &RemoteEasyconnectCreatePairingResponse,
    ) -> Result<(), RemoteEasyconnectPairingError>,
{
    let bind_address = options
        .callback_port
        .map(|port| format!("127.0.0.1:{port}"))
        .unwrap_or_else(|| "127.0.0.1:0".to_string());
    let listener = TcpListener::bind(&bind_address).map_err(|source| {
        RemoteEasyconnectPairingError::CallbackBindFailed {
            address: bind_address,
            source,
        }
    })?;
    let callback_port = listener.local_addr()?.port();
    let contract = define_easyconnect_contract(RemoteEasyconnectContractRequest {
        host_or_ip: options.host_or_ip,
        https_port: options.https_port,
        callback_port: Some(callback_port),
    })?;
    let requested_object_store = options.requested_object_store.clone();
    let transport = pinned_https_client(&contract.host_or_ip, options.https_port)?;
    let discovery_url = transport_url(
        &contract.appliance_base_url,
        &transport.base_url,
        &contract.discovery_url,
        "discovery_url",
    )?;
    let discovery =
        get_json::<RemoteEasyconnectDiscoveryResponse>(&transport.client, &discovery_url)?;
    if !discovery
        .auth_providers
        .contains(&dasobjectstore_daemon::RemoteEasyconnectAuthProvider::Pistis)
    {
        return Err(RemoteEasyconnectPairingError::Protocol(
            "appliance does not advertise a Pistis EasyConnect authority; no pairing was created"
                .to_string(),
        ));
    }
    validate_server_url(
        &contract.appliance_base_url,
        &discovery.pairing_create_url,
        "pairing_create_url",
    )?;
    validate_server_url(
        &contract.appliance_base_url,
        &discovery.pairing_exchange_url,
        "pairing_exchange_url",
    )?;
    let pairing = post_json::<_, RemoteEasyconnectCreatePairingResponse>(
        &transport.client,
        &transport_url(
            &contract.appliance_base_url,
            &transport.base_url,
            &discovery.pairing_create_url,
            "pairing_create_url",
        )?,
        &RemoteEasyconnectCreatePairingRequest {
            client_name: "dasobjectstore-remote".to_string(),
            callback_url: contract.local_callback_url.clone(),
            requested_object_store,
            requested_session_lifetime_seconds: Some(
                discovery.session_policy.default_lifetime_seconds,
            ),
            client_request_id: None,
        },
    )?;
    validate_server_url(
        &contract.appliance_base_url,
        &pairing.browser_login_url,
        "browser_login_url",
    )?;
    if pairing.callback_url != contract.local_callback_url {
        return Err(RemoteEasyconnectPairingError::Protocol(
            "appliance changed the exact loopback callback URL".to_string(),
        ));
    }
    ready(&contract, &pairing)?;
    if options.open_browser {
        launcher.open(&pairing.browser_login_url)?;
    }
    let callback = wait_for_pairing_callback_or_poll(
        &listener,
        &transport.client,
        &transport_url(
            &contract.appliance_base_url,
            &transport.base_url,
            &pairing.polling_url,
            "polling_url",
        )?,
        &pairing.pairing_id,
        options.timeout,
    )?;
    if callback.pairing_id != pairing.pairing_id {
        return Err(RemoteEasyconnectPairingError::Protocol(
            "loopback callback pairing ID did not match the created pairing".to_string(),
        ));
    }
    let exchange = post_json::<_, RemoteEasyconnectExchangeConnectionResponse>(
        &transport.client,
        &transport_url(
            &contract.appliance_base_url,
            &transport.base_url,
            &discovery.pairing_exchange_url,
            "pairing_exchange_url",
        )?,
        &RemoteEasyconnectExchangePairingRequest {
            pairing_id: callback.pairing_id,
            exchange_code: callback.exchange_code,
            client_request_id: None,
        },
    )?;
    if exchange.exchange.appliance_id != discovery.appliance_id {
        return Err(RemoteEasyconnectPairingError::Protocol(
            "exchange response appliance identity did not match discovery".to_string(),
        ));
    }
    validate_exchange_envelope(&contract, &exchange)?;
    validate_requested_store(
        options.requested_object_store.as_deref(),
        &exchange.exchange.object_stores,
    )?;
    Ok(RemoteEasyconnectCompletedPairing {
        https_port: options.https_port,
        contract,
        discovery,
        pairing,
        exchange,
    })
}

fn wait_for_pairing_callback_or_poll(
    listener: &TcpListener,
    client: &Client,
    polling_url: &str,
    pairing_id: &str,
    timeout: Duration,
) -> Result<RemoteEasyconnectPairingResult, RemoteEasyconnectPairingError> {
    let callback_listener = listener.try_clone()?;
    let callback_pairing_id = pairing_id.to_string();
    let polling_pairing_id = pairing_id.to_string();
    let polling_url = polling_url.to_string();
    let polling_client = client.clone();
    let (sender, receiver) = std::sync::mpsc::channel();
    let callback_sender = sender.clone();
    std::thread::spawn(move || {
        let result = wait_for_pairing_callback(&callback_listener, timeout).and_then(|result| {
            if result.pairing_id == callback_pairing_id {
                Ok(result)
            } else {
                Err(RemoteEasyconnectPairingError::Protocol(
                    "loopback callback pairing ID did not match the created pairing".to_string(),
                ))
            }
        });
        let _ = callback_sender.send(result);
    });
    std::thread::spawn(move || {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            match get_json::<RemoteEasyconnectPairingStatusResponse>(&polling_client, &polling_url)
            {
                Ok(status)
                    if status.pairing_id == polling_pairing_id
                        && status.state == RemoteEasyconnectPairingState::Approved =>
                {
                    let result = status
                        .exchange_code
                        .filter(|code| !code.is_empty())
                        .map(|exchange_code| RemoteEasyconnectPairingResult {
                            pairing_id: polling_pairing_id.clone(),
                            exchange_code,
                        })
                        .ok_or_else(|| {
                            RemoteEasyconnectPairingError::Protocol(
                                "approved polling response omitted the exchange capability"
                                    .to_string(),
                            )
                        });
                    let _ = sender.send(result);
                    return;
                }
                Ok(status)
                    if matches!(
                        status.state,
                        RemoteEasyconnectPairingState::Expired
                            | RemoteEasyconnectPairingState::Exchanged
                    ) =>
                {
                    let _ = sender.send(Err(RemoteEasyconnectPairingError::Protocol(
                        "pairing became unusable before exchange".to_string(),
                    )));
                    return;
                }
                Ok(_) | Err(_) => std::thread::sleep(Duration::from_millis(250)),
            }
        }
        let _ = sender.send(Err(RemoteEasyconnectPairingError::PairingTimedOut));
    });
    receiver
        .recv_timeout(timeout + Duration::from_secs(1))
        .map_err(|_| RemoteEasyconnectPairingError::PairingTimedOut)?
}

pub(super) fn validate_requested_store(
    requested: Option<&str>,
    grants: &[dasobjectstore_daemon::RemoteEasyconnectObjectStoreGrant],
) -> Result<(), RemoteEasyconnectPairingError> {
    if requested.is_some_and(|requested| grants.len() != 1 || grants[0].object_store != requested) {
        return Err(RemoteEasyconnectPairingError::Protocol(
            "exchange grants did not preserve the exact requested ObjectStore".to_string(),
        ));
    }
    Ok(())
}

pub(super) fn validate_exchange_envelope(
    contract: &RemoteEasyconnectContract,
    response: &RemoteEasyconnectExchangeConnectionResponse,
) -> Result<(), RemoteEasyconnectPairingError> {
    if response.schema_version != "dasobjectstore.remote_easyconnect_exchange.v1" {
        return Err(RemoteEasyconnectPairingError::Protocol(
            "unsupported EasyConnect exchange schema".to_string(),
        ));
    }
    let exchange = &response.exchange;
    if exchange.auth_provider != dasobjectstore_daemon::RemoteEasyconnectAuthProvider::Pistis
        || exchange.appliance_id.trim().is_empty()
        || exchange.approved_actor.trim().is_empty()
        || exchange.session.session_id.trim().is_empty()
        || exchange.session.credentials.access_key_id.trim().is_empty()
        || exchange.session.credentials.secret_access_key.is_empty()
        || exchange
            .session
            .credentials
            .session_token
            .as_deref()
            .unwrap_or("")
            .is_empty()
        || exchange.session.expires_at_utc.trim().is_empty()
        || exchange.session.renewal.renewal_token.is_empty()
    {
        return Err(RemoteEasyconnectPairingError::Protocol(
            "exchange identity or session was incomplete".to_string(),
        ));
    }
    let issued = dasobjectstore_core::utc::parse_canonical_utc_timestamp_seconds(
        &exchange.session.issued_at_utc,
    );
    let expires = dasobjectstore_core::utc::parse_canonical_utc_timestamp_seconds(
        &exchange.session.expires_at_utc,
    );
    let renew_after = dasobjectstore_core::utc::parse_canonical_utc_timestamp_seconds(
        &exchange.session.renewal.renew_after_utc,
    );
    if !matches!(
        (issued, renew_after, expires),
        (Some(issued), Some(renew_after), Some(expires))
            if issued < renew_after && renew_after < expires
    ) {
        return Err(RemoteEasyconnectPairingError::Protocol(
            "exchange returned invalid canonical session timestamps".to_string(),
        ));
    }
    if exchange.appliance_base_url != "/products/dasobjectstore/api" {
        return Err(RemoteEasyconnectPairingError::Protocol(
            "exchange returned an unexpected internal appliance API base".to_string(),
        ));
    }
    let renew = &exchange.session.renewal.renew_url;
    if !renew.starts_with("/api/v1/remote/easyconnect/sessions/")
        || !renew.ends_with("/renew")
        || renew.contains('?')
        || renew.contains('#')
    {
        return Err(RemoteEasyconnectPairingError::Protocol(
            "exchange returned an unsafe session renewal route".to_string(),
        ));
    }
    if exchange.object_stores.is_empty() {
        return Err(RemoteEasyconnectPairingError::Protocol(
            "exchange granted no ObjectStores".to_string(),
        ));
    }
    let mut stores = std::collections::HashSet::new();
    for grant in &exchange.object_stores {
        if grant.object_store.trim().is_empty()
            || grant.bucket.trim().is_empty()
            || !stores.insert(grant.object_store.as_str())
        {
            return Err(RemoteEasyconnectPairingError::Protocol(
                "exchange contained blank or duplicate ObjectStore grants".to_string(),
            ));
        }
    }
    let endpoint = reqwest::Url::parse(&response.s3.endpoint_url).map_err(|_| {
        RemoteEasyconnectPairingError::Protocol(
            "exchange returned a malformed public S3 endpoint".to_string(),
        )
    })?;
    if endpoint.scheme() != "https"
        || endpoint.host_str().is_none()
        || !endpoint.username().is_empty()
        || endpoint.password().is_some()
        || endpoint.query().is_some()
        || endpoint.fragment().is_some()
        || endpoint.path() != "/"
        || response.s3.region.trim().is_empty()
        || !matches!(response.s3.addressing_style.as_str(), "path" | "virtual")
    {
        return Err(RemoteEasyconnectPairingError::Protocol(
            "exchange returned an unsupported public S3 connection descriptor".to_string(),
        ));
    }
    validate_server_url(
        &contract.appliance_base_url,
        &contract.discovery_url,
        "discovery_url",
    )
}

struct PinnedHttpsClient {
    client: Client,
    base_url: String,
}

fn pinned_https_client(
    host: &str,
    port: u16,
) -> Result<PinnedHttpsClient, RemoteEasyconnectPairingError> {
    let trust = crate::trust::load_trust(host, port)
        .map_err(|error| RemoteEasyconnectPairingError::Trust(error.to_string()))?
        .ok_or_else(|| {
            RemoteEasyconnectPairingError::Trust(format!(
                "no enrolled certificate exists for {host}:{port}; run `dasobjectstore-remote trust inspect {host}` and complete certificate enrollment first"
            ))
        })?;
    let socket = format!("{host}:{port}")
        .to_socket_addrs()?
        .next()
        .ok_or_else(|| {
            RemoteEasyconnectPairingError::Transport(
                "appliance host resolved to no address".to_string(),
            )
        })?;
    let certificate = reqwest::Certificate::from_pem(trust.certificate_pem.as_bytes())
        .map_err(|error| RemoteEasyconnectPairingError::Trust(error.to_string()))?;
    let client = Client::builder()
        .timeout(Duration::from_secs(20))
        .resolve(&trust.tls_server_name, socket)
        .add_root_certificate(certificate)
        .build()
        .map_err(|error| RemoteEasyconnectPairingError::Transport(error.to_string()))?;
    Ok(PinnedHttpsClient {
        client,
        base_url: format!("https://{}:{port}", trust.tls_server_name),
    })
}

fn get_json<T: serde::de::DeserializeOwned>(
    client: &Client,
    url: &str,
) -> Result<T, RemoteEasyconnectPairingError> {
    decode_response(client.get(url).send())
}

fn post_json<B: Serialize, T: serde::de::DeserializeOwned>(
    client: &Client,
    url: &str,
    body: &B,
) -> Result<T, RemoteEasyconnectPairingError> {
    decode_response(client.post(url).json(body).send())
}

fn decode_response<T: serde::de::DeserializeOwned>(
    response: Result<reqwest::blocking::Response, reqwest::Error>,
) -> Result<T, RemoteEasyconnectPairingError> {
    let response =
        response.map_err(|error| RemoteEasyconnectPairingError::Transport(error.to_string()))?;
    let status = response.status();
    if !status.is_success() {
        return Err(RemoteEasyconnectPairingError::Server(status.as_u16()));
    }
    response
        .json()
        .map_err(|error| RemoteEasyconnectPairingError::Protocol(error.to_string()))
}

pub(super) fn validate_server_url(
    appliance_base_url: &str,
    value: &str,
    field: &str,
) -> Result<(), RemoteEasyconnectPairingError> {
    let base = reqwest::Url::parse(appliance_base_url)
        .map_err(|error| RemoteEasyconnectPairingError::Protocol(error.to_string()))?;
    let url = reqwest::Url::parse(value).map_err(|_| {
        RemoteEasyconnectPairingError::Protocol(format!("{field} was not an absolute URL"))
    })?;
    if url.scheme() != "https"
        || url.host_str() != base.host_str()
        || url.port_or_known_default() != base.port_or_known_default()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err(RemoteEasyconnectPairingError::Protocol(format!(
            "{field} escaped the pinned appliance HTTPS origin"
        )));
    }
    Ok(())
}

pub(super) fn transport_url(
    appliance_base_url: &str,
    transport_base_url: &str,
    value: &str,
    field: &str,
) -> Result<String, RemoteEasyconnectPairingError> {
    validate_server_url(appliance_base_url, value, field)?;
    let logical = reqwest::Url::parse(value)
        .map_err(|error| RemoteEasyconnectPairingError::Protocol(error.to_string()))?;
    let transport = reqwest::Url::parse(transport_base_url)
        .map_err(|error| RemoteEasyconnectPairingError::Protocol(error.to_string()))?;
    let mut rewritten = transport;
    rewritten.set_path(logical.path());
    rewritten.set_query(logical.query());
    Ok(rewritten.to_string())
}
