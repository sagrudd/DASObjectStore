use super::*;

#[cfg(any(target_arch = "wasm32", test))]
pub(super) fn bioinformatics_context_group(
    section: &str,
    cards: &[crate::api::BioinformaticsContextCardResponse],
) -> Vec<BioinformaticsContextSummary> {
    cards
        .iter()
        .map(|card| BioinformaticsContextSummary {
            section: section.to_string(),
            label: card.label.clone(),
            state: card.state.clone(),
            state_label: bioinformatics_readiness_state_label(&card.state).to_string(),
            summary: card.summary.clone(),
            detail: card.detail.clone(),
            evidence: bioinformatics_metadata_summary(&card.evidence),
        })
        .collect()
}

#[cfg(any(target_arch = "wasm32", test))]
pub fn bioinformatics_derivation_source_summaries(
    view: &BioinformaticsWorkspaceResponse,
) -> Vec<BioinformaticsDerivationSourceSummary> {
    view.derivation_sources
        .iter()
        .map(|source| BioinformaticsDerivationSourceSummary {
            source_kind: source.source_kind.clone(),
            source_id: source.source_id.clone(),
            display_name: source.display_name.clone(),
            object_type: source.object_type.clone(),
            parent: source
                .parent_id
                .clone()
                .unwrap_or_else(|| "top-level source".to_string()),
            endpoint_export: source
                .endpoint_export_mode
                .clone()
                .unwrap_or_else(|| "not exported".to_string()),
            binding: match &source.governance_domain {
                Some(domain) => format!("{} · {}", source.mneion_binding_state, domain),
                None => source.mneion_binding_state.clone(),
            },
            workflow_roles: bioinformatics_metadata_summary(&source.workflow_roles),
            evidence: bioinformatics_metadata_summary(&source.evidence),
        })
        .collect()
}

#[cfg(any(target_arch = "wasm32", test))]
pub fn bioinformatics_summary_cards(
    view: &BioinformaticsWorkspaceResponse,
) -> Vec<(String, String, String)> {
    let cards = bioinformatics_readiness_summaries(view);
    let context_cards = bioinformatics_context_summaries(view);
    let derivation_sources = bioinformatics_derivation_source_summaries(view);
    let workflow_ready = cards
        .iter()
        .filter(|card| card.state == "workflow_ready")
        .count();
    let metadata_needed = cards
        .iter()
        .filter(|card| card.state.contains("metadata"))
        .count();

    vec![
        (
            "Object families".to_string(),
            cards.len().to_string(),
            "Supported bioinformatics object classifications".to_string(),
        ),
        (
            "Workflow ready".to_string(),
            workflow_ready.to_string(),
            "Cards with sufficient default handoff semantics".to_string(),
        ),
        (
            "Metadata needed".to_string(),
            metadata_needed.to_string(),
            "Cards that require explicit reference or provenance binding".to_string(),
        ),
        (
            "Context views".to_string(),
            context_cards.len().to_string(),
            "Provenance, lineage, handoff, and governance cards".to_string(),
        ),
        (
            "Derivation sources".to_string(),
            derivation_sources.len().to_string(),
            "ObjectStore, SubObject, object-type, and Mneion source records".to_string(),
        ),
    ]
}

#[cfg(any(target_arch = "wasm32", test))]
pub(super) fn bioinformatics_readiness_state_label(state: &str) -> &'static str {
    match state {
        "workflow_ready" => "Workflow ready",
        "metadata_required" => "Metadata needed",
        "catalogue_ready" => "Catalogue ready",
        "planned" => "Planned",
        "binding_required" => "Binding needed",
        "reserved" => "Reserved",
        _ => "Review",
    }
}

#[cfg(any(target_arch = "wasm32", test))]
pub(super) fn bioinformatics_metadata_summary(required_metadata: &[String]) -> String {
    if required_metadata.is_empty() {
        "metadata contract pending".to_string()
    } else {
        required_metadata.join("; ")
    }
}

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Debug, Eq, PartialEq, Properties)]
pub struct BioinformaticsPageProps {
    pub api_base_path: String,
}

