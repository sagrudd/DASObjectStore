use super::*;

#[cfg(target_arch = "wasm32")]
use crate::components::{TaskPane, TaskPaneMode};

#[cfg(any(target_arch = "wasm32", test))]
mod jobs;
#[cfg(any(target_arch = "wasm32", test))]
pub(crate) use jobs::*;

#[cfg(target_arch = "wasm32")]
#[function_component(EnclosuresPage)]
pub fn enclosures_page(props: &EnclosuresPageProps) -> Html {
    let api_path = WorkspacePage::Enclosures.api_path(&props.api_base_path);
    let selected_id = use_state(String::new);
    let enclosures_state = use_state(|| ApiLoadState::<EnclosuresPageResponse>::Loading);
    let wizard_state = use_state(EnclosureWizardState::default);
    let pane_mode = use_state(|| TaskPaneMode::Closed);
    let prepare_trigger_ref = use_node_ref();

    {
        let api_path = api_path.clone();
        let enclosures_state = enclosures_state.clone();
        use_effect_with(api_path.clone(), move |path| {
            let path = path.clone();
            wasm_bindgen_futures::spawn_local(async move {
                enclosures_state.set(page_load_state_from_result(
                    crate::api::get_enclosures_dashboard(&path).await,
                    |_| None,
                ));
            });
            || ()
        });
    }

    html! {
        <section class="dos-page" data-page="enclosures" data-api-route={api_path}>
            <PageHeader
                eyebrow="Storage hardware"
                title="Enclosures"
                summary="Physical shelves and landing media grouped for operator review."
            />
            { render_enclosures_state(
                &*enclosures_state,
                selected_id,
                wizard_state,
                pane_mode,
                prepare_trigger_ref,
                props.api_base_path.clone(),
            ) }
        </section>
    }
}

#[cfg(target_arch = "wasm32")]
pub(super) fn render_enclosures_state(
    state: &ApiLoadState<EnclosuresPageResponse>,
    selected_id: UseStateHandle<String>,
    wizard_state: UseStateHandle<EnclosureWizardState>,
    pane_mode: UseStateHandle<TaskPaneMode>,
    prepare_trigger_ref: NodeRef,
    api_base_path: String,
) -> Html {
    match state {
        ApiLoadState::Loading => html! {
            { render_enclosures_state_message(
                "Loading",
                "Loading enclosure inventory",
                "The Web console is requesting daemon-backed DAS enclosure, drive, mount, capacity, and warning state.",
            ) }
        },
        ApiLoadState::Success(view) | ApiLoadState::StaleData { value: view, .. } => {
            render_enclosure_inventory(
                view,
                selected_id,
                wizard_state,
                pane_mode,
                prepare_trigger_ref,
                api_base_path,
            )
        }
        ApiLoadState::Empty(message) => {
            render_enclosures_state_message("Inventory", "No live enclosures reported yet", message)
        }
        ApiLoadState::PermissionDenied(message) => render_enclosures_state_message(
            "Permission denied",
            "Enclosure inventory requires an authenticated session",
            message,
        ),
        ApiLoadState::TransportError(message) => {
            render_enclosures_state_message("Error", "Unable to load enclosure inventory", message)
        }
    }
}

#[cfg(target_arch = "wasm32")]
pub(super) fn render_enclosure_inventory(
    view: &EnclosuresPageResponse,
    selected_id: UseStateHandle<String>,
    wizard_state: UseStateHandle<EnclosureWizardState>,
    pane_mode: UseStateHandle<TaskPaneMode>,
    prepare_trigger_ref: NodeRef,
    api_base_path: String,
) -> Html {
    let candidate_id = if selected_id.is_empty() {
        view.selected_enclosure_id
            .as_deref()
            .or_else(|| {
                view.enclosures
                    .first()
                    .map(|enclosure| enclosure.enclosure_id.as_str())
            })
            .unwrap_or_default()
            .to_string()
    } else {
        (*selected_id).clone()
    };

    let candidate = enclosure_prepare_candidate(view, &candidate_id);
    let can_prepare = view.add_enclosure.enabled
        && candidate
            .as_ref()
            .is_some_and(EnclosurePrepareCandidate::ready);
    let open_prepare = {
        let wizard_state = wizard_state.clone();
        let pane_mode = pane_mode.clone();
        Callback::from(move |_| {
            let mut next = (*wizard_state).clone();
            next.open = true;
            next.error = None;
            clear_enclosure_job_monitor(&mut next);
            wizard_state.set(next);
            pane_mode.set(TaskPaneMode::Create);
        })
    };

    html! {
        <div class="dos-enclosures-registry">
            <div class="dos-enclosures-toolbar" aria-label="Enclosure registry actions">
                <div>
                    <strong>{ format!("{} {}", view.enclosures.len(), if view.enclosures.len() == 1 { "enclosure" } else { "enclosures" }) }</strong>
                    <span>{ "Select an enclosure to inspect hardware evidence and bay state." }</span>
                </div>
                <button
                    ref={prepare_trigger_ref.clone()}
                    type="button"
                    class="dos-enclosures-primary-action"
                    disabled={!can_prepare}
                    onclick={open_prepare}
                >
                    { view.add_enclosure.label.clone() }
                </button>
            </div>
            { render_enclosure_registry_table(view, &*pane_mode, pane_mode.clone(), selected_id.clone()) }
            { render_enclosure_task_surface(
                view,
                &candidate_id,
                candidate,
                wizard_state,
                pane_mode,
                prepare_trigger_ref,
                api_base_path,
            ) }
        </div>
    }
}

