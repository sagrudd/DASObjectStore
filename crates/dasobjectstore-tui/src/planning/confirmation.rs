use super::{
    normalize_optional_text, ImportDescriptionMetadata, ImportDescriptionMetadataDisplay,
    ImportPlan, ImportPlanningSummary, IMPORT_LAUNCH_CONFIRMATION_PHRASE,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportLaunchConfirmation {
    pub metadata: ImportDescriptionMetadata,
    pub confirmation_phrase: Option<String>,
}

impl ImportLaunchConfirmation {
    pub fn new(metadata: ImportDescriptionMetadata, confirmation_phrase: Option<String>) -> Self {
        Self {
            metadata,
            confirmation_phrase: normalize_optional_text(confirmation_phrase),
        }
    }

    pub fn review(&self, plan: &ImportPlan) -> ImportLaunchReview {
        let mut blockers = Vec::new();

        if self.metadata.description.is_none() {
            blockers.push(ImportLaunchBlocker::MissingDescription);
        }

        match self.confirmation_phrase.as_deref() {
            None => blockers.push(ImportLaunchBlocker::MissingConfirmation {
                required_phrase: IMPORT_LAUNCH_CONFIRMATION_PHRASE,
            }),
            Some(value) if value != IMPORT_LAUNCH_CONFIRMATION_PHRASE => {
                blockers.push(ImportLaunchBlocker::ConfirmationMismatch {
                    required_phrase: IMPORT_LAUNCH_CONFIRMATION_PHRASE,
                });
            }
            Some(_) => {}
        }

        ImportLaunchReview {
            planning: plan.summary(),
            metadata: self.metadata.display_data(),
            required_confirmation_phrase: IMPORT_LAUNCH_CONFIRMATION_PHRASE,
            blockers,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportLaunchReview {
    pub planning: ImportPlanningSummary,
    pub metadata: ImportDescriptionMetadataDisplay,
    pub required_confirmation_phrase: &'static str,
    pub blockers: Vec<ImportLaunchBlocker>,
}

impl ImportLaunchReview {
    pub fn is_ready_to_launch(&self) -> bool {
        self.blockers.is_empty()
    }

    pub fn status_label(&self) -> &'static str {
        if self.is_ready_to_launch() {
            "ready"
        } else {
            "blocked"
        }
    }

    pub fn blocker_labels(&self) -> Vec<String> {
        self.blockers
            .iter()
            .map(ImportLaunchBlocker::display_label)
            .collect()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ImportLaunchBlocker {
    MissingDescription,
    MissingConfirmation { required_phrase: &'static str },
    ConfirmationMismatch { required_phrase: &'static str },
}

impl ImportLaunchBlocker {
    pub fn display_label(&self) -> String {
        match self {
            Self::MissingDescription => "import description is required".to_string(),
            Self::MissingConfirmation { required_phrase } => {
                format!("launch confirmation is required: `{required_phrase}`")
            }
            Self::ConfirmationMismatch { required_phrase } => {
                format!("launch confirmation must be `{required_phrase}`")
            }
        }
    }
}