#[cfg(target_arch = "wasm32")]
#[function_component(BioinformaticsPage)]
pub fn bioinformatics_page(props: &BioinformaticsPageProps) -> Html {
    let api_path = WorkspacePage::Bioinformatics.api_path(&props.api_base_path);
    let bioinformatics_state =
        use_state(|| ApiLoadState::<BioinformaticsWorkspaceResponse>::Loading);

    {
        let api_path = api_path.clone();
        let bioinformatics_state = bioinformatics_state.clone();
        use_effect_with(api_path.clone(), move |path| {
            let path = path.clone();
            wasm_bindgen_futures::spawn_local(async move {
                bioinformatics_state.set(page_load_state_from_result(
                    crate::api::get_bioinformatics_workspace(&path).await,
                    |view| {
                        (view.supported_object_types.is_empty()
                            && view.readiness_cards.is_empty())
                        .then(|| {
                            "No bioinformatics object types or readiness cards were reported by the daemon workspace API."
                                .to_string()
                        })
                    },
                ));
            });
            || ()
        });
    }

    html! {
        <section class="dos-page" data-page="bioinformatics" data-api-route={api_path}>
            <PageHeader
                eyebrow="Workflow integration"
                title="Bioinformatics"
                summary="Sequencing data readiness, workflow handoff, and Mnemosyne integration state."
            />
            { render_bioinformatics_state(&*bioinformatics_state) }
        </section>
    }
}

#[cfg(target_arch = "wasm32")]
pub(super) fn render_bioinformatics_state(
    state: &ApiLoadState<BioinformaticsWorkspaceResponse>,
) -> Html {
    match state {
        ApiLoadState::Loading => render_bioinformatics_state_message(
            "Loading",
            "Loading bioinformatics readiness",
            "The Web console is requesting daemon-backed object type and workflow readiness state.",
        ),
        ApiLoadState::Success(view) | ApiLoadState::StaleData { value: view, .. } => {
            render_bioinformatics_workspace(view)
        }
        ApiLoadState::Empty(message) => render_bioinformatics_state_message(
            "Inventory",
            "No bioinformatics readiness data",
            message,
        ),
        ApiLoadState::PermissionDenied(message) => render_bioinformatics_state_message(
            "Permission denied",
            "Bioinformatics readiness requires an authenticated session",
            message,
        ),
        ApiLoadState::TransportError(message) => render_bioinformatics_state_message(
            "Error",
            "Unable to load bioinformatics readiness",
            message,
        ),
    }
}

#[cfg(target_arch = "wasm32")]
pub(super) fn render_bioinformatics_workspace(view: &BioinformaticsWorkspaceResponse) -> Html {
    let summaries = bioinformatics_readiness_summaries(view);
    let derivation_sources = bioinformatics_derivation_source_summaries(view);
    let context_summaries = bioinformatics_context_summaries(view);
    html! {
        <>
            <div class="dos-metric-grid">
                { for bioinformatics_summary_cards(view).into_iter().map(render_bioinformatics_metric_card) }
            </div>
            <section class="dos-card dos-wide-card" data-state={if view.available { "available" } else { "reserved" }}>
                <span class="dos-card-label">{ if view.available { "Workflow readiness" } else { "Reserved workflow" } }</span>
                <h2>{ if view.available { "Bioinformatics object-type readiness is available." } else { "Bioinformatics workspace is reserved." } }</h2>
                <p>{ &view.message }</p>
                <div class="dos-chip-row">
                    { for view.supported_object_types.iter().map(|object_type| html! {
                        <span class="dos-status-pill">{ object_type }</span>
                    }) }
                </div>
            </section>
            <div class="dos-store-grid">
                { for summaries.into_iter().map(render_bioinformatics_readiness_card) }
            </div>
            if !derivation_sources.is_empty() {
                <section class="dos-card dos-wide-card">
                    <span class="dos-card-label">{ "Metadata derivation" }</span>
                    <h2>{ "API-owned readiness source records" }</h2>
                    <p>{ "The Bioinformatics page renders source records supplied by the API instead of hard-coding ObjectStore, SubObject, or Mneion metadata paths in browser code." }</p>
                </section>
                <div class="dos-store-grid">
                    { for derivation_sources.into_iter().map(render_bioinformatics_derivation_source_card) }
                </div>
            }
            if !context_summaries.is_empty() {
                <section class="dos-card dos-wide-card">
                    <span class="dos-card-label">{ "Workflow context" }</span>
                    <h2>{ "Provenance, lineage, handoff, and governance state" }</h2>
                    <p>{ "These read-only cards describe the orchestration context that must be resolved before daemon-owned workflow dispatch." }</p>
                </section>
                <div class="dos-store-grid">
                    { for context_summaries.into_iter().map(render_bioinformatics_context_card) }
                </div>
            }
        </>
    }
}

