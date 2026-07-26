use dasobjectstore_workspace_host::{
    execute_request, BrokerConfig, BrokerRequest, BrokerResponse, PROTOCOL_VERSION,
};
use std::env;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::fd::{FromRawFd, RawFd};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;

const DEFAULT_CONFIG: &str = "/etc/dasobjectstore/workspace-host.json";
const MAX_REQUEST_BYTES: u64 = 256 * 1024;

fn main() {
    if unsafe { libc::geteuid() } != 0 {
        eprintln!("dasobjectstore-workspace-host must run as root");
        std::process::exit(1);
    }
    let config_path = env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_CONFIG));
    let config = BrokerConfig::load_root_owned(&config_path).unwrap_or_else(|error| {
        eprintln!("{error}");
        std::process::exit(1);
    });
    let listener = inherited_listener().unwrap_or_else(|error| {
        eprintln!("{error}");
        std::process::exit(1);
    });
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => handle_connection(&config, stream),
            Err(error) => eprintln!("accept broker connection: {error}"),
        }
    }
}

fn inherited_listener() -> Result<UnixListener, String> {
    let listen_pid = env::var("LISTEN_PID")
        .map_err(|_| "broker requires systemd socket activation".to_string())?
        .parse::<u32>()
        .map_err(|_| "invalid LISTEN_PID".to_string())?;
    let listen_fds = env::var("LISTEN_FDS")
        .map_err(|_| "broker requires systemd socket activation".to_string())?
        .parse::<u32>()
        .map_err(|_| "invalid LISTEN_FDS".to_string())?;
    if listen_pid != std::process::id() || listen_fds != 1 {
        return Err("broker requires exactly one inherited socket".to_string());
    }
    const SYSTEMD_LISTEN_FD: RawFd = 3;
    // SAFETY: systemd guarantees descriptor 3 is owned by this process when
    // LISTEN_PID matches and LISTEN_FDS is one. Ownership transfers once.
    Ok(unsafe { UnixListener::from_raw_fd(SYSTEMD_LISTEN_FD) })
}

fn handle_connection(config: &BrokerConfig, mut stream: UnixStream) {
    let mut line = String::new();
    let result = BufReader::new(
        stream
            .try_clone()
            .expect("Unix stream cloning cannot change the authority boundary"),
    )
    .take(MAX_REQUEST_BYTES)
    .read_line(&mut line)
    .map_err(|error| error.to_string())
    .and_then(|_| serde_json::from_str::<BrokerRequest>(&line).map_err(|error| error.to_string()))
    .and_then(|request| {
        execute_request(config, &request).map_err(|error| {
            serde_json::to_string(&BrokerResponse {
                protocol_version: PROTOCOL_VERSION,
                request_id: request.request_id,
                workspace_id: request.workspace_id,
                ok: false,
                error_code: Some("workspace_host_rejected".to_string()),
                error_message: Some(error.to_string()),
                branches: Vec::new(),
            })
            .expect("serialize bounded error response")
        })
    });
    let response = match result {
        Ok(response) => serde_json::to_string(&response).expect("serialize response"),
        Err(serialized_or_error) if serialized_or_error.starts_with('{') => serialized_or_error,
        Err(error) => serde_json::to_string(&BrokerResponse {
            protocol_version: PROTOCOL_VERSION,
            request_id: "invalid".to_string(),
            workspace_id: "invalid".to_string(),
            ok: false,
            error_code: Some("invalid_request".to_string()),
            error_message: Some(error),
            branches: Vec::new(),
        })
        .expect("serialize protocol error"),
    };
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.write_all(b"\n");
}
