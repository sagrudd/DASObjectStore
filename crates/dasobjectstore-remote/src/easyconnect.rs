use serde::{Deserialize, Serialize};
use std::fmt;

pub const DEFAULT_APPLIANCE_HTTPS_PORT: u16 = 8448;
pub const DEFAULT_LOCAL_CALLBACK_BIND: &str = "127.0.0.1:<ephemeral>";
pub const EASYCONNECT_DISCOVERY_PATH: &str =
    "/products/dasobjectstore/api/v1/remote/easyconnect/discovery";
pub const EASYCONNECT_PAIRING_PATH: &str =
    "/products/dasobjectstore/api/v1/remote/easyconnect/pairings";
pub const EASYCONNECT_LOGIN_PATH: &str = "/products/dasobjectstore/remote/easyconnect/login";
pub const EASYCONNECT_CALLBACK_PATH: &str = "/products/dasobjectstore/remote/easyconnect/callback";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemoteEasyconnectContractRequest {
    pub host_or_ip: String,
    pub https_port: u16,
    pub callback_port: Option<u16>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemoteEasyconnectContract {
    pub host_or_ip: String,
    pub appliance_base_url: String,
    pub discovery_url: String,
    pub pairing_create_url: String,
    pub browser_login_url: String,
    pub local_callback_bind: String,
    pub local_callback_url: String,
    pub polling_url_template: String,
    pub lifecycle: Vec<RemoteEasyconnectLifecycleStep>,
    pub failure_states: Vec<RemoteEasyconnectFailureState>,
    pub cli_output: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemoteEasyconnectLifecycleStep {
    pub state: String,
    pub actor: String,
    pub message: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemoteEasyconnectFailureState {
    pub code: String,
    pub message: String,
    pub retryable: bool,
}

pub fn define_easyconnect_contract(
    request: RemoteEasyconnectContractRequest,
) -> Result<RemoteEasyconnectContract, RemoteEasyconnectContractError> {
    let host = normalize_host(&request.host_or_ip)?;
    if request.https_port == 0 {
        return Err(RemoteEasyconnectContractError::InvalidHttpsPort);
    }
    let appliance_base_url = format!("https://{}:{}", host, request.https_port);
    let discovery_url = format!("{appliance_base_url}{EASYCONNECT_DISCOVERY_PATH}");
    let pairing_create_url = format!("{appliance_base_url}{EASYCONNECT_PAIRING_PATH}");
    let local_callback_bind = request
        .callback_port
        .map(|port| format!("127.0.0.1:{port}"))
        .unwrap_or_else(|| DEFAULT_LOCAL_CALLBACK_BIND.to_string());
    let local_callback_url = request
        .callback_port
        .map(|port| format!("http://127.0.0.1:{port}{EASYCONNECT_CALLBACK_PATH}"))
        .unwrap_or_else(|| {
            format!("http://{DEFAULT_LOCAL_CALLBACK_BIND}{EASYCONNECT_CALLBACK_PATH}")
        });
    let browser_login_url = format!(
        "{appliance_base_url}{EASYCONNECT_LOGIN_PATH}?callback={}",
        url_component(&local_callback_url)
    );
    let polling_url_template = format!("{pairing_create_url}/{{pairing_id}}");

    let lifecycle = easyconnect_lifecycle();
    let failure_states = easyconnect_failure_states();
    let cli_output = vec![
        "discover appliance pairing capabilities".to_string(),
        "start a loopback callback listener, or poll when callback binding is unavailable"
            .to_string(),
        "open the browser login URL without printing passwords or S3 credentials".to_string(),
        "wait for browser-approved pairing and exchange it for a remote upload session".to_string(),
        "persist only the issued remote session reference and non-secret appliance metadata"
            .to_string(),
    ];

    Ok(RemoteEasyconnectContract {
        host_or_ip: host,
        appliance_base_url,
        discovery_url,
        pairing_create_url,
        browser_login_url,
        local_callback_bind,
        local_callback_url,
        polling_url_template,
        lifecycle,
        failure_states,
        cli_output,
    })
}

fn easyconnect_lifecycle() -> Vec<RemoteEasyconnectLifecycleStep> {
    [
        (
            "discovery",
            "remote_cli",
            "Resolve HTTPS base URL and fetch appliance easyconnect capabilities.",
        ),
        (
            "local_callback",
            "remote_cli",
            "Bind a loopback callback listener; if unavailable, switch to bounded polling.",
        ),
        (
            "browser_login",
            "browser",
            "Open the appliance login and pairing approval page.",
        ),
        (
            "pairing_pending",
            "appliance",
            "Hold a short-lived pairing challenge until authenticated approval.",
        ),
        (
            "session_exchange",
            "remote_cli",
            "Exchange approved pairing for a remote upload session and accessible-store list.",
        ),
        (
            "ready",
            "remote_cli",
            "Store non-secret session metadata and use daemon-issued routing for uploads.",
        ),
    ]
    .into_iter()
    .map(|(state, actor, message)| RemoteEasyconnectLifecycleStep {
        state: state.to_string(),
        actor: actor.to_string(),
        message: message.to_string(),
    })
    .collect()
}

fn easyconnect_failure_states() -> Vec<RemoteEasyconnectFailureState> {
    [
        (
            "discovery_unreachable",
            "The remote client could not reach the appliance HTTPS discovery URL.",
            true,
        ),
        (
            "tls_or_identity_untrusted",
            "The appliance identity could not be trusted by the remote client.",
            false,
        ),
        (
            "callback_bind_failed",
            "The local callback listener could not bind; the client should use polling fallback.",
            true,
        ),
        (
            "browser_launch_failed",
            "The browser could not be opened automatically; print the login URL for manual opening.",
            true,
        ),
        (
            "login_denied",
            "The authenticated appliance login denied the pairing request.",
            false,
        ),
        (
            "pairing_expired",
            "The pairing challenge expired before approval.",
            true,
        ),
        (
            "session_exchange_denied",
            "The approved pairing could not be exchanged for a remote upload session.",
            false,
        ),
        (
            "agent_disconnected",
            "The browser-approved flow completed but the local remote agent was no longer reachable.",
            true,
        ),
    ]
    .into_iter()
    .map(|(code, message, retryable)| RemoteEasyconnectFailureState {
        code: code.to_string(),
        message: message.to_string(),
        retryable,
    })
    .collect()
}

fn normalize_host(value: &str) -> Result<String, RemoteEasyconnectContractError> {
    let host = value.trim().trim_end_matches('/');
    if host.is_empty() {
        return Err(RemoteEasyconnectContractError::BlankHost);
    }
    if host.contains("://") {
        return Err(RemoteEasyconnectContractError::HostIncludesScheme);
    }
    Ok(host.to_string())
}

fn url_component(value: &str) -> String {
    value
        .bytes()
        .flat_map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                vec![byte as char]
            }
            _ => format!("%{byte:02X}").chars().collect(),
        })
        .collect()
}