#[cfg(target_arch = "wasm32")]
pub(super) fn render_bioinformatics_metric_card(metric: (String, String, String)) -> Html {
    html! {
        <section class="dos-card dos-metric-card">
            <div class="dos-card-row">
                <span class="dos-card-label">{ metric.0 }</span>
                <span class="dos-status-pill">{ "Readiness" }</span>
            </div>
            <strong>{ metric.1 }</strong>
            <p>{ metric.2 }</p>
        </section>
    }
}

#[cfg(target_arch = "wasm32")]
pub(super) fn render_bioinformatics_readiness_card(card: BioinformaticsReadinessSummary) -> Html {
    html! {
        <section class="dos-card dos-store-card" data-object-type={card.object_type.clone()} data-state={card.state.clone()}>
            <div class="dos-card-row">
                <span class="dos-card-label">{ card.category }</span>
                <span class="dos-status-pill">{ card.state_label }</span>
            </div>
            <strong>{ card.label }</strong>
            <p>{ card.primary_workflow }</p>
            <p>{ format!("Handoff: {}", card.handoff) }</p>
            <p>{ format!("Metadata: {}", card.metadata) }</p>
        </section>
    }
}

#[cfg(target_arch = "wasm32")]
pub(super) fn render_bioinformatics_derivation_source_card(
    source: BioinformaticsDerivationSourceSummary,
) -> Html {
    html! {
        <section class="dos-card dos-store-card" data-source-kind={source.source_kind.clone()} data-object-type={source.object_type.clone()}>
            <div class="dos-card-row">
                <span class="dos-card-label">{ source.source_kind }</span>
                <span class="dos-status-pill">{ source.object_type }</span>
            </div>
            <strong>{ source.display_name }</strong>
            <p>{ format!("Source: {} · parent {}", source.source_id, source.parent) }</p>
            <p>{ format!("Export: {} · binding {}", source.endpoint_export, source.binding) }</p>
            <p>{ format!("Roles: {}", source.workflow_roles) }</p>
            <p>{ format!("Evidence: {}", source.evidence) }</p>
        </section>
    }
}

#[cfg(target_arch = "wasm32")]
pub(super) fn render_bioinformatics_context_card(card: BioinformaticsContextSummary) -> Html {
    html! {
        <section class="dos-card dos-store-card" data-state={card.state.clone()}>
            <div class="dos-card-row">
                <span class="dos-card-label">{ card.section }</span>
                <span class="dos-status-pill">{ card.state_label }</span>
            </div>
            <strong>{ card.label }</strong>
            <p>{ card.summary }</p>
            <p>{ card.detail }</p>
            <p>{ format!("Evidence: {}", card.evidence) }</p>
        </section>
    }
}

#[cfg(target_arch = "wasm32")]
pub(super) fn render_bioinformatics_state_message(label: &str, title: &str, message: &str) -> Html {
    html! {
        <section class="dos-card dos-wide-card">
            <span class="dos-card-label">{ label }</span>
            <h2>{ title }</h2>
            <p>{ message }</p>
        </section>
    }
}
