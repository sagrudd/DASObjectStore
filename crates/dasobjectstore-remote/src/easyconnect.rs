use serde::{Deserialize, Serialize};
use std::fmt;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::Command;
use std::time::{Duration, Instant};

pub const DEFAULT_APPLIANCE_HTTPS_PORT: u16 = 8448;
pub const DEFAULT_PAIRING_TIMEOUT_SECS: u64 = 300;
pub const DEFAULT_REMOTE_SESSION_LIFETIME_SECS: u64 = 8 * 60 * 60;
pub const DEFAULT_REMOTE_SESSION_RENEWAL_LEAD_SECS: u64 = 60 * 60;
pub const DEFAULT_LOCAL_CALLBACK_BIND: &str = "127.0.0.1:<ephemeral>";
pub const EASYCONNECT_DISCOVERY_PATH: &str =
    "/products/dasobjectstore/api/v1/remote/easyconnect/discovery";
pub const EASYCONNECT_PAIRING_PATH: &str =
    "/products/dasobjectstore/api/v1/remote/easyconnect/pairings";
pub const EASYCONNECT_LOGIN_PATH: &str = "/products/dasobjectstore/remote/easyconnect/login";
pub const EASYCONNECT_CALLBACK_PATH: &str = "/products/dasobjectstore/remote/easyconnect/callback";

pub trait BrowserLauncher {
    fn open(&self, url: &str) -> Result<(), RemoteEasyconnectPairingError>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SystemBrowserLauncher;

impl BrowserLauncher for SystemBrowserLauncher {
    fn open(&self, url: &str) -> Result<(), RemoteEasyconnectPairingError> {
        let status = browser_open_command(url).status()?;
        if status.success() {
            Ok(())
        } else {
            Err(RemoteEasyconnectPairingError::BrowserLaunchFailed)
        }
    }
}

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
    pub default_session_lifetime_seconds: u64,
    pub session_renewal_lead_seconds: u64,
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteEasyconnectPairingOptions {
    pub host_or_ip: String,
    pub https_port: u16,
    pub callback_port: Option<u16>,
    pub timeout: Duration,
    pub open_browser: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteEasyconnectPairingOutcome {
    pub contract: RemoteEasyconnectContract,
    pub result: RemoteEasyconnectPairingResult,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteEasyconnectPairingResult {
    pub pairing_id: String,
    pub exchange_code: String,
}

impl RemoteEasyconnectPairingResult {
    pub fn redacted_exchange_code(&self) -> &'static str {
        "<redacted>"
    }
}

pub fn run_easyconnect_pairing(
    options: RemoteEasyconnectPairingOptions,
    launcher: &impl BrowserLauncher,
) -> Result<RemoteEasyconnectPairingOutcome, RemoteEasyconnectPairingError> {
    run_easyconnect_pairing_with_ready(options, launcher, |_| Ok(()))
}

pub fn run_easyconnect_pairing_with_ready<F>(
    options: RemoteEasyconnectPairingOptions,
    launcher: &impl BrowserLauncher,
    ready: F,
) -> Result<RemoteEasyconnectPairingOutcome, RemoteEasyconnectPairingError>
where
    F: FnOnce(&RemoteEasyconnectContract) -> Result<(), RemoteEasyconnectPairingError>,
{
    let bind_address = options
        .callback_port
        .map(|port| format!("127.0.0.1:{port}"))
        .unwrap_or_else(|| "127.0.0.1:0".to_string());
    let listener = TcpListener::bind(&bind_address).map_err(|error| {
        RemoteEasyconnectPairingError::CallbackBindFailed {
            address: bind_address.clone(),
            source: error,
        }
    })?;
    let callback_port = listener.local_addr()?.port();
    let contract = define_easyconnect_contract(RemoteEasyconnectContractRequest {
        host_or_ip: options.host_or_ip,
        https_port: options.https_port,
        callback_port: Some(callback_port),
    })?;
    ready(&contract)?;
    if options.open_browser {
        launcher.open(&contract.browser_login_url)?;
    }
    let result = wait_for_pairing_callback(&listener, options.timeout)?;
    Ok(RemoteEasyconnectPairingOutcome { contract, result })
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
        "renew active upload sessions with daemon-issued renewal tokens, not stored passwords"
            .to_string(),
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
        default_session_lifetime_seconds: DEFAULT_REMOTE_SESSION_LIFETIME_SECS,
        session_renewal_lead_seconds: DEFAULT_REMOTE_SESSION_RENEWAL_LEAD_SECS,
        lifecycle,
        failure_states,
        cli_output,
    })
}

