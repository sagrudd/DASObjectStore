#[derive(Clone, Debug, Eq, PartialEq)]
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
            Self::Error(message) => Some(message.clone()),
            _ => None,
        }
    }
}
