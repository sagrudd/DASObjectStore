use super::*;

#[cfg(target_arch = "wasm32")]
pub(super) fn render_subobject_create_card(
    view: &ObjectStoresPageResponse,
    subobject_state: UseStateHandle<SubObjectFormState>,
    api_base_path: String,
) -> Html {
    let state = (*subobject_state).clone();
    let enabled = view.create_object_store.enabled && !view.stores.is_empty();
    let status = if enabled {
        "Available"
    } else if view.stores.is_empty() {
        "No stores"
    } else {
        "Admin only"
    };
    let detail = if enabled {
        "Create a named SubObject endpoint with parent selection, nested prefix preview, object-type policy, and S3 routing review."
    } else if view.stores.is_empty() {
        "SubObjects require an existing parent ObjectStore."
    } else {
        view.create_object_store
            .blocked_reason
            .as_deref()
            .unwrap_or("Current user must be an administrator to create SubObjects.")
    };
    let can_plan = enabled
        && !state.planning
        && !state.subobject_name.trim().is_empty()
        && ((state.parent_kind == "store" && !state.parent_store_id.trim().is_empty())
            || (state.parent_kind == "subobject"
                && !state.parent_subobject_name.trim().is_empty()))
        && (state.object_type_mode == "inherit" || !state.object_type.trim().is_empty());
    let capacity_limit = state.capacity_limit_bytes.trim();
    let can_plan = can_plan
        && (capacity_limit.is_empty()
            || capacity_limit.parse::<u64>().is_ok_and(|limit| limit > 0));

    let open_form = {
        let subobject_state = subobject_state.clone();
        let initial = SubObjectFormState::from_view(Some(view));
        Callback::from(move |_| {
            let mut next = (*subobject_state).clone();
            if !next.open {
                let mut seeded = initial.clone();
                seeded.open = true;
                next = seeded;
            } else {
                next.open = false;
            }
            subobject_state.set(next);
        })
    };

    let plan = {
        let subobject_state = subobject_state.clone();
        let api_base_path = api_base_path.clone();
        Callback::from(move |_| {
            let mut pending = (*subobject_state).clone();
            pending.planning = true;
            pending.plan = None;
            pending.error = None;
            subobject_state.set(pending.clone());

            let subobject_state = subobject_state.clone();
            let api_base_path = api_base_path.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let request = GuiActionPlanRequest {
                    action: "subobject_create".to_string(),
                    store_id: None,
                    store_class: None,
                    store_copies: None,
                    bucket: None,
                    writer_group: None,
                    ssd_root: (!pending.ssd_root.trim().is_empty())
                        .then(|| pending.ssd_root.trim().to_string()),
                    public: None,
                    writeable: None,
                    capacity_behavior: None,
                    retention: None,
                    endpoint_export_mode: None,
                    subobject_name: Some(pending.subobject_name.trim().to_string()),
                    parent_store_id: (pending.parent_kind == "store")
                        .then(|| pending.parent_store_id.trim().to_string()),
                    parent_subobject_name: (pending.parent_kind == "subobject")
                        .then(|| pending.parent_subobject_name.trim().to_string()),
                    subobject_capacity_limit_bytes: pending
                        .capacity_limit_bytes
                        .trim()
                        .parse::<u64>()
                        .ok(),
                    subobject_object_type: (pending.object_type_mode == "override")
                        .then(|| pending.object_type.clone()),
                    subobject_inherits_object_type: Some(pending.object_type_mode == "inherit"),
                    subobject_s3_routing: Some(pending.s3_routing.clone()),
                    ssd_device: None,
                    hdd_devices: Vec::new(),
                    mount_root: None,
                    filesystem: None,
                    owner: None,
                    allow_format: false,
                    existing_data_acknowledged: false,
                    confirmation_phrase: None,
                };
                let result = crate::api::plan_gui_action(&api_base_path, &request).await;
                let mut next = (*subobject_state).clone();
                next.planning = false;
                match result {
                    Ok(plan) => {
                        next.plan = Some(plan);
                        next.error = None;
                    }
                    Err(error) => {
                        next.plan = None;
                        next.error = Some(error.message);
                    }
                }
                subobject_state.set(next);
            });
        })
    };

    html! {
        <section class="dos-card dos-create-card dos-subobject-create" data-action="subobject_create">
            <span class="dos-create-mark">{ "/" }</span>
            <h2>{ "Create SubObject" }</h2>
            <p>{ detail }</p>
            <span class="dos-status-pill">{ status }</span>
            <button
                class="dos-secondary-action"
                type="button"
                disabled={!enabled}
                onclick={open_form}
            >
                { if state.open { "Close SubObject form" } else { "Define SubObject" } }
            </button>
            if state.open {
                <div class="dos-objectstore-form">
                    <div class="dos-form-grid">
                        { object_store_text_field("SubObject name", state.subobject_name.clone(), {
                            let subobject_state = subobject_state.clone();
                            Callback::from(move |event: InputEvent| {
                                let input: HtmlInputElement = event.target_unchecked_into();
                                let mut next = (*subobject_state).clone();
                                next.subobject_name = input.value();
                                next.reset_plan();
                                subobject_state.set(next);
                            })
                        }) }
                        <label class="dos-form-field">
                            <span>{ "Parent type" }</span>
                            <select onchange={{
                                let subobject_state = subobject_state.clone();
                                Callback::from(move |event: Event| {
                                    let input: HtmlSelectElement = event.target_unchecked_into();
                                    let mut next = (*subobject_state).clone();
                                    next.parent_kind = input.value();
                                    next.reset_plan();
                                    subobject_state.set(next);
                                })
                            }} value={state.parent_kind.clone()}>
                                <option value="store">{ "ObjectStore" }</option>
                                <option value="subobject">{ "SubObject" }</option>
                            </select>
                        </label>
                        if state.parent_kind == "store" {
                            <label class="dos-form-field">
                                <span>{ "Parent ObjectStore" }</span>
                                <select onchange={{
                                    let subobject_state = subobject_state.clone();
                                    Callback::from(move |event: Event| {
                                        let input: HtmlSelectElement = event.target_unchecked_into();
                                        let mut next = (*subobject_state).clone();
                                        next.parent_store_id = input.value();
                                        next.reset_plan();
                                        subobject_state.set(next);
                                    })
                                }} value={state.parent_store_id.clone()}>
                                    { for view.stores.iter().map(|store| html! {
                                        <option value={store.store_id.clone()}>{ format!("{} ({})", store.display_name, store.store_id) }</option>
                                    }) }
                                </select>
                            </label>
                        } else {
                            { object_store_text_field("Parent SubObject", state.parent_subobject_name.clone(), {
                                let subobject_state = subobject_state.clone();
                                Callback::from(move |event: InputEvent| {
                                    let input: HtmlInputElement = event.target_unchecked_into();
                                    let mut next = (*subobject_state).clone();
                                    next.parent_subobject_name = input.value();
                                    next.reset_plan();
                                    subobject_state.set(next);
                                })
                            }) }
                        }
                        { object_store_text_field("Logical capacity bytes (optional)", state.capacity_limit_bytes.clone(), {
                            let subobject_state = subobject_state.clone();
                            Callback::from(move |event: InputEvent| {
                                let input: HtmlInputElement = event.target_unchecked_into();
                                let mut next = (*subobject_state).clone();
                                next.capacity_limit_bytes = input.value();
                                next.reset_plan();
                                subobject_state.set(next);
                            })
                        }) }
                        <label class="dos-form-field">
                            <span>{ "Object type policy" }</span>
                            <select onchange={{
                                let subobject_state = subobject_state.clone();
                                Callback::from(move |event: Event| {
                                    let input: HtmlSelectElement = event.target_unchecked_into();
                                    let mut next = (*subobject_state).clone();
                                    next.object_type_mode = input.value();
                                    next.reset_plan();
                                    subobject_state.set(next);
                                })
                            }} value={state.object_type_mode.clone()}>
                                <option value="inherit">{ "Inherit from parent/import" }</option>
                                <option value="override">{ "Override for this SubObject" }</option>
                            </select>
                        </label>
                        if state.object_type_mode == "override" {
                            <label class="dos-form-field">
                                <span>{ "Object type" }</span>
                                <select onchange={{
                                    let subobject_state = subobject_state.clone();
                                    Callback::from(move |event: Event| {
                                        let input: HtmlSelectElement = event.target_unchecked_into();
                                        let mut next = (*subobject_state).clone();
                                        next.object_type = input.value();
                                        next.reset_plan();
                                        subobject_state.set(next);
                                    })
                                }} value={state.object_type.clone()}>
                                    { for ["naive", "bam", "cram", "pod5", "fastq", "fastq_gz", "fasta", "vcf", "bcf", "gff", "gtf", "ena_sra"].iter().map(|value| html! {
                                        <option value={(*value).to_string()}>{ *value }</option>
                                    }) }
                                </select>
                            </label>
                        }
                        <label class="dos-form-field">
                            <span>{ "S3 routing" }</span>
                            <select onchange={{
                                let subobject_state = subobject_state.clone();
                                Callback::from(move |event: Event| {
                                    let input: HtmlSelectElement = event.target_unchecked_into();
                                    let mut next = (*subobject_state).clone();
                                    next.s3_routing = input.value();
                                    next.reset_plan();
                                    subobject_state.set(next);
                                })
                            }} value={state.s3_routing.clone()}>
                                <option value="inherit_parent">{ "Inherit parent route" }</option>
                                <option value="dedicated_prefix">{ "Dedicated S3 prefix" }</option>
                                <option value="dedicated_bucket">{ "Dedicated bucket" }</option>
                                <option value="disabled">{ "No S3 route" }</option>
                            </select>
                        </label>
                        { object_store_text_field("SSD root", state.ssd_root.clone(), {
                            let subobject_state = subobject_state.clone();
                            Callback::from(move |event: InputEvent| {
                                let input: HtmlInputElement = event.target_unchecked_into();
                                let mut next = (*subobject_state).clone();
                                next.ssd_root = input.value();
                                next.reset_plan();
                                subobject_state.set(next);
                            })
                        }) }
                    </div>
                    <section class="dos-plan-result">
                        <span class="dos-card-label">{ "Registry preview" }</span>
                        <p>{ subobject_registry_preview(&state) }</p>
                        <button class="dos-secondary-action" type="button" disabled={!can_plan} onclick={plan}>
                            { if state.planning { "Planning..." } else { "Review SubObject plan" } }
                        </button>
                        if let Some(error) = &state.error {
                            <div class="dos-auth-error" role="alert">{ error.clone() }</div>
                        }
                        if let Some(plan) = &state.plan {
                            <code>{ plan.argv.join(" ") }</code>
                            <p class="dos-job-message">{ format!("{} · confirmation required: {}", plan.execution, plan.confirmation_required) }</p>
                        }
                    </section>
                </div>
            }
        </section>
    }
}

