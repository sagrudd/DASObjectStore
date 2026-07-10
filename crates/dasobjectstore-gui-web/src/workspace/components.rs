#[cfg(target_arch = "wasm32")]
use super::*;

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Debug, Eq, PartialEq, Properties)]
pub(super) struct PageHeaderProps {
    pub(super) eyebrow: &'static str,
    pub(super) title: &'static str,
    pub(super) summary: &'static str,
}

#[cfg(target_arch = "wasm32")]
#[function_component(PageHeader)]
pub(super) fn page_header(props: &PageHeaderProps) -> Html {
    html! {
        <header class="dos-page-header">
            <p>{ props.eyebrow }</p>
            <h1>{ props.title }</h1>
            <span>{ props.summary }</span>
        </header>
    }
}
