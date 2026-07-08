pub const ENDPOINTS_WORKSPACE_ROUTE: &str = "workspaces/endpoints";
pub const ENDPOINT_INVENTORY_UPSERT_ROUTE: &str = "workspaces/endpoints/upsert";
pub const ENDPOINT_RECORD_CONFIRMATION: &str = "record endpoint inventory";

pub fn endpoints_workspace_api_path(api_base_path: &str) -> String {
    format!(
        "{}/{}",
        api_base_path.trim_end_matches('/'),
        ENDPOINTS_WORKSPACE_ROUTE
    )
}

pub fn endpoint_inventory_upsert_action_api_path(api_base_path: &str) -> String {
    format!(
        "{}/{}",
        api_base_path.trim_end_matches('/'),
        ENDPOINT_INVENTORY_UPSERT_ROUTE
    )
}

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EndpointCardSummary {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub validation: String,
    pub object_service_url: String,
    pub manager_product_id: String,
    pub bindings: String,
    pub warning_count: usize,
    pub checked_at: String,
}

#[cfg(any(target_arch = "wasm32", test))]
pub fn endpoint_card_summaries(
    view: &crate::api::EndpointsWorkspaceResponse,
) -> Vec<EndpointCardSummary> {
    view.inventory
        .endpoints
        .iter()
        .map(|endpoint| EndpointCardSummary {
            id: endpoint.endpoint_id.clone(),
            name: endpoint.display_name.clone(),
            kind: endpoint.kind.clone(),
            validation: endpoint.validation.state.clone(),
            object_service_url: endpoint.object_service_url.clone(),
            manager_product_id: endpoint.manager_product_id.clone(),
            bindings: format!("{} binding(s)", endpoint.active_bindings.len()),
            warning_count: endpoint.warnings.len(),
            checked_at: endpoint
                .validation
                .checked_at_utc
                .as_deref()
                .unwrap_or("not checked")
                .to_string(),
        })
        .collect()
}

#[cfg(any(target_arch = "wasm32", test))]
pub fn endpoint_inventory_summary_cards(
    view: &crate::api::EndpointsWorkspaceResponse,
) -> Vec<(String, String, String)> {
    vec![
        (
            "Endpoints".to_string(),
            view.inventory.endpoint_count.to_string(),
            "Registered storage endpoints".to_string(),
        ),
        (
            "Bindings".to_string(),
            view.inventory.binding_count.to_string(),
            "Active ObjectStore/governance-domain bindings".to_string(),
        ),
        (
            "Degraded".to_string(),
            view.inventory.degraded_endpoint_count.to_string(),
            "Endpoints requiring operator review".to_string(),
        ),
        (
            "Warnings".to_string(),
            view.inventory.warnings.len().to_string(),
            "Endpoint and binding warnings".to_string(),
        ),
    ]
}

#[cfg(any(target_arch = "wasm32", test))]
pub fn endpoint_upsert_fields_ready(
    endpoint_id: &str,
    display_name: &str,
    object_service_url: &str,
) -> bool {
    !endpoint_id.trim().is_empty()
        && !display_name.trim().is_empty()
        && !object_service_url.trim().is_empty()
}

#[cfg(any(target_arch = "wasm32", test))]
pub fn endpoint_binding_fields_ready(
    binding_enabled: bool,
    binding_id: &str,
    governance_domain: &str,
    store_id: &str,
) -> bool {
    !binding_enabled
        || (!binding_id.trim().is_empty()
            && !governance_domain.trim().is_empty()
            && !store_id.trim().is_empty())
}

#[cfg(any(target_arch = "wasm32", test))]
pub fn endpoint_upsert_confirmation_matches(dry_run: bool, confirmation_phrase: &str) -> bool {
    dry_run || confirmation_phrase.trim() == ENDPOINT_RECORD_CONFIRMATION
}

#[cfg(any(target_arch = "wasm32", test))]
pub fn endpoint_upsert_review_from_values(
    endpoint_id: &str,
    kind: &str,
    validation_state: &str,
    object_service_url: &str,
    binding_enabled: bool,
    store_id: &str,
) -> String {
    format!(
        "{} · {} · validation {} · {} · {}",
        if endpoint_id.trim().is_empty() {
            "endpoint pending"
        } else {
            endpoint_id.trim()
        },
        kind,
        validation_state,
        object_service_url.trim(),
        if binding_enabled {
            format!("binding to {}", store_id.trim())
        } else {
            "no active binding".to_string()
        }
    )
}