#[cfg(target_arch = "wasm32")]
pub(super) fn object_store_text_field(
    label: &'static str,
    value: String,
    oninput: Callback<InputEvent>,
) -> Html {
    html! {
        <label class="dos-form-field">
            <span>{ label }</span>
            <input value={value} oninput={oninput} />
        </label>
    }
}

#[cfg(any(target_arch = "wasm32", test))]
pub(super) fn object_store_create_review_from_values(
    store_id: &str,
    object_type: &str,
    required_copies: u8,
    writer_group: &str,
    endpoint_export_mode: &str,
    public: bool,
    enclosure_id: &str,
) -> String {
    format!(
        "{} · type {} · {} copy/copies · writer group {} · enclosure {} · export {} · {} · writeable until locked",
        if store_id.trim().is_empty() {
            "unnamed store"
        } else {
            store_id.trim()
        },
        object_type,
        required_copies,
        if writer_group.trim().is_empty() {
            "pending"
        } else {
            writer_group.trim()
        },
        if enclosure_id.trim().is_empty() {
            "pending"
        } else {
            enclosure_id.trim()
        },
        endpoint_export_mode,
        if public { "public" } else { "private" }
    )
}

#[cfg(target_arch = "wasm32")]
pub(super) fn object_store_create_review(state: &ObjectStoreCreateFormState) -> String {
    object_store_create_review_from_values(
        &state.store_id,
        &state.object_type,
        state.required_copies,
        &state.writer_group,
        &state.endpoint_export_mode,
        state.public,
        &state.enclosure_id,
    )
}

