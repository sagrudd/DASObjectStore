//! Bounded local inspection of the Docker-backed object service.

use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

pub const DEFAULT_OBJECT_SERVICE_PORT: u16 = 3900;
const INSPECTION_TIMEOUT: Duration = Duration::from_secs(2);

pub fn docker_object_service_binding(port: u16) -> Option<String> {
    let mut command = Command::new("docker");
    command.args([
        "ps",
        "--format",
        "{{.Ports}}",
        "--filter",
        &format!("publish={port}"),
    ]);
    let output = bounded_command_output(command)?;
    if !output.status.success() {
        return None;
    }
    parse_docker_published_bind_address(&String::from_utf8_lossy(&output.stdout), port)
}

pub fn docker_object_service_container_state(port: u16) -> Option<String> {
    let mut command = Command::new("docker");
    command.args([
        "ps",
        "--format",
        "{{.Status}}",
        "--filter",
        &format!("publish={port}"),
    ]);
    let output = bounded_command_output(command)?;
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

pub fn parse_docker_published_bind_address(ports: &str, port: u16) -> Option<String> {
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

fn bounded_command_output(mut command: Command) -> Option<Output> {
    command.stdout(Stdio::piped()).stderr(Stdio::null());
    let mut child = command.spawn().ok()?;
    let started_at = Instant::now();
    loop {
        if child.try_wait().ok()?.is_some() {
            return child.wait_with_output().ok();
        }
        if started_at.elapsed() >= INSPECTION_TIMEOUT {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(test)]
mod tests {
    use super::parse_docker_published_bind_address;

    #[test]
    fn parses_loopback_and_public_bindings() {
        assert_eq!(
            parse_docker_published_bind_address("127.0.0.1:3900->3900/tcp", 3900).as_deref(),
            Some("127.0.0.1")
        );
        assert_eq!(
            parse_docker_published_bind_address(
                "0.0.0.0:3900->3900/tcp, [::]:3900->3900/tcp",
                3900
            )
            .as_deref(),
            Some("0.0.0.0")
        );
    }
}