#[cfg(target_arch = "wasm32")]
use crate::api::{
    EndpointBindingUpsertRequest, EndpointInventoryUpsertRequest, EndpointInventoryUpsertResponse,
    EndpointValidationUpsertRequest, EndpointsWorkspaceResponse,
};
#[cfg(target_arch = "wasm32")]
use crate::workspace::ApiLoadState;
#[cfg(target_arch = "wasm32")]
use web_sys::{HtmlInputElement, HtmlSelectElement};
#[cfg(target_arch = "wasm32")]
use yew::prelude::*;

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Debug, Eq, PartialEq, Properties)]
pub struct EndpointsWorkspaceProps {
    pub api_base_path: String,
}

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Debug, Eq, PartialEq)]
struct EndpointUpsertFormState {
    endpoint_id: String,
    display_name: String,
    kind: String,
    object_service_url: String,
    validation_state: String,
    checked_at_utc: String,
    validation_message: String,
    manager_product_id: String,
    binding_enabled: bool,
    binding_id: String,
    governance_domain: String,
    store_id: String,
    binding_readiness: String,
    dry_run: bool,
    confirmation_phrase: String,
    submitting: bool,
    submitted: Option<EndpointInventoryUpsertResponse>,
    error: Option<String>,
}

#[cfg(target_arch = "wasm32")]
impl Default for EndpointUpsertFormState {
    fn default() -> Self {
        Self {
            endpoint_id: String::new(),
            display_name: String::new(),
            kind: "dasobjectstore_nfs".to_string(),
            object_service_url: String::new(),
            validation_state: "pending_validation".to_string(),
            checked_at_utc: String::new(),
            validation_message: String::new(),
            manager_product_id: "dasobjectstore".to_string(),
            binding_enabled: false,
            binding_id: String::new(),
            governance_domain: "local".to_string(),
            store_id: String::new(),
            binding_readiness: "ready".to_string(),
            dry_run: true,
            confirmation_phrase: String::new(),
            submitting: false,
            submitted: None,
            error: None,
        }
    }
}

#[cfg(target_arch = "wasm32")]
impl EndpointUpsertFormState {
    fn clear_result(&mut self) {
        self.submitted = None;
        self.error = None;
    }
}

#[cfg(target_arch = "wasm32")]
#[function_component(EndpointsWorkspace)]
pub fn endpoints_workspace(props: &EndpointsWorkspaceProps) -> Html {
    let api_path = endpoints_workspace_api_path(&props.api_base_path);
    let endpoints_state = use_state(|| ApiLoadState::<EndpointsWorkspaceResponse>::Loading);
    let form_state = use_state(EndpointUpsertFormState::default);

    {
        let api_path = api_path.clone();
        let endpoints_state = endpoints_state.clone();
        use_effect_with(api_path.clone(), move |path| {
            let path = path.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let next = match crate::api::get_endpoints_workspace(&path).await {
                    Ok(view) => ApiLoadState::success(view),
                    Err(error) if error.is_permission_denied() => {
                        ApiLoadState::permission_denied(error.message)
                    }
                    Err(error) => ApiLoadState::transport_error(error.message),
                };
                endpoints_state.set(next);
            });
            || ()
        });
    }

    html! {
        <section class="dos-page dos-endpoints" data-page="endpoints" data-api-route={api_path}>
            <header class="dos-page-header">
                <p>{ "Endpoint inventory" }</p>
                <h1>{ "Endpoints" }</h1>
                <span>{ "DAS, NAS/NFS, S3-compatible, and Mnemosyne-governed storage endpoints." }</span>
            </header>
            { render_endpoints_state(&*endpoints_state, form_state, props.api_base_path.clone()) }
        </section>
    }
}