#[cfg(target_arch = "wasm32")]
fn render_enclosure_registry_table(
    view: &EnclosuresPageResponse,
    pane_mode: &TaskPaneMode,
    set_pane_mode: UseStateHandle<TaskPaneMode>,
    selected_id: UseStateHandle<String>,
) -> Html {
    html! {
        <div class="dos-enclosures-table-wrap">
            <table class="dos-enclosures-table">
                <thead>
                    <tr>
                        <th scope="col">{ "Enclosure" }</th>
                        <th scope="col">{ "State" }</th>
                        <th scope="col">{ "Drives" }</th>
                        <th scope="col">{ "Capacity" }</th>
                        <th scope="col">{ "Connection" }</th>
                        <th scope="col">{ "Last seen" }</th>
                        <th scope="col"><span class="dos-visually-hidden">{ "Open" }</span></th>
                    </tr>
                </thead>
                <tbody>
                    if view.enclosures.is_empty() {
                        <tr class="dos-enclosures-empty">
                            <td colspan="7">
                                <strong>{ "No enclosures detected" }</strong>
                                <span>{ "Supported hardware will appear here when the daemon reports it." }</span>
                            </td>
                        </tr>
                    }
                    { for view.enclosures.iter().map(|enclosure| {
                        let enclosure_id = enclosure.enclosure_id.clone();
                        let open = matches!(pane_mode, TaskPaneMode::Edit(context) if context == &format!("inspect:{enclosure_id}"));
                        let open_enclosure = {
                            let set_pane_mode = set_pane_mode.clone();
                            let selected_id = selected_id.clone();
                            let enclosure_id = enclosure_id.clone();
                            Callback::from(move |_| {
                                selected_id.set(enclosure_id.clone());
                                set_pane_mode.set(TaskPaneMode::Edit(format!("inspect:{enclosure_id}")));
                            })
                        };
                        html! {
                            <tr data-selected={open.to_string()} data-enclosure-id={enclosure.enclosure_id.clone()}>
                                <th scope="row">
                                    <button
                                        type="button"
                                        class="dos-enclosures-name"
                                        aria-expanded={open.to_string()}
                                        aria-controls="dos-task-pane-title"
                                        onclick={open_enclosure.clone()}
                                    >
                                        <strong>{ enclosure.display_name.clone() }</strong>
                                        <span>{ enclosure.enclosure_id.clone() }</span>
                                    </button>
                                </th>
                                <td><span class="dos-enclosures-state" data-state={enclosure.health.clone()}>{ labelize_state(&enclosure.health) }</span></td>
                                <td>{ format!("{} / {} mounted", enclosure.drive_count.mounted, enclosure.drive_count.total) }</td>
                                <td>{ format!("{} TiB free / {} TiB", enclosure.capacity.free_tib, enclosure.capacity.total_tib) }</td>
                                <td>{ format!("{} · {} · {}", enclosure.connection.bus, enclosure.connection.protocol, enclosure.connection.link_speed) }</td>
                                <td>{ enclosure.last_seen_at_utc.clone() }</td>
                                <td><button type="button" class="dos-enclosures-open" onclick={open_enclosure}>{ "Open" }</button></td>
                            </tr>
                        }
                    }) }
                </tbody>
            </table>
        </div>
    }
}