fn wait_for_pairing_callback(
    listener: &TcpListener,
    timeout: Duration,
) -> Result<RemoteEasyconnectPairingResult, RemoteEasyconnectPairingError> {
    listener.set_nonblocking(true)?;
    let deadline = Instant::now() + timeout;
    let mut stream = loop {
        match listener.accept() {
            Ok((stream, _)) => break stream,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    return Err(RemoteEasyconnectPairingError::PairingTimedOut);
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(error) => return Err(error.into()),
        }
    };
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    let mut buffer = [0_u8; 8192];
    let read = stream.read(&mut buffer)?;
    let request = String::from_utf8_lossy(&buffer[..read]);
    let response = match parse_pairing_callback_request(&request) {
        Ok(result) => {
            write_http_response(
                &mut stream,
                "200 OK",
                "DASObjectStore easyconnect pairing was received. You may return to the terminal.",
            )?;
            return Ok(result);
        }
        Err(error) => {
            write_http_response(
                &mut stream,
                "400 Bad Request",
                "DASObjectStore easyconnect pairing callback was invalid.",
            )?;
            error
        }
    };
    Err(response)
}

fn write_http_response(
    writer: &mut impl Write,
    status: &str,
    body: &str,
) -> Result<(), std::io::Error> {
    write!(
        writer,
        "HTTP/1.1 {status}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

fn parse_pairing_callback_request(
    request: &str,
) -> Result<RemoteEasyconnectPairingResult, RemoteEasyconnectPairingError> {
    let request_line =
        request
            .lines()
            .next()
            .ok_or(RemoteEasyconnectPairingError::InvalidCallback(
                "missing HTTP request line".to_string(),
            ))?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default();
    let target = parts.next().unwrap_or_default();
    if method != "GET" {
        return Err(RemoteEasyconnectPairingError::InvalidCallback(
            "pairing callback must use GET".to_string(),
        ));
    }
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    if path != EASYCONNECT_CALLBACK_PATH {
        return Err(RemoteEasyconnectPairingError::InvalidCallback(format!(
            "pairing callback path must be {EASYCONNECT_CALLBACK_PATH}"
        )));
    }
    if let Some(error) = query_value(query, "error") {
        return Err(RemoteEasyconnectPairingError::PairingDenied(error));
    }
    let pairing_id = query_value(query, "pairing_id").ok_or_else(|| {
        RemoteEasyconnectPairingError::InvalidCallback("missing pairing_id".to_string())
    })?;
    let exchange_code = query_value(query, "exchange_code")
        .or_else(|| query_value(query, "code"))
        .ok_or_else(|| {
            RemoteEasyconnectPairingError::InvalidCallback(
                "missing one-time exchange_code".to_string(),
            )
        })?;
    Ok(RemoteEasyconnectPairingResult {
        pairing_id,
        exchange_code,
    })
}

fn query_value(query: &str, name: &str) -> Option<String> {
    query
        .split('&')
        .filter(|part| !part.is_empty())
        .find_map(|part| {
            let (key, value) = part.split_once('=').unwrap_or((part, ""));
            if url_decode(key) == name {
                Some(url_decode(value))
            } else {
                None
            }
        })
}

fn browser_open_command(url: &str) -> Command {
    #[cfg(target_os = "macos")]
    {
        let mut command = Command::new("open");
        command.arg(url);
        command
    }
    #[cfg(target_os = "windows")]
    {
        let mut command = Command::new("cmd");
        command.args(["/C", "start", "", url]);
        command
    }
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    {
        let mut command = Command::new("xdg-open");
        command.arg(url);
        command
    }
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
        (
            "session_renewal",
            "remote_cli",
            "Renew active upload sessions before expiry using the renewal token; do not keep or replay login passwords.",
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
            "session_renewal_denied",
            "The appliance rejected renewal of an active remote upload session.",
            true,
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

fn url_decode(value: &str) -> String {
    let mut output = Vec::new();
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'+' => {
                output.push(b' ');
                index += 1;
            }
            b'%' if index + 2 < bytes.len() => {
                if let Ok(hex) = u8::from_str_radix(&value[index + 1..index + 3], 16) {
                    output.push(hex);
                    index += 3;
                } else {
                    output.push(bytes[index]);
                    index += 1;
                }
            }
            byte => {
                output.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&output).into_owned()
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

#[derive(Debug)]
pub enum RemoteEasyconnectPairingError {
    Contract(RemoteEasyconnectContractError),
    Io(std::io::Error),
    CallbackBindFailed {
        address: String,
        source: std::io::Error,
    },
    BrowserLaunchFailed,
    PairingTimedOut,
    PairingDenied(String),
    InvalidCallback(String),
}

impl fmt::Display for RemoteEasyconnectPairingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Contract(error) => write!(formatter, "{error}"),
            Self::Io(error) => write!(formatter, "{error}"),
            Self::CallbackBindFailed { address, source } => {
                write!(formatter, "could not bind easyconnect callback at {address}: {source}")
            }
            Self::BrowserLaunchFailed => formatter.write_str(
                "could not open the browser automatically; rerun with --no-browser and open the printed URL",
            ),
            Self::PairingTimedOut => formatter.write_str("easyconnect pairing timed out"),
            Self::PairingDenied(error) if error.is_empty() => {
                formatter.write_str("easyconnect pairing was denied")
            }
            Self::PairingDenied(error) => write!(formatter, "easyconnect pairing was denied: {error}"),
            Self::InvalidCallback(message) => write!(formatter, "invalid easyconnect callback: {message}"),
        }
    }
}

impl std::error::Error for RemoteEasyconnectPairingError {}

impl From<RemoteEasyconnectContractError> for RemoteEasyconnectPairingError {
    fn from(error: RemoteEasyconnectContractError) -> Self {
        Self::Contract(error)
    }
}

impl From<std::io::Error> for RemoteEasyconnectPairingError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        define_easyconnect_contract, parse_pairing_callback_request, query_value,
        run_easyconnect_pairing, BrowserLauncher, RemoteEasyconnectContractError,
        RemoteEasyconnectContractRequest, RemoteEasyconnectPairingError,
        RemoteEasyconnectPairingOptions, DEFAULT_APPLIANCE_HTTPS_PORT,
        DEFAULT_REMOTE_SESSION_LIFETIME_SECS, DEFAULT_REMOTE_SESSION_RENEWAL_LEAD_SECS,
        EASYCONNECT_CALLBACK_PATH,
    };
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::time::Duration;

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
        assert_eq!(
            contract.default_session_lifetime_seconds,
            DEFAULT_REMOTE_SESSION_LIFETIME_SECS
        );
        assert_eq!(
            contract.session_renewal_lead_seconds,
            DEFAULT_REMOTE_SESSION_RENEWAL_LEAD_SECS
        );
        assert!(contract
            .lifecycle
            .iter()
            .any(|step| step.state == "session_renewal"
                && step
                    .message
                    .contains("do not keep or replay login passwords")));
        assert!(contract
            .failure_states
            .iter()
            .any(|failure| failure.code == "pairing_expired" && failure.retryable));
        assert!(contract
            .failure_states
            .iter()
            .any(|failure| failure.code == "session_renewal_denied" && failure.retryable));
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

    #[test]
    fn parses_pairing_callback_request() {
        let request = concat!(
            "GET /products/dasobjectstore/remote/easyconnect/callback?",
            "pairing_id=pair-123&exchange_code=one-time-code HTTP/1.1\r\n",
            "Host: 127.0.0.1\r\n\r\n"
        );

        let result = parse_pairing_callback_request(request).expect("callback parses");

        assert_eq!(result.pairing_id, "pair-123");
        assert_eq!(result.exchange_code, "one-time-code");
    }

    #[test]
    fn rejects_denied_pairing_callback() {
        let request = concat!(
            "GET /products/dasobjectstore/remote/easyconnect/callback?",
            "error=login_denied HTTP/1.1\r\n",
            "Host: 127.0.0.1\r\n\r\n"
        );

        let err = parse_pairing_callback_request(request).expect_err("denial rejected");

        assert!(matches!(
            err,
            RemoteEasyconnectPairingError::PairingDenied(message) if message == "login_denied"
        ));
    }

    #[test]
    fn rejects_expired_pairing_callback() {
        let request = concat!(
            "GET /products/dasobjectstore/remote/easyconnect/callback?",
            "error=pairing_expired HTTP/1.1\r\n",
            "Host: 127.0.0.1\r\n\r\n"
        );

        let err = parse_pairing_callback_request(request).expect_err("expiry rejected");

        assert!(matches!(
            err,
            RemoteEasyconnectPairingError::PairingDenied(message) if message == "pairing_expired"
        ));
    }

    #[test]
    fn launches_browser_and_waits_for_pairing_callback() {
        let launcher = CallbackLauncher;

        let outcome = run_easyconnect_pairing(
            RemoteEasyconnectPairingOptions {
                host_or_ip: "192.168.1.192".to_string(),
                https_port: DEFAULT_APPLIANCE_HTTPS_PORT,
                callback_port: None,
                timeout: Duration::from_secs(5),
                open_browser: true,
            },
            &launcher,
        )
        .expect("pairing callback received");

        assert_eq!(outcome.result.pairing_id, "pair-123");
        assert_eq!(outcome.result.exchange_code, "one-time-code");
        assert!(outcome
            .contract
            .local_callback_bind
            .starts_with("127.0.0.1:"));
        assert_ne!(outcome.contract.local_callback_bind, "127.0.0.1:0");
    }

    struct CallbackLauncher;

    impl BrowserLauncher for CallbackLauncher {
        fn open(&self, url: &str) -> Result<(), RemoteEasyconnectPairingError> {
            let query = url.split_once('?').map(|(_, query)| query).unwrap_or("");
            let callback = query_value(query, "callback").expect("callback URL");
            std::thread::spawn(move || send_pairing_callback(&callback));
            Ok(())
        }
    }

    fn send_pairing_callback(callback: &str) {
        let without_scheme = callback
            .strip_prefix("http://127.0.0.1:")
            .expect("loopback callback URL");
        let (port, path) = without_scheme.split_once('/').expect("callback path");
        let mut stream = TcpStream::connect(format!("127.0.0.1:{port}")).expect("callback socket");
        let target = format!("/{path}?pairing_id=pair-123&exchange_code=one-time-code");
        assert!(target.starts_with(EASYCONNECT_CALLBACK_PATH));
        write!(
            stream,
            "GET {target} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
        )
        .expect("write callback");
        let mut response = String::new();
        stream
            .read_to_string(&mut response)
            .expect("read callback response");
        assert!(response.starts_with("HTTP/1.1 200 OK"));
    }
}
