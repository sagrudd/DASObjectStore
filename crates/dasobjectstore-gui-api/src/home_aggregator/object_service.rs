//! Local S3-compatible service discovery for the Home dashboard.

use crate::dashboard::ObjectServiceStatusView;
use std::env;
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const DEFAULT_OBJECT_SERVICE_PORT: u16 = 3900;
const DEFAULT_OBJECT_SERVICE_BIND_ADDRESS: &str = "0.0.0.0";

pub(super) fn status() -> ObjectServiceStatusView {
    let port = DEFAULT_OBJECT_SERVICE_PORT;
    let bind_address = docker_object_service_binding(port)
        .unwrap_or_else(|| DEFAULT_OBJECT_SERVICE_BIND_ADDRESS.to_string());
    let active = local_tcp_listener_active(&bind_address, port);
    let remote_ready = active && !is_loopback_bind(&bind_address);
    let local_url = format!("http://127.0.0.1:{port}");
    let remote_url = remote_ready.then(|| remote_object_service_url(&bind_address, port));
    let service_state = docker_object_service_container_state(port);
    let message = if !active {
        Some("S3-compatible object service is not reachable on the appliance.".to_string())
    } else if is_loopback_bind(&bind_address) {
        Some(
            "S3-compatible object service is bound to loopback and cannot accept remote uploads."
                .to_string(),
        )
    } else {
        None
    };

    ObjectServiceStatusView {
        active,
        remote_ready,
        bind_address,
        port,
        local_url,
        remote_url,
        service_state,
        message,
    }
}

fn docker_object_service_binding(port: u16) -> Option<String> {
    let mut command = Command::new("docker");
    command.args([
        "ps",
        "--format",
        "{{.Ports}}",
        "--filter",
        &format!("publish={port}"),
    ]);
    let output = bounded_command_output(command, Duration::from_secs(2))?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_docker_published_bind_address(&stdout, port)
}

fn docker_object_service_container_state(port: u16) -> Option<String> {
    let mut command = Command::new("docker");
    command.args([
        "ps",
        "--format",
        "{{.Status}}",
        "--filter",
        &format!("publish={port}"),
    ]);
    let output = bounded_command_output(command, Duration::from_secs(2))?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .next()
        .map(str::trim)
        .filter(|state| !state.is_empty())
        .map(str::to_string)
}

fn bounded_command_output(mut command: Command, timeout: Duration) -> Option<Output> {
    command.stdout(Stdio::piped()).stderr(Stdio::null());
    let mut child = command.spawn().ok()?;
    let started_at = Instant::now();
    loop {
        if child.try_wait().ok()?.is_some() {
            return child.wait_with_output().ok();
        }
        if started_at.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn parse_docker_published_bind_address(ports: &str, port: u16) -> Option<String> {
    ports
        .split(',')
        .map(str::trim)
        .find_map(|entry| parse_docker_port_entry_bind_address(entry, port))
}

fn parse_docker_port_entry_bind_address(entry: &str, port: u16) -> Option<String> {
    let (host_side, container_side) = entry.split_once("->")?;
    if !published_port_spec_contains_port(container_side.split('/').next()?, port) {
        return None;
    }
    let host_side = host_side
        .rsplit_once(' ')
        .map_or(host_side, |(_, value)| value);
    let (address, host_ports) = host_side.rsplit_once(':')?;
    if !published_port_spec_contains_port(host_ports, port) {
        return None;
    }
    let address = address.trim_matches(['[', ']']).trim();
    (!address.is_empty()).then(|| address.to_string())
}

fn published_port_spec_contains_port(port_spec: &str, port: u16) -> bool {
    match port_spec.split_once('-') {
        Some((start, end)) => {
            let Ok(start) = start.parse::<u16>() else {
                return false;
            };
            let Ok(end) = end.parse::<u16>() else {
                return false;
            };
            (start..=end).contains(&port)
        }
        None => port_spec.parse::<u16>().is_ok_and(|value| value == port),
    }
}

fn local_tcp_listener_active(bind_address: &str, port: u16) -> bool {
    use std::net::{IpAddr, SocketAddr, TcpStream};

    let connect_host = match bind_address.parse::<IpAddr>() {
        Ok(address) if address.is_unspecified() => "127.0.0.1",
        Ok(_) => bind_address,
        Err(_) => "127.0.0.1",
    };
    let Ok(connect_addr) = format!("{connect_host}:{port}").parse::<SocketAddr>() else {
        return false;
    };
    TcpStream::connect_timeout(&connect_addr, Duration::from_millis(200)).is_ok()
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
    if let Ok(host) = env::var("DASOBJECTSTORE_PUBLIC_HOST") {
        let host = host.trim();
        if !host.is_empty() {
            return Some(host.to_string());
        }
    }
    let output = Command::new("hostname").arg("-I").output().ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .find(|address| !address.starts_with("127.") && !address.contains(':'))
        .map(str::to_string)
}