#[cfg(target_arch = "wasm32")]
#[allow(clippy::too_many_arguments)]
fn render_enclosure_task_surface(
    view: &EnclosuresPageResponse,
    candidate_id: &str,
    candidate: Option<EnclosurePrepareCandidate>,
    wizard_state: UseStateHandle<EnclosureWizardState>,
    pane_mode: UseStateHandle<TaskPaneMode>,
    prepare_trigger_ref: NodeRef,
    api_base_path: String,
) -> Html {
    let close = {
        let wizard_state = wizard_state.clone();
        let pane_mode = pane_mode.clone();
        Callback::<()>::from(move |_| {
            let mut next = (*wizard_state).clone();
            next.open = false;
            wizard_state.set(next);
            pane_mode.set(TaskPaneMode::Closed);
        })
    };

    match &*pane_mode {
        TaskPaneMode::Closed | TaskPaneMode::Review => Html::default(),
        TaskPaneMode::Create => html! {
            <TaskPane
                mode={TaskPaneMode::Create}
                title={view.add_enclosure.label.clone()}
                selected_context={Some(format!("Daemon-planned preparation · {candidate_id}"))}
                return_focus_to={Some(prepare_trigger_ref)}
                on_close={close}
            >
                { render_enclosure_wizard(candidate, wizard_state, api_base_path) }
            </TaskPane>
        },
        TaskPaneMode::Edit(context) => {
            let enclosure_id = context.trim_start_matches("inspect:");
            let Some(enclosure) = view
                .enclosures
                .iter()
                .find(|enclosure| enclosure.enclosure_id == enclosure_id)
            else {
                return Html::default();
            };
            let detail = view
                .details
                .as_ref()
                .filter(|detail| detail.enclosure_id == enclosure_id);
            let can_prepare = view.add_enclosure.enabled
                && candidate
                    .as_ref()
                    .is_some_and(EnclosurePrepareCandidate::ready);
            let open_prepare = {
                let wizard_state = wizard_state.clone();
                let pane_mode = pane_mode.clone();
                Callback::from(move |_| {
                    let mut next = (*wizard_state).clone();
                    next.open = true;
                    next.error = None;
                    clear_enclosure_job_monitor(&mut next);
                    wizard_state.set(next);
                    pane_mode.set(TaskPaneMode::Create);
                })
            };
            html! {
                <TaskPane
                    mode={TaskPaneMode::Edit(enclosure_id.to_string())}
                    title={enclosure.display_name.clone()}
                    selected_context={Some(enclosure.enclosure_id.clone())}
                    return_focus_to={Some(prepare_trigger_ref)}
                    on_close={close}
                >
                    <div class="dos-enclosures-detail">
                        { if let Some(detail) = detail {
                            render_enclosure_detail(enclosure, detail)
                        } else {
                            render_enclosure_summary_detail(enclosure)
                        } }
                        <div class="dos-enclosures-pane-actions">
                            <button type="button" disabled={!can_prepare} onclick={open_prepare}>
                                { view.add_enclosure.label.clone() }
                            </button>
                        </div>
                    </div>
                </TaskPane>
            }
        }
    }
}

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct EnclosureWizardState {
    pub(super) open: bool,
    pub(super) selected_ssd: String,
    pub(super) selected_hdds: Vec<String>,
    pub(super) mount_root: String,
    pub(super) filesystem: String,
    pub(super) owner: String,
    pub(super) allow_format: bool,
    pub(super) existing_data_acknowledged: bool,
    pub(super) confirmation_phrase: String,
    pub(super) submitting: bool,
    pub(super) job: Option<EnclosurePrepareResponse>,
    pub(super) job_status: Option<AdminJobStatusResponse>,
    pub(super) job_polling: bool,
    pub(super) job_status_error: Option<String>,
    pub(super) cancelling: bool,
    pub(super) cancellation: Option<AdminJobCancelResponse>,
    pub(super) cancel_error: Option<String>,
    pub(super) error: Option<String>,
}

#[cfg(any(target_arch = "wasm32", test))]
impl Default for EnclosureWizardState {
    fn default() -> Self {
        Self {
            open: false,
            selected_ssd: String::new(),
            selected_hdds: Vec::new(),
            mount_root: "/srv/dasobjectstore".to_string(),
            filesystem: "ext4".to_string(),
            owner: String::new(),
            allow_format: false,
            existing_data_acknowledged: false,
            confirmation_phrase: String::new(),
            submitting: false,
            job: None,
            job_status: None,
            job_polling: false,
            job_status_error: None,
            cancelling: false,
            cancellation: None,
            cancel_error: None,
            error: None,
        }
    }
}