#[cfg(target_arch = "wasm32")]
fn render_endpoints_state(
    state: &ApiLoadState<EndpointsWorkspaceResponse>,
    form_state: UseStateHandle<EndpointUpsertFormState>,
    api_base_path: String,
) -> Html {
    match state {
        ApiLoadState::Loading => html! {
            <div class="dos-store-grid">
                { render_endpoint_upsert_card(form_state, api_base_path) }
                { render_endpoint_state_message(
                    "Loading",
                    "Loading endpoint inventory",
                    "The Web console is requesting the daemon-backed endpoint registry snapshot.",
                ) }
            </div>
        },
        ApiLoadState::Success(view) | ApiLoadState::StaleData { value: view, .. } => {
            render_endpoint_inventory(view, form_state, api_base_path)
        }
        ApiLoadState::Empty(message) => html! {
            <div class="dos-store-grid">
                { render_endpoint_upsert_card(form_state, api_base_path) }
                { render_endpoint_state_message("Inventory", "No endpoints reported yet", message) }
            </div>
        },
        ApiLoadState::PermissionDenied(message) => render_endpoint_state_message(
            "Permission denied",
            "Endpoint inventory requires an authenticated session",
            message,
        ),
        ApiLoadState::TransportError(message) => {
            render_endpoint_state_message("Error", "Unable to load endpoint inventory", message)
        }
    }
}

#[cfg(target_arch = "wasm32")]
fn render_endpoint_inventory(
    view: &EndpointsWorkspaceResponse,
    form_state: UseStateHandle<EndpointUpsertFormState>,
    api_base_path: String,
) -> Html {
    html! {
        <>
            <div class="dos-metric-grid">
                { for endpoint_inventory_summary_cards(view).into_iter().map(render_endpoint_metric_card) }
            </div>
            <div class="dos-store-grid">
                { render_endpoint_upsert_card(form_state, api_base_path) }
                { for endpoint_card_summaries(view).into_iter().map(render_endpoint_card) }
                if view.inventory.endpoints.is_empty() {
                    { render_endpoint_state_message(
                        "Inventory",
                        "No endpoint records yet",
                        "Create a draft or validated endpoint record to make it visible to Activity and Mneion binding workflows.",
                    ) }
                }
                if !view.inventory.warnings.is_empty() {
                    <section class="dos-card dos-wide-card" data-state="warning">
                        <span class="dos-card-label">{ "Endpoint warnings" }</span>
                        <h2>{ format!("{} warning(s)", view.inventory.warnings.len()) }</h2>
                        { for view.inventory.warnings.iter().take(4).map(|warning| html! {
                            <p>{ format!("{} · {} · {}", warning.severity, warning.code, warning.message) }</p>
                        }) }
                    </section>
                }
            </div>
        </>
    }
}

#[cfg(target_arch = "wasm32")]
fn render_endpoint_metric_card(metric: (String, String, String)) -> Html {
    html! {
        <section class="dos-card dos-metric-card">
            <div class="dos-card-row">
                <span class="dos-card-label">{ metric.0 }</span>
                <span class="dos-status-pill">{ "Inventory" }</span>
            </div>
            <strong>{ metric.1 }</strong>
            <p>{ metric.2 }</p>
        </section>
    }
}

#[cfg(target_arch = "wasm32")]
fn render_endpoint_card(endpoint: EndpointCardSummary) -> Html {
    html! {
        <section class="dos-card dos-store-card" data-endpoint-id={endpoint.id.clone()}>
            <div class="dos-card-row">
                <span class="dos-card-label">{ endpoint.kind.clone() }</span>
                <span class="dos-status-pill">{ endpoint.validation.clone() }</span>
            </div>
            <strong>{ endpoint.name }</strong>
            <p>{ endpoint.object_service_url }</p>
            <p>{ format!("{} · {} · checked {}", endpoint.manager_product_id, endpoint.bindings, endpoint.checked_at) }</p>
            <p>{ format!("{} warning(s) · {}", endpoint.warning_count, endpoint.id) }</p>
        </section>
    }
}

