use super::CliError;
use crate::cli::StatusArgs;
use dasobjectstore_core::DEFAULT_STANDALONE_CONFIG_PATH;
use dasobjectstore_daemon::DaemonRuntimeConfig;
use dasobjectstore_gui_api::StandaloneServerConfig;
#[cfg(test)]
use dasobjectstore_object_service::parse_docker_published_bind_address;
use dasobjectstore_object_service::{
    docker_object_service_binding, docker_object_service_container_state,
};
use serde::Serialize;
use std::fs;
use std::io::Write;
use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::process::{Command as ProcessCommand, Stdio};
use std::time::Duration;

pub(super) fn run_status(args: &StatusArgs, writer: &mut impl Write) -> Result<(), CliError> {
    let report = read_runtime_status();
    if args.json() {
        serde_json::to_writer_pretty(&mut *writer, &report)?;
        writer.write_all(b"\n")?;
    } else {
        write_runtime_status(&report, writer)?;
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct RuntimeStatusReport {
    daemon: RuntimeEndpointStatus,
    web: RuntimeEndpointStatus,
    object_service: RuntimeEndpointStatus,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct RuntimeEndpointStatus {
    name: &'static str,
    kind: &'static str,
    configured: bool,
    active: bool,
    remote_ready: bool,
    bind_address: Option<String>,
    port: Option<u16>,
    url: Option<String>,
    remote_url: Option<String>,
    service: Option<String>,
    service_state: Option<String>,
    config_path: Option<String>,
    message: Option<String>,
}

fn read_runtime_status() -> RuntimeStatusReport {
    RuntimeStatusReport {
        daemon: daemon_runtime_status(),
        web: web_runtime_status(),
        object_service: object_service_runtime_status(),
    }
}

fn daemon_runtime_status() -> RuntimeEndpointStatus {
    let socket_path = DaemonRuntimeConfig::default().socket_path;
    let service_state = systemd_service_state("dasobjectstored.service");
    let active = socket_path.exists() || service_state.as_deref() == Some("active");
    RuntimeEndpointStatus {
        name: "daemon",
        kind: "unix_socket",
        configured: true,
        active,
        remote_ready: active,
        bind_address: None,
        port: None,
        url: Some(socket_path.display().to_string()),
        remote_url: None,
        service: Some("dasobjectstored.service".to_string()),
        service_state,
        config_path: Some("/etc/dasobjectstore/daemon.json".to_string()),
        message: if active {
            None
        } else {
            Some("daemon socket is not available".to_string())
        },
    }
}

fn web_runtime_status() -> RuntimeEndpointStatus {
    let config_path = PathBuf::from(DEFAULT_STANDALONE_CONFIG_PATH);
    let config = fs::read_to_string(&config_path)
        .ok()
        .and_then(|contents| serde_json::from_str::<StandaloneServerConfig>(&contents).ok())
        .unwrap_or_else(StandaloneServerConfig::default);
    let socket_addr = config.socket_addr().ok();
    let active = socket_addr.is_some_and(local_tcp_listener_active);
    let service_state = systemd_service_state("dasobjectstore-server.service");
    RuntimeEndpointStatus {
        name: "web",
        kind: "https",
        configured: true,
        active,
        remote_ready: active && !is_loopback_bind(&config.bind_address),
        bind_address: Some(config.bind_address),
        port: Some(config.https_port),
        url: Some(config.public_base_url),
        remote_url: None,
        service: Some("dasobjectstore-server.service".to_string()),
        service_state,
        config_path: Some(config_path.display().to_string()),
        message: if active {
            None
        } else {
            Some("web listener is not reachable locally".to_string())
        },
    }
}

fn object_service_runtime_status() -> RuntimeEndpointStatus {
    let port = 3900;
    let docker_binding = docker_object_service_binding(port);
    let bind_address = docker_binding.unwrap_or_else(|| "0.0.0.0".to_string());
    let endpoint = format!("http://{bind_address}:{port}");
    let active = format!("{bind_address}:{port}")
        .parse::<SocketAddr>()
        .is_ok_and(local_tcp_listener_active);
    let remote_ready = active && !is_loopback_bind(&bind_address);
    RuntimeEndpointStatus {
        name: "object_service",
        kind: "s3_compatible",
        configured: true,
        active,
        remote_ready,
        bind_address: Some(bind_address.clone()),
        port: Some(port),
        url: Some(endpoint),
        remote_url: remote_ready.then(|| remote_object_service_url(&bind_address, port)),
        service: Some("docker".to_string()),
        service_state: docker_object_service_container_state(port),
        config_path: None,
        message: if bind_address == "127.0.0.1" {
            Some(
                "S3-compatible object-service listener is only reachable on loopback; remote upload clients need a non-loopback bind address".to_string(),
            )
        } else if active {
            None
        } else {
            Some("S3-compatible object-service listener is not reachable locally".to_string())
        },
    }
}

fn is_loopback_bind(bind_address: &str) -> bool {
    matches!(bind_address, "127.0.0.1" | "::1" | "localhost")
}

fn remote_object_service_url(bind_address: &str, port: u16) -> String {
    let host = if bind_address == "0.0.0.0" || bind_address == "::" {
        public_host_address().unwrap_or_else(|| bind_address.to_string())
    } else {
        bind_address.to_string()
    };
    format!("http://{host}:{port}")
}

fn public_host_address() -> Option<String> {
    if let Ok(host) = std::env::var("DASOBJECTSTORE_PUBLIC_HOST") {
        let host = host.trim();
        if !host.is_empty() {
            return Some(host.to_string());
        }
    }
    let output = ProcessCommand::new("hostname").arg("-I").output().ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .find(|address| !address.starts_with("127.") && !address.contains(':'))
        .map(str::to_string)
}

fn local_tcp_listener_active(addr: SocketAddr) -> bool {
    let connect_addr = if addr.ip().is_unspecified() {
        SocketAddr::new("127.0.0.1".parse().expect("loopback IP"), addr.port())
    } else {
        addr
    };
    TcpStream::connect_timeout(&connect_addr, Duration::from_millis(200)).is_ok()
}

fn systemd_service_state(service: &str) -> Option<String> {
    let output = ProcessCommand::new("systemctl")
        .arg("is-active")
        .arg(service)
        .stderr(Stdio::null())
        .output()
        .ok()?;
    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
        .filter(|value| !value.is_empty())
}

fn write_runtime_status(
    report: &RuntimeStatusReport,
    writer: &mut impl Write,
) -> Result<(), CliError> {
    writeln!(writer, "DASObjectStore status")?;
    write_runtime_endpoint_status(&report.daemon, writer)?;
    write_runtime_endpoint_status(&report.web, writer)?;
    write_runtime_endpoint_status(&report.object_service, writer)?;
    Ok(())
}

fn write_runtime_endpoint_status(
    endpoint: &RuntimeEndpointStatus,
    writer: &mut impl Write,
) -> Result<(), CliError> {
    writeln!(
        writer,
        "- {}: {}{}",
        endpoint.name,
        if endpoint.active {
            "active"
        } else {
            "inactive"
        },
        endpoint
            .service_state
            .as_deref()
            .map(|state| format!(" (service {state})"))
            .unwrap_or_default()
    )?;
    if let Some(url) = &endpoint.url {
        writeln!(writer, "  url: {url}")?;
    }
    if let Some(remote_url) = &endpoint.remote_url {
        writeln!(writer, "  remote url: {remote_url}")?;
    }
    if let Some(bind_address) = &endpoint.bind_address {
        writeln!(
            writer,
            "  bind: {}:{}",
            bind_address,
            endpoint.port.unwrap_or_default()
        )?;
    }
    if let Some(config_path) = &endpoint.config_path {
        writeln!(writer, "  config: {config_path}")?;
    }
    if let Some(message) = &endpoint.message {
        writeln!(writer, "  note: {message}")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{parse_docker_published_bind_address, remote_object_service_url};

    #[test]
    fn parses_loopback_and_range_bindings() {
        assert_eq!(
            parse_docker_published_bind_address("127.0.0.1:3900->3900/tcp", 3900).as_deref(),
            Some("127.0.0.1")
        );
        assert_eq!(
            parse_docker_published_bind_address("127.0.0.1:3900-3903->3900-3903/tcp", 3900)
                .as_deref(),
            Some("127.0.0.1")
        );
    }

    #[test]
    fn parses_public_bindings_and_preserves_specific_remote_host() {
        assert_eq!(
            parse_docker_published_bind_address(
                "0.0.0.0:3900->3900/tcp, [::]:3900->3900/tcp",
                3900
            )
            .as_deref(),
            Some("0.0.0.0")
        );
        assert_eq!(
            remote_object_service_url("192.168.1.192", 3900),
            "http://192.168.1.192:3900"
        );
    }
}