#[cfg(target_arch = "wasm32")]
pub(super) fn render_enclosure_wizard(
    candidate: Option<EnclosurePrepareCandidate>,
    wizard_state: UseStateHandle<EnclosureWizardState>,
    api_base_path: String,
) -> Html {
    let state = (*wizard_state).clone();
    if !state.open {
        return html! {};
    }
    let Some(candidate) = candidate else {
        return html! {};
    };

    let selected_ssd = if state.selected_ssd.is_empty() {
        candidate
            .ssd_devices
            .first()
            .map(|device| device.device_path.clone())
            .unwrap_or_default()
    } else {
        state.selected_ssd.clone()
    };
    let selected_hdds = if state.selected_hdds.is_empty() {
        candidate
            .hdd_devices
            .iter()
            .map(|device| device.device_path.clone())
            .collect::<Vec<_>>()
    } else {
        state.selected_hdds.clone()
    };
    let selected_hdd_devices = selected_hdds
        .iter()
        .filter_map(|path| {
            candidate
                .hdd_devices
                .iter()
                .find(|device| &device.device_path == path)
                .map(|device| EnclosurePrepareHddDevice {
                    disk_id: device.disk_id.clone(),
                    device_path: device.device_path.clone(),
                })
        })
        .collect::<Vec<_>>();
    let confirmed = enclosure_prepare_confirmed(
        state.allow_format,
        state.existing_data_acknowledged,
        &state.confirmation_phrase,
    );
    let can_submit = !state.submitting
        && state.job.is_none()
        && !selected_ssd.is_empty()
        && !selected_hdds.is_empty()
        && confirmed;

    let on_ssd = {
        let wizard_state = wizard_state.clone();
        Callback::from(move |event: Event| {
            let input: HtmlSelectElement = event.target_unchecked_into();
            let mut next = (*wizard_state).clone();
            next.selected_ssd = input.value();
            clear_enclosure_job_monitor(&mut next);
            wizard_state.set(next);
        })
    };
    let on_mount_root = string_input_callback(wizard_state.clone(), |state, value| {
        state.mount_root = value;
    });
    let on_filesystem = string_change_callback(wizard_state.clone(), |state, value| {
        state.filesystem = value;
    });
    let on_owner = string_input_callback(wizard_state.clone(), |state, value| {
        state.owner = value;
    });
    let on_confirmation = string_input_callback(wizard_state.clone(), |state, value| {
        state.confirmation_phrase = value;
    });
    let on_allow_format = {
        let wizard_state = wizard_state.clone();
        Callback::from(move |event: Event| {
            let input: HtmlInputElement = event.target_unchecked_into();
            let mut next = (*wizard_state).clone();
            next.allow_format = input.checked();
            clear_enclosure_job_monitor(&mut next);
            wizard_state.set(next);
        })
    };
    let on_existing_data_acknowledged = {
        let wizard_state = wizard_state.clone();
        Callback::from(move |event: Event| {
            let input: HtmlInputElement = event.target_unchecked_into();
            let mut next = (*wizard_state).clone();
            next.existing_data_acknowledged = input.checked();
            clear_enclosure_job_monitor(&mut next);
            wizard_state.set(next);
        })
    };
    let submit = {
        let wizard_state = wizard_state.clone();
        let api_base_path = api_base_path.clone();
        let selected_ssd = selected_ssd.clone();
        let selected_hdds = selected_hdds.clone();
        let selected_hdd_devices = selected_hdd_devices.clone();
        Callback::from(move |_| {
            let mut pending = (*wizard_state).clone();
            pending.submitting = true;
            pending.error = None;
            clear_enclosure_job_monitor(&mut pending);
            wizard_state.set(pending.clone());

            let wizard_state = wizard_state.clone();
            let api_base_path = api_base_path.clone();
            let selected_ssd = selected_ssd.clone();
            let selected_hdds = selected_hdds.clone();
            let selected_hdd_devices = selected_hdd_devices.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let request = EnclosurePrepareRequest {
                    ssd_device: selected_ssd,
                    hdd_devices: selected_hdd_devices,
                    mount_root: (!pending.mount_root.trim().is_empty())
                        .then(|| pending.mount_root.clone()),
                    filesystem: (!pending.filesystem.trim().is_empty())
                        .then(|| pending.filesystem.clone()),
                    owner: (!pending.owner.trim().is_empty()).then(|| pending.owner.clone()),
                    dry_run: false,
                    client_request_id: None,
                    allow_format: pending.allow_format,
                    existing_data_acknowledged: pending.existing_data_acknowledged,
                    confirmation_marker: Some(pending.confirmation_phrase.clone()),
                };
                let selected_ssd = request.ssd_device.clone();
                let result = crate::api::submit_enclosure_prepare(&api_base_path, &request).await;
                let mut next = (*wizard_state).clone();
                next.submitting = false;
                match result {
                    Ok(job) => {
                        let job_id = job.accepted.job_id.clone();
                        next.selected_ssd = selected_ssd;
                        next.selected_hdds = selected_hdds;
                        next.job = Some(job);
                        next.job_status = None;
                        next.job_polling = true;
                        next.job_status_error = None;
                        next.cancellation = None;
                        next.cancel_error = None;
                        next.error = None;
                        wizard_state.set(next);
                        schedule_enclosure_job_status_poll(
                            api_base_path.clone(),
                            wizard_state.clone(),
                            job_id,
                            0,
                        );
                        return;
                    }
                    Err(error) => {
                        clear_enclosure_job_monitor(&mut next);
                        next.error = Some(error.message);
                    }
                }
                wizard_state.set(next);
            });
        })
    };

    html! {
        <section class="dos-enclosure-wizard" data-workflow="enclosure_add">
            <header class="dos-wizard-header">
                <span class="dos-card-label">{ "Preparation wizard" }</span>
                <h3>{ format!("Prepare {}", candidate.display_name) }</h3>
            </header>
            <ol class="dos-wizard-steps">
                <li>{ "Supported DAS detected from daemon inventory." }</li>
                <li>{ "Select SSD landing media and HDD settlement media." }</li>
                <li>{ "Review the destructive format plan." }</li>
                <li>{ "Submit the daemon-owned preparation plan." }</li>
            </ol>
            <label class="dos-form-field">
                <span>{ "SSD landing device" }</span>
                <select onchange={on_ssd} value={selected_ssd.clone()}>
                    { for candidate.ssd_devices.iter().map(|device| html! {
                        <option value={device.device_path.clone()}>{ device.label.clone() }</option>
                    }) }
                </select>
            </label>
            <div class="dos-form-field">
                <span>{ "HDD settlement devices" }</span>
                <div class="dos-checkbox-list">
                    { for candidate.hdd_devices.iter().map(|device| {
                        let checked = selected_hdds.contains(&device.device_path);
                        let device_path = device.device_path.clone();
                        let wizard_state = wizard_state.clone();
                        html! {
                            <label>
                                <input
                                    type="checkbox"
                                    checked={checked}
                                    onchange={Callback::from(move |event: Event| {
                                        let input: HtmlInputElement = event.target_unchecked_into();
                                        let mut next = (*wizard_state).clone();
                                        if input.checked() {
                                            if !next.selected_hdds.contains(&device_path) {
                                                next.selected_hdds.push(device_path.clone());
                                            }
                                        } else {
                                            next.selected_hdds.retain(|path| path != &device_path);
                                        }
                                        clear_enclosure_job_monitor(&mut next);
                                        wizard_state.set(next);
                                    })}
                                />
                                <span>{ device.label.clone() }</span>
                            </label>
                        }
                    }) }
                </div>
            </div>
            <div class="dos-form-grid">
                <label class="dos-form-field">
                    <span>{ "Mount root" }</span>
                    <input value={state.mount_root.clone()} oninput={on_mount_root} />
                </label>
                <label class="dos-form-field">
                    <span>{ "Filesystem" }</span>
                    <select onchange={on_filesystem} value={state.filesystem.clone()}>
                        <option value="ext4">{ "ext4" }</option>
                        <option value="xfs">{ "xfs" }</option>
                    </select>
                </label>
                <label class="dos-form-field">
                    <span>{ "Mounted-root owner" }</span>
                    <input placeholder="optional local user" value={state.owner.clone()} oninput={on_owner} />
                </label>
            </div>
            <section class="dos-risk-review">
                <span class="dos-card-label">{ "Data-loss review" }</span>
                <p>{ "Preparing this enclosure formats the selected SSD and HDD devices, creates DASObjectStore mount roots, and delegates execution to the daemon-side storage authority." }</p>
                <label>
                    <input type="checkbox" checked={state.allow_format} onchange={on_allow_format} />
                    <span>{ "I allow formatting of the selected devices." }</span>
                </label>
                <label>
                    <input type="checkbox" checked={state.existing_data_acknowledged} onchange={on_existing_data_acknowledged} />
                    <span>{ "I acknowledge existing data on selected devices may be destroyed." }</span>
                </label>
                <label class="dos-form-field">
                    <span>{ "Confirmation phrase" }</span>
                    <input
                        placeholder="confirm prepare das"
                        value={state.confirmation_phrase.clone()}
                        oninput={on_confirmation}
                    />
                </label>
            </section>
            if let Some(error) = &state.error {
                <div class="dos-auth-error" role="alert">{ error.clone() }</div>
            }
            { render_enclosure_job_monitor(&state, wizard_state.clone(), api_base_path.clone()) }
            <button class="dos-auth-submit" type="button" disabled={!can_submit} onclick={submit}>
                { if state.submitting { "Submitting..." } else { "Submit preparation job" } }
            </button>
        </section>
    }
}