#[cfg(target_arch = "wasm32")]
fn render_endpoint_upsert_card(
    form_state: UseStateHandle<EndpointUpsertFormState>,
    api_base_path: String,
) -> Html {
    let state = (*form_state).clone();
    let fields_ready = endpoint_upsert_fields_ready(
        &state.endpoint_id,
        &state.display_name,
        &state.object_service_url,
    );
    let binding_ready = endpoint_binding_fields_ready(
        state.binding_enabled,
        &state.binding_id,
        &state.governance_domain,
        &state.store_id,
    );
    let confirmation_ready =
        endpoint_upsert_confirmation_matches(state.dry_run, &state.confirmation_phrase);
    let can_submit = fields_ready && binding_ready && confirmation_ready && !state.submitting;
    let submit = {
        let form_state = form_state.clone();
        let api_base_path = api_base_path.clone();
        Callback::from(move |_| {
            let mut pending = (*form_state).clone();
            pending.submitting = true;
            pending.submitted = None;
            pending.error = None;
            form_state.set(pending.clone());

            let form_state = form_state.clone();
            let api_base_path = api_base_path.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let request = endpoint_upsert_request_from_state(&pending);
                let result =
                    crate::api::submit_endpoint_inventory_upsert(&api_base_path, &request).await;
                let mut next = (*form_state).clone();
                next.submitting = false;
                match result {
                    Ok(response) => {
                        next.submitted = Some(response);
                        next.error = None;
                    }
                    Err(error) => {
                        next.submitted = None;
                        next.error = Some(error.message);
                    }
                }
                form_state.set(next);
            });
        })
    };

    html! {
        <section class="dos-card dos-create-card dos-endpoint-upsert" data-action="endpoint_inventory_upsert">
            <span class="dos-create-mark">{ "+" }</span>
            <h2>{ "Create or update endpoint" }</h2>
            <p>{ "Record a validated storage endpoint through dasobjectstored. The daemon persists the registry and Activity receives an endpoint-validation job." }</p>
            <span class="dos-status-pill">{ if state.dry_run { "Dry run" } else { "Live update" } }</span>
            <div class="dos-objectstore-form">
                <div class="dos-form-grid">
                    { endpoint_text_field("Endpoint ID", state.endpoint_id.clone(), {
                        let form_state = form_state.clone();
                        Callback::from(move |event: InputEvent| {
                            update_endpoint_form_from_input(&form_state, event, |state, value| {
                                state.endpoint_id = value;
                            });
                        })
                    }) }
                    { endpoint_text_field("Display name", state.display_name.clone(), {
                        let form_state = form_state.clone();
                        Callback::from(move |event: InputEvent| {
                            update_endpoint_form_from_input(&form_state, event, |state, value| {
                                state.display_name = value;
                            });
                        })
                    }) }
                    <label class="dos-form-field">
                        <span>{ "Endpoint kind" }</span>
                        <select onchange={{
                            let form_state = form_state.clone();
                            Callback::from(move |event: Event| {
                                update_endpoint_form_from_select(&form_state, event, |state, value| {
                                    state.kind = value;
                                });
                            })
                        }} value={state.kind.clone()}>
                            <option value="dasobjectstore_das">{ "DASObjectStore DAS" }</option>
                            <option value="dasobjectstore_nfs">{ "DASObjectStore NAS/NFS" }</option>
                            <option value="s3_compatible">{ "S3 compatible" }</option>
                        </select>
                    </label>
                    { endpoint_text_field("Object-service URL", state.object_service_url.clone(), {
                        let form_state = form_state.clone();
                        Callback::from(move |event: InputEvent| {
                            update_endpoint_form_from_input(&form_state, event, |state, value| {
                                state.object_service_url = value;
                            });
                        })
                    }) }
                    <label class="dos-form-field">
                        <span>{ "Validation state" }</span>
                        <select onchange={{
                            let form_state = form_state.clone();
                            Callback::from(move |event: Event| {
                                update_endpoint_form_from_select(&form_state, event, |state, value| {
                                    state.validation_state = value;
                                });
                            })
                        }} value={state.validation_state.clone()}>
                            <option value="draft">{ "Draft" }</option>
                            <option value="pending_validation">{ "Pending validation" }</option>
                            <option value="validated">{ "Validated" }</option>
                            <option value="degraded">{ "Degraded" }</option>
                            <option value="rejected">{ "Rejected" }</option>
                            <option value="unknown">{ "Unknown" }</option>
                        </select>
                    </label>
                    { endpoint_text_field("Checked at UTC", state.checked_at_utc.clone(), {
                        let form_state = form_state.clone();
                        Callback::from(move |event: InputEvent| {
                            update_endpoint_form_from_input(&form_state, event, |state, value| {
                                state.checked_at_utc = value;
                            });
                        })
                    }) }
                    { endpoint_text_field("Validation message", state.validation_message.clone(), {
                        let form_state = form_state.clone();
                        Callback::from(move |event: InputEvent| {
                            update_endpoint_form_from_input(&form_state, event, |state, value| {
                                state.validation_message = value;
                            });
                        })
                    }) }
                    { endpoint_text_field("Manager product", state.manager_product_id.clone(), {
                        let form_state = form_state.clone();
                        Callback::from(move |event: InputEvent| {
                            update_endpoint_form_from_input(&form_state, event, |state, value| {
                                state.manager_product_id = value;
                            });
                        })
                    }) }
                </div>
                <section class="dos-risk-review">
                    <span class="dos-card-label">{ "Active binding" }</span>
                    <label>
                        <input
                            type="checkbox"
                            checked={state.binding_enabled}
                            onchange={{
                                let form_state = form_state.clone();
                                Callback::from(move |event: Event| {
                                    let input: HtmlInputElement = event.target_unchecked_into();
                                    let mut next = (*form_state).clone();
                                    next.binding_enabled = input.checked();
                                    next.clear_result();
                                    form_state.set(next);
                                })
                            }}
                        />
                        <span>{ "Attach an ObjectStore/governance binding to this endpoint record." }</span>
                    </label>
                    if state.binding_enabled {
                        <div class="dos-form-grid">
                            { endpoint_text_field("Binding ID", state.binding_id.clone(), {
                                let form_state = form_state.clone();
                                Callback::from(move |event: InputEvent| {
                                    update_endpoint_form_from_input(&form_state, event, |state, value| {
                                        state.binding_id = value;
                                    });
                                })
                            }) }
                            { endpoint_text_field("Governance domain", state.governance_domain.clone(), {
                                let form_state = form_state.clone();
                                Callback::from(move |event: InputEvent| {
                                    update_endpoint_form_from_input(&form_state, event, |state, value| {
                                        state.governance_domain = value;
                                    });
                                })
                            }) }
                            { endpoint_text_field("ObjectStore ID", state.store_id.clone(), {
                                let form_state = form_state.clone();
                                Callback::from(move |event: InputEvent| {
                                    update_endpoint_form_from_input(&form_state, event, |state, value| {
                                        state.store_id = value;
                                    });
                                })
                            }) }
                            <label class="dos-form-field">
                                <span>{ "Readiness" }</span>
                                <select onchange={{
                                    let form_state = form_state.clone();
                                    Callback::from(move |event: Event| {
                                        update_endpoint_form_from_select(&form_state, event, |state, value| {
                                            state.binding_readiness = value;
                                        });
                                    })
                                }} value={state.binding_readiness.clone()}>
                                    <option value="ready">{ "Ready" }</option>
                                    <option value="degraded">{ "Degraded" }</option>
                                    <option value="blocked">{ "Blocked" }</option>
                                </select>
                            </label>
                        </div>
                    }
                </section>
                <section class="dos-plan-result">
                    <span class="dos-card-label">{ "Submission review" }</span>
                    <p>{ endpoint_upsert_review_from_values(
                        &state.endpoint_id,
                        &state.kind,
                        &state.validation_state,
                        &state.object_service_url,
                        state.binding_enabled,
                        &state.store_id,
                    ) }</p>
                    <div class="dos-checkbox-list dos-objectstore-flags">
                        <label>
                            <input
                                type="checkbox"
                                checked={state.dry_run}
                                onchange={{
                                    let form_state = form_state.clone();
                                    Callback::from(move |event: Event| {
                                        let input: HtmlInputElement = event.target_unchecked_into();
                                        let mut next = (*form_state).clone();
                                        next.dry_run = input.checked();
                                        next.clear_result();
                                        form_state.set(next);
                                    })
                                }}
                            />
                            <span>{ "Dry run only" }</span>
                        </label>
                    </div>
                    <label class="dos-form-field">
                        <span>{ "Confirmation phrase" }</span>
                        <input
                            placeholder={ENDPOINT_RECORD_CONFIRMATION}
                            value={state.confirmation_phrase.clone()}
                            oninput={{
                                let form_state = form_state.clone();
                                Callback::from(move |event: InputEvent| {
                                    update_endpoint_form_from_input(&form_state, event, |state, value| {
                                        state.confirmation_phrase = value;
                                    });
                                })
                            }}
                        />
                    </label>
                    if let Some(error) = &state.error {
                        <div class="dos-auth-error" role="alert">{ error.clone() }</div>
                    }
                    <button class="dos-auth-submit" type="button" disabled={!can_submit} onclick={submit}>
                        { if state.submitting { "Submitting..." } else if state.dry_run { "Submit dry run" } else { "Record endpoint" } }
                    </button>
                    if let Some(submitted) = &state.submitted {
                        <section class="dos-plan-result" data-job-state="accepted">
                            <span class="dos-card-label">{ "Daemon job accepted" }</span>
                            <h3>{ format!("{} recorded as {}.", submitted.endpoint_id, submitted.validation_state) }</h3>
                            <p>{ format!("Job {} · {} · dry run {}", submitted.accepted.job_id, submitted.accepted.kind, submitted.accepted.dry_run) }</p>
                            <code>{ format!("{} · {} · {}", submitted.display_name, submitted.kind, submitted.registry_path) }</code>
                            if let Some(actor) = &submitted.administrator_actor {
                                <p class="dos-job-message">{ format!("Administrator actor: {actor}") }</p>
                            }
                        </section>
                    }
                </section>
            </div>
        </section>
    }
}

