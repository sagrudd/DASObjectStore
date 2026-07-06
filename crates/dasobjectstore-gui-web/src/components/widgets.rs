use super::{capacity_percent, InspectorSection, SegmentedOption, StatusTone};
use yew::prelude::*;

#[derive(Clone, Debug, Eq, PartialEq, Properties)]
pub struct DenseTableProps {
    pub caption: String,
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub empty_label: String,
}

#[function_component(DenseTable)]
pub fn dense_table(props: &DenseTableProps) -> Html {
    html! {
        <table class="dos-dense-table">
            <caption>{ props.caption.clone() }</caption>
            <thead>
                <tr>
                    { for props.headers.iter().map(|header| html! { <th scope="col">{ header }</th> }) }
                </tr>
            </thead>
            <tbody>
                if props.rows.is_empty() {
                    <tr>
                        <td colspan={props.headers.len().to_string()}>{ props.empty_label.clone() }</td>
                    </tr>
                } else {
                    { for props.rows.iter().map(|row| html! {
                        <tr>
                            { for row.iter().map(|cell| html! { <td>{ cell }</td> }) }
                        </tr>
                    }) }
                }
            </tbody>
        </table>
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Properties)]
pub struct InspectorDrawerProps {
    pub title: String,
    pub open: bool,
    pub sections: Vec<InspectorSection>,
}

#[function_component(InspectorDrawer)]
pub fn inspector_drawer(props: &InspectorDrawerProps) -> Html {
    html! {
        <aside class="dos-inspector-drawer" data-open={props.open.to_string()}>
            <header class="dos-inspector-drawer__header">
                <h2>{ props.title.clone() }</h2>
            </header>
            <dl class="dos-inspector-drawer__sections">
                { for props.sections.iter().map(|section| html! {
                    <>
                        <dt>{ &section.label }</dt>
                        <dd>{ &section.value }</dd>
                    </>
                }) }
            </dl>
        </aside>
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Properties)]
pub struct StatusBadgeProps {
    pub label: String,
    pub tone: StatusTone,
}

#[function_component(StatusBadge)]
pub fn status_badge(props: &StatusBadgeProps) -> Html {
    html! {
        <span class={format!("dos-status-badge dos-status-badge--{}", props.tone.class_suffix())}>
            { props.label.clone() }
        </span>
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Properties)]
pub struct CapacityBarProps {
    pub label: String,
    pub used_bytes: u64,
    pub capacity_bytes: u64,
}

#[function_component(CapacityBar)]
pub fn capacity_bar(props: &CapacityBarProps) -> Html {
    let percent = capacity_percent(props.used_bytes, props.capacity_bytes);

    html! {
        <div class="dos-capacity-bar">
            <div class="dos-capacity-bar__label">{ props.label.clone() }</div>
            <div
                class="dos-capacity-bar__track"
                role="progressbar"
                aria-label={props.label.clone()}
                aria-valuemin="0"
                aria-valuemax="100"
                aria-valuenow={percent.to_string()}
            >
                <div class="dos-capacity-bar__fill" style={format!("width: {}%;", percent)} />
            </div>
        </div>
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Properties)]
pub struct SegmentedControlProps {
    pub label: String,
    pub options: Vec<SegmentedOption>,
}

#[function_component(SegmentedControl)]
pub fn segmented_control(props: &SegmentedControlProps) -> Html {
    html! {
        <div class="dos-segmented-control" role="group" aria-label={props.label.clone()}>
            { for props.options.iter().map(|option| html! {
                <button
                    class="dos-segmented-control__option"
                    type="button"
                    data-value={option.value.clone()}
                    aria-pressed={option.selected.to_string()}
                    disabled={option.disabled}
                >
                    { option.label.clone() }
                </button>
            }) }
        </div>
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Properties)]
pub struct IconButtonProps {
    pub icon: String,
    pub label: String,
    pub disabled: bool,
    pub tone: StatusTone,
}

#[function_component(IconButton)]
pub fn icon_button(props: &IconButtonProps) -> Html {
    html! {
        <button
            class={format!("dos-icon-button dos-icon-button--{}", props.tone.class_suffix())}
            type="button"
            aria-label={props.label.clone()}
            title={props.label.clone()}
            disabled={props.disabled}
        >
            <span aria-hidden="true">{ props.icon.clone() }</span>
        </button>
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Properties)]
pub struct RiskyConfirmationPanelProps {
    pub operation: String,
    pub impact: String,
    pub confirmation_label: String,
    pub enabled: bool,
}

#[function_component(RiskyConfirmationPanel)]
pub fn risky_confirmation_panel(props: &RiskyConfirmationPanelProps) -> Html {
    html! {
        <section class="dos-risky-confirmation" data-enabled={props.enabled.to_string()}>
            <header>
                <h2>{ props.operation.clone() }</h2>
            </header>
            <p>{ props.impact.clone() }</p>
            <label>
                <input type="checkbox" disabled={!props.enabled} />
                { props.confirmation_label.clone() }
            </label>
            <button type="button" disabled={!props.enabled}>{ "Confirm" }</button>
        </section>
    }
}