#[cfg(target_arch = "wasm32")]
pub(super) fn render_enclosure_job_monitor(
    state: &EnclosureWizardState,
    wizard_state: UseStateHandle<EnclosureWizardState>,
    api_base_path: String,
) -> Html {
    let Some(job) = &state.job else {
        return html! {};
    };
    let job_id = job.accepted.job_id.clone();
    let latest = state.job_status.as_ref().map(|status| &status.job);
    let status_label = latest
        .map(|job| job.state.clone())
        .unwrap_or_else(|| "accepted".to_string());
    let terminal = latest.is_some_and(|job| admin_job_state_is_terminal(&job.state));
    let can_cancel = !terminal && !state.cancelling;
    let can_refresh = !state.job_polling && !state.cancelling;
    let refresh = {
        let wizard_state = wizard_state.clone();
        let api_base_path = api_base_path.clone();
        let job_id = job_id.clone();
        Callback::from(move |_| {
            let mut next = (*wizard_state).clone();
            if enclosure_wizard_job_id(&next).as_deref() != Some(job_id.as_str()) {
                return;
            }
            next.job_polling = true;
            next.job_status_error = None;
            wizard_state.set(next);
            schedule_enclosure_job_status_poll(
                api_base_path.clone(),
                wizard_state.clone(),
                job_id.clone(),
                0,
            );
        })
    };
    let cancel = {
        let wizard_state = wizard_state.clone();
        let api_base_path = api_base_path.clone();
        let job_id = job_id.clone();
        Callback::from(move |_| {
            let mut pending = (*wizard_state).clone();
            if enclosure_wizard_job_id(&pending).as_deref() != Some(job_id.as_str()) {
                return;
            }
            pending.cancelling = true;
            pending.cancel_error = None;
            wizard_state.set(pending);

            let wizard_state = wizard_state.clone();
            let api_base_path = api_base_path.clone();
            let job_id = job_id.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let request = AdminJobCancelRequest {
                    reason: Some("cancelled from Enclosures Web preparation wizard".to_string()),
                };
                let result = crate::api::cancel_admin_job(&api_base_path, &job_id, &request).await;
                let mut next = (*wizard_state).clone();
                if enclosure_wizard_job_id(&next).as_deref() != Some(job_id.as_str()) {
                    return;
                }
                next.cancelling = false;
                match result {
                    Ok(cancelled) => {
                        next.cancellation = Some(cancelled);
                        next.cancel_error = None;
                        next.job_polling = true;
                        wizard_state.set(next);
                        schedule_enclosure_job_status_poll(api_base_path, wizard_state, job_id, 0);
                    }
                    Err(error) => {
                        next.cancel_error = Some(error.message);
                        wizard_state.set(next);
                    }
                }
            });
        })
    };
    let retry = {
        let wizard_state = wizard_state.clone();
        Callback::from(move |_| {
            let mut next = (*wizard_state).clone();
            enclosure_retry_clears_job_state(&mut next);
            wizard_state.set(next);
        })
    };

    html! {
        <section class="dos-plan-result" data-job-state={status_label.clone()}>
            <span class="dos-card-label">{ "Daemon job" }</span>
            <h3>{ admin_job_monitor_title(latest, state.job_status_error.as_deref()) }</h3>
            <p>{ format!("Job {} · {} · dry run {}", job.accepted.job_id, job.accepted.kind, job.accepted.dry_run) }</p>
            <code>{ format!("{} -> {} HDD device(s)", job.ssd_device, job.hdd_devices.len()) }</code>
            { render_admin_job_progress(latest) }
            <div class="dos-job-meta">
                <span>{ format!("State: {status_label}") }</span>
                <span>{ format!("Submitted: {}", latest.map(|job| job.submitted_at_utc.as_str()).unwrap_or(job.accepted.accepted_at_utc.as_str())) }</span>
                <span>{ format!("Updated: {}", latest.map(|job| job.updated_at_utc.as_str()).unwrap_or("pending")) }</span>
                <span>{ if state.job_polling { "Status: polling daemon" } else { "Status: current" } }</span>
            </div>
            if let Some(message) = &state.job_status_error {
                <div class="dos-auth-error" role="alert">{ format!("Status refresh failed: {message}") }</div>
            }
            if let Some(message) = &state.cancel_error {
                <div class="dos-auth-error" role="alert">{ format!("Cancellation failed: {message}") }</div>
            }
            if let Some(cancelled) = &state.cancellation {
                <p class="dos-job-message">{ format!("Cancellation request {} with daemon state {}.", if cancelled.accepted { "accepted" } else { "not accepted" }, cancelled.state) }</p>
            }
            <div class="dos-job-actions">
                <button type="button" onclick={refresh} disabled={!can_refresh}>
                    { if state.job_polling { "Refreshing..." } else { "Refresh status" } }
                </button>
                <button type="button" onclick={cancel} disabled={!can_cancel}>
                    { if state.cancelling { "Cancelling..." } else { "Cancel job" } }
                </button>
                <button type="button" onclick={retry} disabled={!terminal && state.job_status_error.is_none()}>
                    { "Retry preparation" }
                </button>
            </div>
        </section>
    }
}

