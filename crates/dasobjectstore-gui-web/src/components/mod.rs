mod footer;
#[cfg(target_arch = "wasm32")]
mod widgets;

pub use footer::{
    footer_required_for_state, DasObjectStoreFooterContent, FooterAvailabilityState,
    MNEMOSYNE_HOME_URL,
};
#[cfg(target_arch = "wasm32")]
pub use footer::{DasObjectStoreFooter, DasObjectStoreFooterProps};

#[cfg(target_arch = "wasm32")]
pub use widgets::{
    CapacityBar, CapacityBarProps, DenseTable, DenseTableProps, IconButton, IconButtonProps,
    InspectorDrawer, InspectorDrawerProps, RiskyConfirmationPanel, RiskyConfirmationPanelProps,
    SegmentedControl, SegmentedControlProps, StatusBadge, StatusBadgeProps, TaskPane, TaskPaneMode,
    TaskPaneProps,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StatusTone {
    Neutral,
    Good,
    Info,
    Warning,
    Critical,
}

impl StatusTone {
    pub fn class_suffix(self) -> &'static str {
        match self {
            Self::Neutral => "neutral",
            Self::Good => "good",
            Self::Info => "info",
            Self::Warning => "warning",
            Self::Critical => "critical",
        }
    }
}

pub fn capacity_percent(used_bytes: u64, capacity_bytes: u64) -> u8 {
    if capacity_bytes == 0 {
        return 0;
    }

    ((used_bytes as u128 * 100) / capacity_bytes as u128).min(100) as u8
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InspectorSection {
    pub label: String,
    pub value: String,
}

impl InspectorSection {
    pub fn new(label: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            value: value.into(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SegmentedOption {
    pub value: String,
    pub label: String,
    pub selected: bool,
    pub disabled: bool,
}

impl SegmentedOption {
    pub fn new(value: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            label: label.into(),
            selected: false,
            disabled: false,
        }
    }

    pub fn selected(mut self) -> Self {
        self.selected = true;
        self
    }

    pub fn disabled(mut self) -> Self {
        self.disabled = true;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::{capacity_percent, InspectorSection, SegmentedOption, StatusTone};

    #[test]
    fn clamps_capacity_percent() {
        assert_eq!(capacity_percent(0, 0), 0);
        assert_eq!(capacity_percent(50, 200), 25);
        assert_eq!(capacity_percent(250, 200), 100);
    }

    #[test]
    fn maps_status_tones_to_class_suffixes() {
        assert_eq!(StatusTone::Neutral.class_suffix(), "neutral");
        assert_eq!(StatusTone::Good.class_suffix(), "good");
        assert_eq!(StatusTone::Info.class_suffix(), "info");
        assert_eq!(StatusTone::Warning.class_suffix(), "warning");
        assert_eq!(StatusTone::Critical.class_suffix(), "critical");
    }

    #[test]
    fn builds_inspector_sections_and_segmented_options() {
        let section = InspectorSection::new("Health", "Watch");
        let option = SegmentedOption::new("generated", "Generated").selected();
        let disabled = SegmentedOption::new("critical", "Critical").disabled();

        assert_eq!(section.label, "Health");
        assert!(option.selected);
        assert!(!option.disabled);
        assert!(!disabled.selected);
        assert!(disabled.disabled);
    }
}