#[cfg(target_arch = "wasm32")]
fn endpoint_upsert_request_from_state(
    state: &EndpointUpsertFormState,
) -> EndpointInventoryUpsertRequest {
    EndpointInventoryUpsertRequest {
        endpoint_id: state.endpoint_id.trim().to_string(),
        display_name: state.display_name.trim().to_string(),
        kind: state.kind.clone(),
        object_service_url: state.object_service_url.trim().to_string(),
        validation: EndpointValidationUpsertRequest {
            state: state.validation_state.clone(),
            checked_at_utc: (!state.checked_at_utc.trim().is_empty())
                .then(|| state.checked_at_utc.trim().to_string()),
            message: (!state.validation_message.trim().is_empty())
                .then(|| state.validation_message.trim().to_string()),
        },
        manager_product_id: state.manager_product_id.trim().to_string(),
        active_bindings: if state.binding_enabled {
            vec![EndpointBindingUpsertRequest {
                binding_id: state.binding_id.trim().to_string(),
                governance_domain: state.governance_domain.trim().to_string(),
                store_id: state.store_id.trim().to_string(),
                readiness: state.binding_readiness.clone(),
            }]
        } else {
            Vec::new()
        },
        dry_run: state.dry_run,
        client_request_id: None,
        confirmation_marker: (!state.confirmation_phrase.trim().is_empty())
            .then(|| state.confirmation_phrase.trim().to_string()),
    }
}