#[cfg(target_arch = "wasm32")]
pub(super) fn admin_job_monitor_title(
    job: Option<&AdminJobSummary>,
    status_error: Option<&str>,
) -> String {
    if status_error.is_some() {
        return "Preparation status needs attention.".to_string();
    }
    match job.map(|job| job.state.as_str()) {
        Some("complete") => "Preparation completed by dasobjectstored.".to_string(),
        Some("failed") => "Preparation failed in dasobjectstored.".to_string(),
        Some("cancelled") => "Preparation cancelled.".to_string(),
        Some("running") => "Preparation is running.".to_string(),
        Some("waiting") => "Preparation is waiting.".to_string(),
        Some("queued") => "Preparation is queued.".to_string(),
        Some(_) => "Preparation state reported by dasobjectstored.".to_string(),
        None => "Preparation submitted to dasobjectstored.".to_string(),
    }
}

#[cfg(target_arch = "wasm32")]
pub(super) fn render_admin_job_progress(job: Option<&AdminJobSummary>) -> Html {
    let Some(job) = job else {
        return html! {
            <div class="dos-job-progress">
                <div class="dos-job-progress-bar"><span style="width: 0%"></span></div>
                <p>{ "Waiting for daemon progress." }</p>
            </div>
        };
    };
    let percent = admin_job_percent(job);
    let width = format!("width: {}%", percent.unwrap_or(0));
    html! {
        <div class="dos-job-progress">
            <div class="dos-job-progress-bar" aria-label="Administrator job progress">
                <span style={width}></span>
            </div>
            <p>
                { format!(
                    "{} · {} · {}",
                    percent.map(|value| format!("{value}%")).unwrap_or_else(|| "Percent pending".to_string()),
                    job.progress.stage,
                    admin_job_progress_text(job)
                ) }
            </p>
            if let Some(message) = &job.progress.message {
                <p class="dos-job-message">{ message.clone() }</p>
            }
            if let Some(message) = &job.failure_message {
                <div class="dos-auth-error" role="alert">{ message.clone() }</div>
            }
        </div>
    }
}