#[derive(Debug, Eq, PartialEq)]
pub enum RemoteEasyconnectContractError {
    BlankHost,
    HostIncludesScheme,
    InvalidHttpsPort,
}

impl fmt::Display for RemoteEasyconnectContractError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BlankHost => formatter.write_str("easyconnect host must not be blank"),
            Self::HostIncludesScheme => formatter.write_str(
                "easyconnect host must be a host name or IP address, not a URL with a scheme",
            ),
            Self::InvalidHttpsPort => formatter.write_str("easyconnect HTTPS port must not be 0"),
        }
    }
}

impl std::error::Error for RemoteEasyconnectContractError {}

#[cfg(test)]
mod tests {
    use super::{
        define_easyconnect_contract, RemoteEasyconnectContractError,
        RemoteEasyconnectContractRequest, DEFAULT_APPLIANCE_HTTPS_PORT,
    };

    #[test]
    fn defines_easyconnect_urls_lifecycle_and_failure_contract() {
        let contract = define_easyconnect_contract(RemoteEasyconnectContractRequest {
            host_or_ip: "192.168.1.192".to_string(),
            https_port: DEFAULT_APPLIANCE_HTTPS_PORT,
            callback_port: Some(49321),
        })
        .expect("contract builds");

        assert_eq!(contract.appliance_base_url, "https://192.168.1.192:8448");
        assert_eq!(
            contract.discovery_url,
            "https://192.168.1.192:8448/products/dasobjectstore/api/v1/remote/easyconnect/discovery"
        );
        assert_eq!(contract.local_callback_bind, "127.0.0.1:49321");
        assert!(contract
            .browser_login_url
            .contains("callback=http%3A%2F%2F127.0.0.1%3A49321%2Fproducts%2Fdasobjectstore%2Fremote%2Feasyconnect%2Fcallback"));
        assert!(contract
            .lifecycle
            .iter()
            .any(|step| step.state == "session_exchange"));
        assert!(contract
            .failure_states
            .iter()
            .any(|failure| failure.code == "pairing_expired" && failure.retryable));
        assert!(contract
            .cli_output
            .iter()
            .any(|line| line.contains("without printing passwords")));
    }

    #[test]
    fn rejects_url_scheme_in_easyconnect_host() {
        let err = define_easyconnect_contract(RemoteEasyconnectContractRequest {
            host_or_ip: "https://192.168.1.192".to_string(),
            https_port: DEFAULT_APPLIANCE_HTTPS_PORT,
            callback_port: None,
        })
        .expect_err("URL host rejected");

        assert_eq!(err, RemoteEasyconnectContractError::HostIncludesScheme);
    }
}
