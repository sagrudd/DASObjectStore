pub const MNEMOSYNE_HOME_URL: &str = "https://mnemosyne.co.uk";
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub const MNEMOSYNE_FOOTER_WORDMARK_SRC: &str = "mnemosyne-biosciences-logo-master-mono.png";
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub const MNEMOSYNE_FOOTER_PARTIAL_SRC: &str = "mnemosyne-biosciences-partial.png";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FooterAvailabilityState {
    Disconnected,
    CheckingSession,
    Connected,
    Busy,
    Error,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DasObjectStoreFooterContent {
    pub product_label: String,
    pub mnemosyne_url: String,
}

impl DasObjectStoreFooterContent {
    pub fn for_version(version: &str) -> Self {
        Self {
            product_label: format!("DASObjectStore v{version}"),
            mnemosyne_url: MNEMOSYNE_HOME_URL.to_string(),
        }
    }
}

pub fn footer_required_for_state(_state: FooterAvailabilityState) -> bool {
    true
}

#[cfg(target_arch = "wasm32")]
use yew::prelude::*;

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Debug, Eq, PartialEq, Properties)]
pub struct DasObjectStoreFooterProps {
    pub product_version: String,
}

#[cfg(target_arch = "wasm32")]
#[function_component(DasObjectStoreFooter)]
pub fn das_object_store_footer(props: &DasObjectStoreFooterProps) -> Html {
    let content = DasObjectStoreFooterContent::for_version(&props.product_version);
    let partial_style = format!("--dos-mnemosyne-partial: url(\"{MNEMOSYNE_FOOTER_PARTIAL_SRC}\")");

    html! {
        <footer class="dos-product-footer" aria-label="Mnemosyne Biosciences product footer">
            <a class="dos-product-footer__wordmark" href={content.mnemosyne_url} aria-label="Mnemosyne Biosciences">
                <img src={MNEMOSYNE_FOOTER_WORDMARK_SRC} alt="" aria-hidden="true" />
            </a>
            <span class="dos-product-footer__version">{ content.product_label }</span>
            <span class="dos-product-footer__partial" style={partial_style} aria-hidden="true"></span>
        </footer>
    }
}

#[cfg(test)]
mod tests {
    use super::{
        footer_required_for_state, DasObjectStoreFooterContent, FooterAvailabilityState,
        MNEMOSYNE_FOOTER_PARTIAL_SRC, MNEMOSYNE_FOOTER_WORDMARK_SRC, MNEMOSYNE_HOME_URL,
    };

    #[test]
    fn footer_content_matches_mnemosyne_product_contract() {
        let content = DasObjectStoreFooterContent::for_version("0.28.0");

        assert_eq!(content.product_label, "DASObjectStore v0.28.0");
        assert_eq!(content.mnemosyne_url, MNEMOSYNE_HOME_URL);
    }

    #[test]
    fn footer_is_required_for_all_app_states() {
        let states = [
            FooterAvailabilityState::Disconnected,
            FooterAvailabilityState::CheckingSession,
            FooterAvailabilityState::Connected,
            FooterAvailabilityState::Busy,
            FooterAvailabilityState::Error,
        ];

        assert!(states.into_iter().all(footer_required_for_state));
    }

    #[test]
    fn footer_assets_are_registered_as_local_trunk_files() {
        let index = include_str!("../../index.html");
        assert!(index.contains("Loading DASObjectStore…"));
        assert!(index.contains(MNEMOSYNE_FOOTER_WORDMARK_SRC));
        assert!(index.contains(MNEMOSYNE_FOOTER_PARTIAL_SRC));
        assert!(
            !include_bytes!("../../assets/mnemosyne-biosciences-logo-master-mono.png").is_empty()
        );
        assert!(!include_bytes!("../../assets/mnemosyne-biosciences-partial.png").is_empty());
    }

    #[test]
    fn footer_uses_the_mnemosyne_surface_and_single_cropped_mark_contract() {
        let styles = include_str!("../../styles.css");
        let source = include_str!("footer.rs");
        assert!(styles.contains(".dos-product-footer"));
        assert!(styles.contains("--dos-mnemosyne-footer-bg: #1c2b0b"));
        assert!(styles.contains(".dos-product-footer__partial"));
        assert!(styles.contains("overflow: visible"));
        assert!(styles.contains("bottom: 0"));
        assert!(styles.contains("height: 152px"));
        assert!(source.contains("MNEMOSYNE_FOOTER_WORDMARK_SRC"));
        assert!(styles.contains("transform: translateY(-61.4%)"));
        assert!(styles.contains("background: var(--dos-mnemosyne-mark-light)"));
        assert!(styles.contains("background: var(--dos-mnemosyne-mark-dark)"));
        let forbidden_orphan_label = ["content.mnemosyne", "_label"].concat();
        assert!(!source.contains(&forbidden_orphan_label));
        assert!(
            source.find("dos-product-footer__wordmark").unwrap()
                < source.find("dos-product-footer__version").unwrap()
        );
        assert!(styles.contains("--dos-interaction-primary: #0f6b78"));
        assert!(styles.contains("--dos-status-danger-fg: #8a3d24"));
        assert!(styles.contains("background: var(--dos-interaction-primary)"));
        assert!(styles.contains("color: var(--dos-status-danger-fg)"));
        assert!(styles.contains(".dos-task-pane"));
        assert!(!styles.contains(".dos-product-footer {\n  flex: 0 0 auto;\n  padding"));
    }
}
