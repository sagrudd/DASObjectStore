use super::model::{ApplianceSessionTelemetry, ApplianceTelemetryMissingReason};
use serde_json::Value;
use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub const DEFAULT_STANDALONE_AUTH_ROOT: &str = "/var/lib/dasobjectstore/auth";
pub const DEFAULT_REMOTE_EASYCONNECT_SESSION_PATH: &str =
    "/var/lib/dasobjectstore/remote-easyconnect/sessions.json";
pub const DEFAULT_LOCAL_GROUP_PATH: &str = "/etc/group";
const ADMINISTRATOR_GROUPS: &[&str] = &["sudo", "wheel", "admin"];

#[derive(Debug, Default)]
struct SessionCollection {
    web_active_sessions: u64,
    remote_agent_active_sessions: u64,
    active_users: BTreeSet<String>,
    session_users: Vec<String>,
    configured_sources: u64,
    errors: Vec<PathBuf>,
}

pub fn collect_appliance_session_telemetry(
    web_auth_root: Option<&Path>,
    remote_session_path: Option<&Path>,
    local_group_path: Option<&Path>,
    now_utc: &str,
    now_unix_seconds: i64,
) -> ApplianceSessionTelemetry {
    let mut collection = SessionCollection::default();
    if let Some(web_auth_root) = web_auth_root {
        collection.configured_sources = collection.configured_sources.saturating_add(1);
        collect_web_auth_sessions(web_auth_root, now_utc, now_unix_seconds, &mut collection);
    }
    if let Some(remote_session_path) = remote_session_path {
        collection.configured_sources = collection.configured_sources.saturating_add(1);
        collect_remote_agent_sessions(remote_session_path, now_utc, &mut collection);
    }

    let (administrator_sessions, operator_sessions) = local_group_path
        .and_then(|path| administrator_users(path).ok())
        .map(|administrators| {
            let administrator_sessions = collection
                .session_users
                .iter()
                .filter(|username| administrators.contains(username.as_str()))
                .count() as u64;
            (
                administrator_sessions,
                collection
                    .session_users
                    .len()
                    .saturating_sub(administrator_sessions as usize) as u64,
            )
        })
        .map_or((None, None), |(administrators, operators)| {
            (Some(administrators), Some(operators))
        });

    ApplianceSessionTelemetry {
        web_active_sessions: Some(collection.web_active_sessions),
        remote_agent_active_sessions: Some(collection.remote_agent_active_sessions),
        distinct_logged_in_users: Some(collection.active_users.len() as u64),
        administrator_sessions,
        operator_sessions,
        missing_reason: if !collection.errors.is_empty() {
            Some(ApplianceTelemetryMissingReason::CollectorUnavailable)
        } else if collection.configured_sources == 0 {
            Some(ApplianceTelemetryMissingReason::NotConfigured)
        } else {
            None
        },
    }
}

fn collect_web_auth_sessions(
    web_auth_root: &Path,
    now_utc: &str,
    now_unix_seconds: i64,
    collection: &mut SessionCollection,
) {
    let entries = match fs::read_dir(web_auth_root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return,
        Err(_) => {
            collection.errors.push(web_auth_root.to_path_buf());
            return;
        }
    };

    for entry in entries {
        let Ok(entry) = entry else {
            collection.errors.push(web_auth_root.to_path_buf());
            continue;
        };
        let path = entry.path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
            continue;
        }
        let Ok(contents) = fs::read_to_string(&path) else {
            collection.errors.push(path);
            continue;
        };
        let Ok(registry) = serde_json::from_str::<Value>(&contents) else {
            continue;
        };
        let Some(users) = registry.get("users").and_then(Value::as_array) else {
            continue;
        };
        for user in users {
            let Some(username) = user.get("username").and_then(Value::as_str) else {
                continue;
            };
            let active_sessions = user
                .get("sessions")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter(|session| session_is_active(session, now_utc, now_unix_seconds))
                .count() as u64;
            if active_sessions == 0 {
                continue;
            }
            collection.web_active_sessions = collection
                .web_active_sessions
                .saturating_add(active_sessions);
            record_active_user(collection, username, active_sessions);
        }
    }
}

fn collect_remote_agent_sessions(
    remote_session_path: &Path,
    now_utc: &str,
    collection: &mut SessionCollection,
) {
    let contents = match fs::read_to_string(remote_session_path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return,
        Err(_) => {
            collection.errors.push(remote_session_path.to_path_buf());
            return;
        }
    };
    let Ok(store) = serde_json::from_str::<Value>(&contents) else {
        collection.errors.push(remote_session_path.to_path_buf());
        return;
    };
    let Some(sessions) = store.get("sessions").and_then(Value::as_array) else {
        return;
    };
    for session in sessions {
        if !remote_session_is_active(session, now_utc) {
            continue;
        }
        let Some(username) = session.get("approved_actor").and_then(Value::as_str) else {
            continue;
        };
        collection.remote_agent_active_sessions =
            collection.remote_agent_active_sessions.saturating_add(1);
        record_active_user(collection, username, 1);
    }
}

fn record_active_user(collection: &mut SessionCollection, username: &str, sessions: u64) {
    let username = username.trim();
    if username.is_empty() {
        return;
    }
    collection.active_users.insert(username.to_string());
    for _ in 0..sessions {
        collection.session_users.push(username.to_string());
    }
}

fn session_is_active(session: &Value, now_utc: &str, now_unix_seconds: i64) -> bool {
    let revoked = session
        .get("revoked_at_unix_seconds")
        .is_some_and(|value| !value.is_null())
        || session
            .get("revoked_at_utc")
            .is_some_and(|value| !value.is_null());
    if revoked {
        return false;
    }

    session
        .get("expires_at_unix_seconds")
        .and_then(Value::as_i64)
        .is_some_and(|expires_at| expires_at > now_unix_seconds)
        || session
            .get("expires_at_utc")
            .and_then(Value::as_str)
            .is_some_and(|expires_at| expires_at > now_utc)
}

fn remote_session_is_active(session: &Value, now_utc: &str) -> bool {
    if session
        .get("revoked_at_utc")
        .is_some_and(|value| !value.is_null())
    {
        return false;
    }
    session
        .get("expires_at_utc")
        .and_then(Value::as_str)
        .is_some_and(|expires_at| expires_at > now_utc)
}

fn administrator_users(group_path: &Path) -> io::Result<BTreeSet<String>> {
    let contents = fs::read_to_string(group_path)?;
    let mut users = BTreeSet::from(["root".to_string()]);
    for line in contents.lines() {
        let mut fields = line.split(':');
        let Some(group_name) = fields.next() else {
            continue;
        };
        let _password = fields.next();
        let _gid = fields.next();
        let members = fields.next().unwrap_or_default();
        if !ADMINISTRATOR_GROUPS.contains(&group_name) {
            continue;
        }
        for member in members
            .split(',')
            .map(str::trim)
            .filter(|member| !member.is_empty())
        {
            users.insert(member.to_string());
        }
    }
    Ok(users)
}