#[cfg(target_arch = "wasm32")]
pub(super) fn string_input_callback<F>(
    wizard_state: UseStateHandle<EnclosureWizardState>,
    update: F,
) -> Callback<InputEvent>
where
    F: Fn(&mut EnclosureWizardState, String) + 'static,
{
    Callback::from(move |event: InputEvent| {
        let input: HtmlInputElement = event.target_unchecked_into();
        let mut next = (*wizard_state).clone();
        update(&mut next, input.value());
        clear_enclosure_job_monitor(&mut next);
        wizard_state.set(next);
    })
}

#[cfg(target_arch = "wasm32")]
pub(super) fn string_change_callback<F>(
    wizard_state: UseStateHandle<EnclosureWizardState>,
    update: F,
) -> Callback<Event>
where
    F: Fn(&mut EnclosureWizardState, String) + 'static,
{
    Callback::from(move |event: Event| {
        let input: HtmlSelectElement = event.target_unchecked_into();
        let mut next = (*wizard_state).clone();
        update(&mut next, input.value());
        clear_enclosure_job_monitor(&mut next);
        wizard_state.set(next);
    })
}

#[cfg(target_arch = "wasm32")]
pub(super) fn render_enclosure_detail(
    enclosure: &DasEnclosureCardResponse,
    detail: &DasEnclosureDetailResponse,
) -> Html {
    html! {
        <>
            <section>
                <h3>{ "Hardware" }</h3>
                <dl class="dos-detail-list">
                    <div><dt>{ "Vendor" }</dt><dd>{ &detail.vendor }</dd></div>
                    <div><dt>{ "Model" }</dt><dd>{ &detail.model }</dd></div>
                    <div><dt>{ "Serial" }</dt><dd>{ &detail.serial }</dd></div>
                    <div><dt>{ "Firmware" }</dt><dd>{ detail.firmware.as_deref().unwrap_or("unknown") }</dd></div>
                </dl>
            </section>
            <section>
                <h3>{ "Connection and capacity" }</h3>
                <dl class="dos-detail-list">
                    <div><dt>{ "Mount" }</dt><dd>{ &enclosure.mount_path }</dd></div>
                    <div><dt>{ "Connection" }</dt><dd>{ format!("{} / {} / {}", enclosure.connection.bus, enclosure.connection.protocol, enclosure.connection.link_speed) }</dd></div>
                    <div><dt>{ "Capacity" }</dt><dd>{ format!("{} TiB free of {} TiB", enclosure.capacity.free_tib, enclosure.capacity.total_tib) }</dd></div>
                    <div><dt>{ "Last seen" }</dt><dd>{ &enclosure.last_seen_at_utc }</dd></div>
                </dl>
            </section>
            <section>
                <h3>{ format!("Drive bays · {}", detail.slots.len()) }</h3>
                <div class="dos-slot-list">
                    { for detail.slots.iter().map(render_drive_slot_card) }
                </div>
            </section>
            { render_enclosure_warnings(enclosure) }
        </>
    }
}