#[cfg(target_arch = "wasm32")]
fn update_endpoint_form_from_input<F>(
    form_state: &UseStateHandle<EndpointUpsertFormState>,
    event: InputEvent,
    update: F,
) where
    F: FnOnce(&mut EndpointUpsertFormState, String),
{
    let input: HtmlInputElement = event.target_unchecked_into();
    let mut next = (**form_state).clone();
    update(&mut next, input.value());
    next.clear_result();
    form_state.set(next);
}

#[cfg(target_arch = "wasm32")]
fn update_endpoint_form_from_select<F>(
    form_state: &UseStateHandle<EndpointUpsertFormState>,
    event: Event,
    update: F,
) where
    F: FnOnce(&mut EndpointUpsertFormState, String),
{
    let input: HtmlSelectElement = event.target_unchecked_into();
    let mut next = (**form_state).clone();
    update(&mut next, input.value());
    next.clear_result();
    form_state.set(next);
}

#[cfg(target_arch = "wasm32")]
fn endpoint_text_field(label: &'static str, value: String, oninput: Callback<InputEvent>) -> Html {
    html! {
        <label class="dos-form-field">
            <span>{ label }</span>
            <input value={value} oninput={oninput} />
        </label>
    }
}

#[cfg(target_arch = "wasm32")]
fn render_endpoint_state_message(label: &str, title: &str, message: &str) -> Html {
    html! {
        <section class="dos-card dos-wide-card">
            <span class="dos-card-label">{ label }</span>
            <h2>{ title }</h2>
            <p>{ message }</p>
        </section>
    }
}

#[cfg(test)]
mod tests {
    use super::{
        endpoint_binding_fields_ready, endpoint_card_summaries, endpoint_inventory_summary_cards,
        endpoint_inventory_upsert_action_api_path, endpoint_upsert_confirmation_matches,
        endpoint_upsert_fields_ready, endpoint_upsert_review_from_values,
        endpoints_workspace_api_path, ENDPOINTS_WORKSPACE_ROUTE, ENDPOINT_INVENTORY_UPSERT_ROUTE,
        ENDPOINT_RECORD_CONFIRMATION,
    };
    use crate::api::{
        EndpointBindingResponse, EndpointInventoryItemResponse, EndpointInventoryResponse,
        EndpointValidationResponse, EndpointsWorkspaceResponse,
    };