#[cfg(target_arch = "wasm32")]
pub(super) fn render_object_stores_state_message(label: &str, title: &str, message: &str) -> Html {
    html! {
        <section class="dos-card dos-wide-card">
            <span class="dos-card-label">{ label }</span>
            <h2>{ title }</h2>
            <p>{ message }</p>
        </section>
    }
}

pub fn users_groups_summary_cards(view: &UsersGroupsWorkspaceResponse) -> Vec<DashboardMetric> {
    let authority = view
        .current_user
        .as_ref()
        .map(|user| {
            if user.sudo_administrator {
                "sudo administrator".to_string()
            } else {
                "standard local user".to_string()
            }
        })
        .unwrap_or_else(|| "not reported".to_string());
    let username = view
        .current_user
        .as_ref()
        .map(|user| user.username.as_str())
        .unwrap_or("none");

    vec![
        DashboardMetric::new(
            "Authority adapter",
            &view.host_mode,
            format!("{:?}", view.authentication_framework),
            "identity",
        ),
        DashboardMetric::new("Local actor", username, authority, "authority"),
        DashboardMetric::new(
            "Local users",
            view.users.len().to_string(),
            "Local principals surfaced to the appliance access map",
            "local",
        ),
        DashboardMetric::new(
            "OS groups",
            view.groups.len().to_string(),
            "Local groups visible for access evaluation",
            "membership",
        ),
        DashboardMetric::new(
            "Access groups",
            view.writer_groups.len().to_string(),
            format!("Local mapping registry: {}", view.groups_file_path),
            "policy",
        ),
        DashboardMetric::new(
            "Access actions",
            view.operations
                .iter()
                .filter(|operation| operation.enabled)
                .count()
                .to_string(),
            if view.capabilities.administrator_actions_enabled {
                "Local access mapping is available."
            } else {
                "Local access mapping requires sudo-derived authority."
            },
            "readiness",
        ),
    ]
}

#[cfg(target_arch = "wasm32")]
pub const LOCAL_GROUP_ADMIN_CONFIRMATION: &str = "confirm local group administration";

pub fn local_group_create_fields_ready(group_name: &str) -> bool {
    !group_name.trim().is_empty()
}
