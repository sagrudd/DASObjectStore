#![cfg_attr(test, allow(dead_code))]

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StableState {
    Disconnected,
    Connected,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AppState {
    CheckingSession,
    Disconnected,
    Connecting,
    Connected,
    Disconnecting,
    SessionInvalid(String),
    ServerUnreachable(String),
    Error(String),
}

impl AppState {
    pub fn busy_label(&self) -> Option<&'static str> {
        match self {
            Self::CheckingSession => Some("Checking session..."),
            Self::Connecting => Some("Signing in..."),
            Self::Disconnecting => Some("Signing out..."),
            _ => None,
        }
    }

    pub fn error_message(&self) -> Option<String> {
        match self {
            Self::SessionInvalid(message)
            | Self::ServerUnreachable(message)
            | Self::Error(message) => Some(message.clone()),
            _ => None,
        }
    }

    pub const fn is_server_unreachable(&self) -> bool {
        matches!(self, Self::ServerUnreachable(_))
    }
}

#[cfg(test)]
mod tests {
    use super::AppState;

    #[test]
    fn invalid_and_unreachable_states_surface_login_messages() {
        assert_eq!(
            AppState::SessionInvalid("session expired".to_string()).error_message(),
            Some("session expired".to_string())
        );
        assert_eq!(
            AppState::ServerUnreachable("server offline".to_string()).error_message(),
            Some("server offline".to_string())
        );
        assert!(AppState::ServerUnreachable("server offline".to_string()).is_server_unreachable());
        assert!(!AppState::SessionInvalid("session expired".to_string()).is_server_unreachable());
    }
}