    #[test]
    fn builds_endpoints_workspace_api_path() {
        assert_eq!(
            endpoints_workspace_api_path("/products/dasobjectstore/api/v1/"),
            "/products/dasobjectstore/api/v1/workspaces/endpoints"
        );
    }

    #[test]
    fn builds_endpoint_inventory_upsert_action_api_path() {
        assert_eq!(
            endpoint_inventory_upsert_action_api_path("/products/dasobjectstore/api/v1/"),
            "/products/dasobjectstore/api/v1/workspaces/endpoints/upsert"
        );
        assert_eq!(ENDPOINTS_WORKSPACE_ROUTE, "workspaces/endpoints");
        assert_eq!(
            ENDPOINT_INVENTORY_UPSERT_ROUTE,
            "workspaces/endpoints/upsert"
        );
    }

    #[test]
    fn endpoint_upsert_form_gates_required_fields_binding_and_confirmation() {
        assert!(endpoint_upsert_fields_ready(
            "nas-staging",
            "NAS staging",
            "https://nas.example.test:9443"
        ));
        assert!(!endpoint_upsert_fields_ready(
            "nas-staging",
            "",
            "https://nas.example.test:9443"
        ));
        assert!(endpoint_binding_fields_ready(false, "", "", ""));
        assert!(endpoint_binding_fields_ready(
            true,
            "binding-1",
            "local",
            "zymo"
        ));
        assert!(!endpoint_binding_fields_ready(
            true,
            "binding-1",
            "",
            "zymo"
        ));
        assert!(endpoint_upsert_confirmation_matches(true, ""));
        assert!(endpoint_upsert_confirmation_matches(
            false,
            ENDPOINT_RECORD_CONFIRMATION
        ));
        assert!(!endpoint_upsert_confirmation_matches(
            false,
            "record endpoint"
        ));
    }

    #[test]
    fn endpoint_upsert_review_captures_validation_and_binding() {
        let review = endpoint_upsert_review_from_values(
            "nas-staging",
            "dasobjectstore_nfs",
            "validated",
            "https://nas.example.test:9443",
            true,
            "zymo",
        );

        assert_eq!(
            review,
            "nas-staging · dasobjectstore_nfs · validation validated · https://nas.example.test:9443 · binding to zymo"
        );
    }

    #[test]
    fn endpoint_inventory_payload_maps_to_cards_and_metrics() {
        let view = EndpointsWorkspaceResponse {
            inventory: EndpointInventoryResponse {
                schema_version: "dasobjectstore.endpoint_inventory.v1".to_string(),
                endpoint_count: 1,
                degraded_endpoint_count: 0,
                binding_count: 1,
                endpoints: vec![EndpointInventoryItemResponse {
                    endpoint_id: "nas-staging".to_string(),
                    display_name: "NAS staging".to_string(),
                    kind: "dasobjectstore_nfs".to_string(),
                    manager_product_id: "dasobjectstore".to_string(),
                    object_service_url: "https://nas.example.test:9443".to_string(),
                    validation: EndpointValidationResponse {
                        state: "validated".to_string(),
                        checked_at_utc: Some("2026-07-09T00:00:00Z".to_string()),
                        message: Some("validated".to_string()),
                    },
                    active_bindings: vec![EndpointBindingResponse {
                        binding_id: "binding-1".to_string(),
                        governance_domain: "local".to_string(),
                        store_id: "zymo".to_string(),
                        readiness: "ready".to_string(),
                    }],
                    warnings: Vec::new(),
                }],
                warnings: Vec::new(),
            },
        };

        let cards = endpoint_card_summaries(&view);
        let metrics = endpoint_inventory_summary_cards(&view);

        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].id, "nas-staging");
        assert_eq!(cards[0].bindings, "1 binding(s)");
        assert_eq!(cards[0].checked_at, "2026-07-09T00:00:00Z");
        assert_eq!(metrics[0].1, "1");
        assert_eq!(metrics[1].1, "1");
    }
}