#[cfg(target_arch = "wasm32")]
fn render_enclosure_warnings(enclosure: &DasEnclosureCardResponse) -> Html {
    html! {
        <section>
            <h3>{ format!("Warnings · {}", enclosure.warnings.len()) }</h3>
            if enclosure.warnings.is_empty() {
                <p class="dos-enclosures-clear-state">{ "No enclosure warnings are currently reported." }</p>
            } else {
                <ul class="dos-enclosures-warning-list">
                    { for enclosure.warnings.iter().map(|warning| html! {
                        <li><strong>{ labelize_state(&warning.code) }</strong><span>{ warning.message.clone() }</span></li>
                    }) }
                </ul>
            }
        </section>
    }
}

#[cfg(target_arch = "wasm32")]
pub(super) fn render_drive_slot_card(slot: &EnclosureDriveSlotResponse) -> Html {
    let bay_label = if slot.slot_number == 0 {
        "SSD".to_string()
    } else {
        format!("Bay {}", slot.slot_number)
    };
    let role = slot.role.as_deref().unwrap_or("unassigned");
    let mount = slot
        .mount_path
        .as_deref()
        .unwrap_or("mount path unavailable");
    let filesystem = slot.filesystem.as_deref().unwrap_or("filesystem unknown");
    let device = slot.device_path.as_deref().unwrap_or("device unknown");
    let actions = if slot.actions_available.is_empty() {
        "Actions unavailable".to_string()
    } else {
        slot.actions_available.join(", ")
    };

    html! {
        <article class="dos-drive-card">
            <div class="dos-card-row">
                <span class="dos-card-label">{ bay_label }</span>
                <span class="dos-status-pill">{ &slot.health }</span>
            </div>
            <strong>{ &slot.drive_id }</strong>
            <div class="dos-drive-meta">
                <span>{ format!("Role: {}", role) }</span>
                <span>{ format!("Capacity: {} TiB", slot.size_tib) }</span>
                <span>{ if slot.mounted { "Mounted" } else { "Not mounted" } }</span>
                <span>{ format!("SMART warnings: {}", slot.smart_warning_count) }</span>
            </div>
            <dl class="dos-drive-detail-list">
                <div><dt>{ "Mount" }</dt><dd>{ mount }</dd></div>
                <div><dt>{ "Device" }</dt><dd>{ device }</dd></div>
                <div><dt>{ "Filesystem" }</dt><dd>{ filesystem }</dd></div>
                <div><dt>{ "Actions" }</dt><dd>{ actions }</dd></div>
            </dl>
        </article>
    }
}

#[cfg(target_arch = "wasm32")]
pub(super) fn render_enclosure_summary_detail(enclosure: &DasEnclosureCardResponse) -> Html {
    html! {
        <>
            <section>
                <h3>{ "Overview" }</h3>
                <dl class="dos-detail-list">
                    <div><dt>{ "Health" }</dt><dd>{ labelize_state(&enclosure.health) }</dd></div>
                    <div><dt>{ "Mount" }</dt><dd>{ &enclosure.mount_path }</dd></div>
                    <div><dt>{ "Connection" }</dt><dd>{ format!("{} / {} / {}", enclosure.connection.bus, enclosure.connection.protocol, enclosure.connection.link_speed) }</dd></div>
                    <div><dt>{ "Drives" }</dt><dd>{ format!("{} mounted of {}", enclosure.drive_count.mounted, enclosure.drive_count.total) }</dd></div>
                    <div><dt>{ "Capacity" }</dt><dd>{ format!("{} TiB free of {} TiB", enclosure.capacity.free_tib, enclosure.capacity.total_tib) }</dd></div>
                    <div><dt>{ "Last seen" }</dt><dd>{ &enclosure.last_seen_at_utc }</dd></div>
                </dl>
            </section>
            { render_enclosure_warnings(enclosure) }
        </>
    }
}

#[cfg(target_arch = "wasm32")]
pub(super) fn render_enclosures_state_message(label: &str, title: &str, message: &str) -> Html {
    html! {
        <section class="dos-card dos-wide-card">
            <span class="dos-card-label">{ label }</span>
            <h2>{ title }</h2>
            <p>{ message }</p>
        </section>
    }
}

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Debug, PartialEq, Properties)]
pub struct ObjectStoresPageProps {
    pub api_base_path: String,
    pub on_upload_target: Callback<String>,
}
