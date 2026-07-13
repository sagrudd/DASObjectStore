use dasobjectstore_daemon::api::{DaemonIngestControlState, IngestControlResponse};

/// Fixed-viewport presentation of an ingest admission control response.
///
/// The action is submitted through the daemon-owned CLI/Web contract; this
/// type deliberately only renders the typed response and never mutates state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IngestControlDisplay {
    pub state_label: String,
    pub action_label: String,
    pub mode_label: String,
    pub reason_label: String,
    pub warning: Option<String>,
}

impl IngestControlDisplay {
    pub fn from_response(response: &IngestControlResponse) -> Self {
        let state_label = match response.state {
            DaemonIngestControlState::Running => "Running",
            DaemonIngestControlState::Throttled => "Throttled",
            DaemonIngestControlState::Paused => "Paused",
        };
        let action_label = if response.changed {
            "changed"
        } else {
            "unchanged"
        };
        let mode_label = if response.dry_run {
            "preview (daemon state unchanged)"
        } else {
            "applied"
        };
        Self {
            state_label: state_label.to_string(),
            action_label: action_label.to_string(),
            mode_label: mode_label.to_string(),
            reason_label: response.reason.clone(),
            warning: response.dry_run.then(|| {
                "Preview only: no new source-object admission state was changed.".to_string()
            }),
        }
    }

    /// Render a stable, line-oriented snapshot suitable for a compact TUI
    /// viewport and for captured operator evidence.
    pub fn snapshot_text(&self) -> String {
        let mut lines = vec![
            format!("Ingest admission: {}", self.state_label),
            format!("Action: {}", self.action_label),
            format!("Mode: {}", self.mode_label),
            format!("Reason: {}", self.reason_label),
        ];
        if let Some(warning) = &self.warning {
            lines.push(format!("Warning: {warning}"));
        }
        lines.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::IngestControlDisplay;
    use dasobjectstore_daemon::api::{DaemonIngestControlState, IngestControlResponse};

    #[test]
    fn renders_applied_pause_for_operator_view() {
        let display = IngestControlDisplay::from_response(&IngestControlResponse {
            state: DaemonIngestControlState::Paused,
            changed: true,
            dry_run: false,
            reason: "protect Web availability".to_string(),
        });

        assert_eq!(
            display.snapshot_text(),
            "Ingest admission: Paused\nAction: changed\nMode: applied\nReason: protect Web availability"
        );
    }

    #[test]
    fn marks_dry_run_as_non_mutating() {
        let display = IngestControlDisplay::from_response(&IngestControlResponse {
            state: DaemonIngestControlState::Throttled,
            changed: true,
            dry_run: true,
            reason: "preview pressure response".to_string(),
        });

        assert!(display
            .snapshot_text()
            .contains("preview (daemon state unchanged)"));
        assert!(display
            .snapshot_text()
            .contains("Preview only: no new source-object admission state was changed."));
    }
}
