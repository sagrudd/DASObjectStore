use super::*;

#[cfg(target_arch = "wasm32")]
pub(super) fn users_groups_empty_workspace_message(
    view: &UsersGroupsWorkspaceResponse,
) -> Option<String> {
    (view.current_user.is_none() && view.users.is_empty() && view.writer_groups.is_empty()).then(
        || "No local identity or writer-policy state was returned by the appliance.".to_string(),
    )
}

#[cfg(any(target_arch = "wasm32", test))]
pub(super) fn local_group_display_name(group_name: &str) -> String {
    let display_name = group_name
        .trim()
        .replace(['-', '_'], " ")
        .split_whitespace()
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => first
                    .to_uppercase()
                    .chain(chars.flat_map(char::to_lowercase))
                    .collect(),
                None => String::new(),
            }
        })
        .collect::<Vec<String>>()
        .join(" ");

    if display_name.is_empty() {
        group_name.trim().to_string()
    } else {
        display_name
    }
}

#[cfg(any(target_arch = "wasm32", test))]
pub(super) fn users_groups_view_with_writer_group(
    mut view: UsersGroupsWorkspaceResponse,
    group_name: &str,
) -> UsersGroupsWorkspaceResponse {
    let group_name = group_name.trim();
    if group_name.is_empty() {
        return view;
    }

    let current_user_member = view
        .current_user
        .as_ref()
        .map(|user| user.groups.iter().any(|group| group == group_name))
        .unwrap_or(false);

    if let Some(group) = view
        .writer_groups
        .iter_mut()
        .find(|group| group.group_name == group_name)
    {
        group.current_user_member |= current_user_member;
    } else {
        view.writer_groups.push(crate::api::StorageGroupResponse {
            group_name: group_name.to_string(),
            display_name: local_group_display_name(group_name),
            source: "object_storage_group_registry".to_string(),
            current_user_member,
        });
    }

    view.writer_groups
        .sort_by(|left, right| left.display_name.cmp(&right.display_name));
    view.selected_group_name = Some(group_name.to_string());
    view
}

#[cfg(any(target_arch = "wasm32", test))]
pub(super) fn users_groups_view_with_group_assignment(
    mut view: UsersGroupsWorkspaceResponse,
    username: &str,
    group_name: &str,
) -> UsersGroupsWorkspaceResponse {
    let username = username.trim();
    let group_name = group_name.trim();
    if username.is_empty() || group_name.is_empty() {
        return view;
    }

    if view
        .current_user
        .as_ref()
        .map(|user| user.username == username)
        .unwrap_or(false)
    {
        if let Some(user) = view.current_user.as_mut() {
            if !user.groups.iter().any(|group| group == group_name) {
                user.groups.push(group_name.to_string());
                user.groups.sort();
            }
        }
        for writer_group in &mut view.writer_groups {
            if writer_group.group_name == group_name {
                writer_group.current_user_member = true;
            }
        }
        for local_group in &mut view.groups {
            if local_group.group_name == group_name {
                local_group.current_user_member = true;
            }
        }
    }

    view.selected_username = Some(username.to_string());
    view.selected_group_name = Some(group_name.to_string());
    view
}

#[cfg(target_arch = "wasm32")]
pub(super) fn users_groups_state_with_writer_group(
    state: &ApiLoadState<UsersGroupsWorkspaceResponse>,
    group_name: &str,
) -> ApiLoadState<UsersGroupsWorkspaceResponse> {
    match state {
        ApiLoadState::Success(view) => ApiLoadState::Success(users_groups_view_with_writer_group(
            view.clone(),
            group_name,
        )),
        ApiLoadState::StaleData { value, message } => ApiLoadState::StaleData {
            value: users_groups_view_with_writer_group(value.clone(), group_name),
            message: message.clone(),
        },
        state => state.clone(),
    }
}

#[cfg(target_arch = "wasm32")]
pub(super) fn users_groups_state_with_group_assignment(
    state: &ApiLoadState<UsersGroupsWorkspaceResponse>,
    username: &str,
    group_name: &str,
) -> ApiLoadState<UsersGroupsWorkspaceResponse> {
    match state {
        ApiLoadState::Success(view) => ApiLoadState::Success(
            users_groups_view_with_group_assignment(view.clone(), username, group_name),
        ),
        ApiLoadState::StaleData { value, message } => ApiLoadState::StaleData {
            value: users_groups_view_with_group_assignment(value.clone(), username, group_name),
            message: message.clone(),
        },
        state => state.clone(),
    }
}

#[cfg(target_arch = "wasm32")]
pub(super) fn refresh_users_groups_workspace(
    api_base_path: String,
    users_groups_state: UseStateHandle<ApiLoadState<UsersGroupsWorkspaceResponse>>,
) {
    let path = users_groups_workspace_api_path(&api_base_path);
    wasm_bindgen_futures::spawn_local(async move {
        users_groups_state.set(page_load_state_from_result(
            crate::api::get_users_groups_workspace(&path).await,
            users_groups_empty_workspace_message,
        ));
    });
}

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct CreateLocalGroupFormState {
    group_name: String,
    applying: bool,
    submitted: Option<LocalGroupAdminResponse>,
    acknowledged: bool,
    error: Option<String>,
}

#[cfg(target_arch = "wasm32")]
impl CreateLocalGroupFormState {
    fn new() -> Self {
        Self {
            group_name: String::new(),
            applying: false,
            submitted: None,
            acknowledged: false,
            error: None,
        }
    }

    fn reset_result(&mut self) {
        self.submitted = None;
        self.error = None;
    }
}

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct AssignLocalUserFormState {
    username: String,
    group_name: String,
    applying: bool,
    submitted: Option<LocalGroupAdminResponse>,
    acknowledged: bool,
    error: Option<String>,
}

#[cfg(target_arch = "wasm32")]
impl AssignLocalUserFormState {
    fn from_view(view: Option<&UsersGroupsWorkspaceResponse>) -> Self {
        Self {
            username: view
                .and_then(|view| view.current_user.as_ref())
                .map(|user| user.username.clone())
                .unwrap_or_default(),
            group_name: view
                .and_then(|view| view.writer_groups.first())
                .map(|group| group.group_name.clone())
                .unwrap_or_default(),
            applying: false,
            submitted: None,
            acknowledged: false,
            error: None,
        }
    }

    fn reset_result(&mut self) {
        self.submitted = None;
        self.error = None;
    }
}

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Debug, Eq, PartialEq, Properties)]
pub struct UsersGroupsPageProps {
    pub api_base_path: String,
}

#[cfg(target_arch = "wasm32")]
#[function_component(UsersGroupsPage)]
pub fn users_groups_page(props: &UsersGroupsPageProps) -> Html {
    let api_path = WorkspacePage::UsersGroups.api_path(&props.api_base_path);
    let users_groups_state = use_state(|| ApiLoadState::<UsersGroupsWorkspaceResponse>::Loading);
    let create_group_state = use_state(CreateLocalGroupFormState::new);
    let assign_user_state = use_state(|| AssignLocalUserFormState::from_view(None));

    {
        let api_path = api_path.clone();
        let users_groups_state = users_groups_state.clone();
        use_effect_with(api_path.clone(), move |path| {
            let path = path.clone();
            wasm_bindgen_futures::spawn_local(async move {
                users_groups_state.set(page_load_state_from_result(
                    crate::api::get_users_groups_workspace(&path).await,
                    users_groups_empty_workspace_message,
                ));
            });
            || ()
        });
    }

    {
        let assign_user_state = assign_user_state.clone();
        use_effect_with((*users_groups_state).clone(), move |state| {
            if let ApiLoadState::Success(view) | ApiLoadState::StaleData { value: view, .. } = state
            {
                let mut next = (*assign_user_state).clone();
                let mut changed = false;
                if next.username.trim().is_empty() {
                    if let Some(user) = &view.current_user {
                        next.username = user.username.clone();
                        changed = true;
                    }
                }
                if next.group_name.trim().is_empty() {
                    if let Some(group) = view.writer_groups.first() {
                        next.group_name = group.group_name.clone();
                        changed = true;
                    }
                }
                if changed {
                    assign_user_state.set(next);
                }
            }
            || ()
        });
    }

    html! {
        <section class="dos-page" data-page="users-groups" data-api-route={api_path}>
            <PageHeader
                eyebrow="Prosopikon-aware appliance mapping"
                title="Local Access"
                summary="Map Prosopikon-recognized local users onto appliance OS groups and DASObjectStore writer/admin access rules."
            />
            { render_users_groups_state(
                &*users_groups_state,
                users_groups_state.clone(),
                create_group_state,
                assign_user_state,
                props.api_base_path.clone(),
            ) }
        </section>
    }
}

#[cfg(target_arch = "wasm32")]
pub(super) fn render_users_groups_state(
    state: &ApiLoadState<UsersGroupsWorkspaceResponse>,
    users_groups_state: UseStateHandle<ApiLoadState<UsersGroupsWorkspaceResponse>>,
    create_group_state: UseStateHandle<CreateLocalGroupFormState>,
    assign_user_state: UseStateHandle<AssignLocalUserFormState>,
    api_base_path: String,
) -> Html {
    match state {
        ApiLoadState::Loading => render_users_groups_state_message(
            "Loading",
            "Loading local access",
            "The Web console is requesting local principal, OS group, and writer-policy readiness.",
        ),
        ApiLoadState::Success(view) | ApiLoadState::StaleData { value: view, .. } => {
            render_users_groups_workspace(
                view,
                users_groups_state,
                create_group_state,
                assign_user_state,
                api_base_path,
            )
        }
        ApiLoadState::Empty(message) => {
            render_users_groups_state_message("Inventory", "No local access data", message)
        }
        ApiLoadState::PermissionDenied(message) => render_users_groups_state_message(
            "Permission denied",
            "Local access requires a standalone authenticated session",
            message,
        ),
        ApiLoadState::TransportError(message) => {
            render_users_groups_state_message("Error", "Unable to load local access", message)
        }
    }
}

#[cfg(target_arch = "wasm32")]
pub(super) fn render_users_groups_workspace(
    view: &UsersGroupsWorkspaceResponse,
    users_groups_state: UseStateHandle<ApiLoadState<UsersGroupsWorkspaceResponse>>,
    create_group_state: UseStateHandle<CreateLocalGroupFormState>,
    assign_user_state: UseStateHandle<AssignLocalUserFormState>,
    api_base_path: String,
) -> Html {
    html! {
        <>
            <section class="dos-metric-grid">
                { for users_groups_summary_cards(view).into_iter().map(render_metric_card) }
            </section>
            { render_prosopikon_local_access_widgets(view) }
            <section class="dos-attention-grid">
                { render_create_local_group_card(
                    view,
                    users_groups_state.clone(),
                    create_group_state,
                    assign_user_state.clone(),
                    api_base_path.clone(),
                ) }
                { render_assign_local_user_card(
                    view,
                    users_groups_state,
                    assign_user_state,
                    api_base_path,
                ) }
                <section class="dos-card dos-wide-card">
                    <span class="dos-card-label">{ "Local appliance actor" }</span>
                    if let Some(user) = &view.current_user {
                        <h2>{ &user.username }</h2>
                        <p>{ if user.sudo_administrator { "Sudo-derived local access administrator." } else { "Inspection-only local actor; Prosopikon remains the identity authority." } }</p>
                        <div class="dos-chip-row">
                            { for user.groups.iter().map(|group| html! {
                                <span class="dos-status-pill">{ group }</span>
                            }) }
                        </div>
                    } else {
                        <h2>{ "No current local user" }</h2>
                        <p>{ "The standalone session did not include OS-local authority metadata." }</p>
                    }
                </section>
                <section class="dos-card">
                    <span class="dos-card-label">{ "Access groups" }</span>
                    <h2>{ format!("{} mapped group(s)", view.writer_groups.len()) }</h2>
                    <p>{ format!("Local registry: {}", view.groups_file_path) }</p>
                    <div class="dos-chip-row">
                        { for view.writer_groups.iter().map(|group| html! {
                            <span class="dos-status-pill">{ format!("{} · {}", group.display_name, if group.current_user_member { "member" } else { "not member" }) }</span>
                        }) }
                    </div>
                </section>
                <section class="dos-card">
                    <span class="dos-card-label">{ "Mapping readiness" }</span>
                    <h2>{ if view.capabilities.administrator_actions_enabled { "Ready" } else { "Not ready" } }</h2>
                    <p>{ if view.capabilities.os_local_group_management { "Local access mapping is available for this session." } else { "Local access mapping is gated until sudo-derived authority is present." } }</p>
                    { for view.operations.iter().map(|operation| html! {
                        <p>{ format!("{}: {}", operation.label, if operation.enabled { "available" } else { operation.blocked_reason.as_deref().unwrap_or("blocked") }) }</p>
                    }) }
                </section>
            </section>
            if !view.warnings.is_empty() {
                <section class="dos-card dos-wide-card">
                    <span class="dos-card-label">{ "Warnings" }</span>
                    { for view.warnings.iter().map(|warning| html! {
                        <p>{ format!("{}: {}", warning.code, warning.message) }</p>
                    }) }
                </section>
            }
        </>
    }
}

#[cfg(target_arch = "wasm32")]
pub(super) fn render_prosopikon_local_access_widgets(view: &UsersGroupsWorkspaceResponse) -> Html {
    let users = local_access_principals(view);
    let groups = local_access_groups(view);
    let memberships = local_access_memberships(view);

    html! {
        <section class="dos-card dos-wide-card" data-section="prosopikon-local-access">
            <span class="dos-card-label">{ "Prosopikon local access" }</span>
            <div class="dos-prosopikon-widget-grid">
                <LocalAccessUserSelector
                    users={users}
                    selected_username={view.selected_username.clone()}
                    on_select={Callback::from(|_| ())}
                />
                <LocalAccessGroupSelector
                    groups={groups}
                    selected_group_name={view.selected_group_name.clone()}
                    on_select={Callback::from(|_| ())}
                />
                <LocalAccessMembershipList memberships={memberships} />
            </div>
            <p>{ format!("Device tokens: {:?}", view.device_token_requirement) }</p>
        </section>
    }
}

#[cfg(target_arch = "wasm32")]
pub(super) fn local_access_principals(
    view: &UsersGroupsWorkspaceResponse,
) -> Vec<LocalAccessPrincipalRecord> {
    view.users
        .iter()
        .map(|user| {
            let sudo_administrator = view.current_user.as_ref().is_some_and(|current| {
                current.username == user.username && current.sudo_administrator
            });
            LocalAccessPrincipalRecord {
                username: user.username.clone(),
                display_name: Some(user.username.clone()),
                sudo_administrator,
            }
        })
        .collect()
}

#[cfg(target_arch = "wasm32")]
pub(super) fn local_access_groups(
    view: &UsersGroupsWorkspaceResponse,
) -> Vec<LocalAccessGroupRecord> {
    view.writer_groups
        .iter()
        .map(|group| LocalAccessGroupRecord {
            group_name: group.group_name.clone(),
            display_name: group.display_name.clone(),
            source: group.source.clone(),
        })
        .collect()
}

#[cfg(target_arch = "wasm32")]
pub(super) fn local_access_memberships(
    view: &UsersGroupsWorkspaceResponse,
) -> Vec<LocalAccessMembershipRecord> {
    let Some(user) = &view.current_user else {
        return Vec::new();
    };

    view.groups
        .iter()
        .map(|group| LocalAccessMembershipRecord {
            username: user.username.clone(),
            group_name: group.group_name.clone(),
            administrator_grant: group.sudo_administrator_group || user.sudo_administrator,
        })
        .collect()
}

#[cfg(target_arch = "wasm32")]
pub(super) fn render_create_local_group_card(
    view: &UsersGroupsWorkspaceResponse,
    users_groups_state: UseStateHandle<ApiLoadState<UsersGroupsWorkspaceResponse>>,
    create_group_state: UseStateHandle<CreateLocalGroupFormState>,
    assign_user_state: UseStateHandle<AssignLocalUserFormState>,
    api_base_path: String,
) -> Html {
    let state = (*create_group_state).clone();
    let enabled = view.capabilities.os_local_group_management;
    let can_apply = enabled
        && local_group_create_fields_ready(&state.group_name)
        && state.acknowledged
        && !state.applying;

    let on_group_name = {
        let create_group_state = create_group_state.clone();
        Callback::from(move |event: InputEvent| {
            let input: HtmlInputElement = event.target_unchecked_into();
            let mut next = (*create_group_state).clone();
            next.group_name = input.value();
            next.reset_result();
            create_group_state.set(next);
        })
    };
    let on_acknowledged = {
        let create_group_state = create_group_state.clone();
        Callback::from(move |event: Event| {
            let input: HtmlInputElement = event.target_unchecked_into();
            let mut next = (*create_group_state).clone();
            next.acknowledged = input.checked();
            next.submitted = None;
            create_group_state.set(next);
        })
    };
    let apply = {
        let create_group_state = create_group_state.clone();
        let users_groups_state = users_groups_state.clone();
        let assign_user_state = assign_user_state.clone();
        let api_base_path = api_base_path.clone();
        Callback::from(move |_| {
            let mut pending = (*create_group_state).clone();
            pending.applying = true;
            pending.error = None;
            pending.submitted = None;
            create_group_state.set(pending.clone());

            let create_group_state = create_group_state.clone();
            let users_groups_state = users_groups_state.clone();
            let assign_user_state = assign_user_state.clone();
            let api_base_path = api_base_path.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let request = CreateLocalGroupRequest {
                    group_name: pending.group_name.trim().to_string(),
                    dry_run: false,
                    confirmation_marker: Some(LOCAL_GROUP_ADMIN_CONFIRMATION.to_string()),
                    client_request_id: None,
                };
                let result = crate::api::submit_create_local_group(&api_base_path, &request).await;
                let mut next = (*create_group_state).clone();
                next.applying = false;
                match result {
                    Ok(response) => {
                        let group_name = response.group_name.clone();
                        create_group_state.set(CreateLocalGroupFormState::new());
                        users_groups_state.set(users_groups_state_with_writer_group(
                            &*users_groups_state,
                            &group_name,
                        ));
                        let mut assign_next = (*assign_user_state).clone();
                        assign_next.group_name = group_name.clone();
                        assign_next.applying = false;
                        assign_next.reset_result();
                        assign_user_state.set(assign_next);
                        refresh_users_groups_workspace(api_base_path, users_groups_state);
                        return;
                    }
                    Err(error) => {
                        next.submitted = None;
                        next.error = Some(error.message);
                    }
                }
                create_group_state.set(next);
            });
        })
    };

    html! {
        <section class="dos-card dos-create-card" data-action="create_local_group">
            <span class="dos-create-mark">{ "+" }</span>
            <h2>{ "Create a data access account or tenant group" }</h2>
            <p>{ if enabled { "Create a local OS group that maps Prosopikon-recognized users to DASObjectStore writer/admin access." } else { "Requires sudo-derived administrator authority." } }</p>
            <span class="dos-status-pill">{ if enabled { "Available" } else { "Admin only" } }</span>
            <label class="dos-form-field">
                <span>{ "Access account or tenant group" }</span>
                <input
                    type="text"
                    value={state.group_name.clone()}
                    placeholder="mnemosyne-writers"
                    oninput={on_group_name}
                    disabled={!enabled}
                />
            </label>
            <label class="dos-checkbox-row">
                <input
                    type="checkbox"
                    checked={state.acknowledged}
                    onchange={on_acknowledged}
                    disabled={!enabled}
                />
                <span>{ "Clicking this dialog enables the creation of the specified access group" }</span>
            </label>
            <button class="dos-auth-submit" type="button" disabled={!can_apply} onclick={apply}>
                { if state.applying { "Submitting..." } else { "Submit access group" } }
            </button>
            { render_local_group_admin_result("Submitted", state.submitted.as_ref()) }
            if let Some(error) = &state.error {
                <div class="dos-auth-error" role="alert">{ error.clone() }</div>
            }
        </section>
    }
}

#[cfg(target_arch = "wasm32")]
pub(super) fn render_assign_local_user_card(
    view: &UsersGroupsWorkspaceResponse,
    users_groups_state: UseStateHandle<ApiLoadState<UsersGroupsWorkspaceResponse>>,
    assign_user_state: UseStateHandle<AssignLocalUserFormState>,
    api_base_path: String,
) -> Html {
    let state = (*assign_user_state).clone();
    let enabled = view.capabilities.os_local_group_management;
    let can_apply = enabled
        && local_group_assignment_fields_ready(&state.username, &state.group_name)
        && state.acknowledged
        && !state.applying;
    let user_options = view.users.clone();
    let group_options = view.writer_groups.clone();

    let on_acknowledged = {
        let assign_user_state = assign_user_state.clone();
        Callback::from(move |event: Event| {
            let input: HtmlInputElement = event.target_unchecked_into();
            let mut next = (*assign_user_state).clone();
            next.acknowledged = input.checked();
            next.submitted = None;
            assign_user_state.set(next);
        })
    };
    let apply = {
        let assign_user_state = assign_user_state.clone();
        let users_groups_state = users_groups_state.clone();
        let api_base_path = api_base_path.clone();
        Callback::from(move |_| {
            let mut pending = (*assign_user_state).clone();
            pending.applying = true;
            pending.error = None;
            pending.submitted = None;
            assign_user_state.set(pending.clone());

            let assign_user_state = assign_user_state.clone();
            let users_groups_state = users_groups_state.clone();
            let api_base_path = api_base_path.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let request = AssignLocalUserToGroupRequest {
                    username: pending.username.trim().to_string(),
                    group_name: pending.group_name.trim().to_string(),
                    dry_run: false,
                    confirmation_marker: Some(LOCAL_GROUP_ADMIN_CONFIRMATION.to_string()),
                    client_request_id: None,
                };
                let result =
                    crate::api::submit_assign_local_user_to_group(&api_base_path, &request).await;
                let mut next = (*assign_user_state).clone();
                next.applying = false;
                match result {
                    Ok(response) => {
                        let username = response
                            .username
                            .clone()
                            .unwrap_or_else(|| pending.username.trim().to_string());
                        let group_name = response.group_name.clone();
                        next.submitted = Some(response);
                        next.acknowledged = false;
                        next.error = None;
                        users_groups_state.set(users_groups_state_with_group_assignment(
                            &*users_groups_state,
                            &username,
                            &group_name,
                        ));
                        refresh_users_groups_workspace(api_base_path, users_groups_state);
                    }
                    Err(error) => {
                        next.submitted = None;
                        next.error = Some(error.message);
                    }
                }
                assign_user_state.set(next);
            });
        })
    };

    html! {
        <section class="dos-card dos-create-card" data-action="assign_local_user_to_group">
            <span class="dos-create-mark">{ "@" }</span>
            <h2>{ "Map user to tenant group" }</h2>
            <p>{ if enabled { "Map a local user to a tenant group for appliance access enforcement." } else { "Requires sudo-derived administrator authority." } }</p>
            <span class="dos-status-pill">{ if enabled { "Available" } else { "Admin only" } }</span>
            <label class="dos-form-field">
                <span>{ "Local user" }</span>
                <input
                    type="text"
                    list="dos-local-users"
                    value={state.username.clone()}
                    placeholder="stephen"
                    oninput={{
                        let assign_user_state = assign_user_state.clone();
                        Callback::from(move |event: InputEvent| {
                            let input: HtmlInputElement = event.target_unchecked_into();
                            let mut next = (*assign_user_state).clone();
                            next.username = input.value();
                            next.reset_result();
                            assign_user_state.set(next);
                        })
                    }}
                    disabled={!enabled}
                />
                <datalist id="dos-local-users">
                    { for user_options.iter().map(|user| html! {
                        <option value={user.username.clone()} />
                    }) }
                </datalist>
            </label>
            <label class="dos-form-field">
                <span>{ "Access account or tenant group" }</span>
                <select onchange={{
                    let assign_user_state = assign_user_state.clone();
                    Callback::from(move |event: Event| {
                        let input: HtmlSelectElement = event.target_unchecked_into();
                        let mut next = (*assign_user_state).clone();
                        next.group_name = input.value();
                        next.reset_result();
                        assign_user_state.set(next);
                    })
                }} value={state.group_name.clone()} disabled={!enabled}>
                    <option value="">{ "Select group" }</option>
                    { for group_options.iter().map(|group| html! {
                        <option value={group.group_name.clone()}>{ format!("{} ({})", group.display_name, group.group_name) }</option>
                    }) }
                    if group_options.is_empty() && !state.group_name.is_empty() {
                        <option value={state.group_name.clone()}>{ state.group_name.clone() }</option>
                    }
                </select>
            </label>
            <label class="dos-checkbox-row">
                <input
                    type="checkbox"
                    checked={state.acknowledged}
                    onchange={on_acknowledged}
                    disabled={!enabled}
                />
                <span>{ "Clicking this dialog enables the selected user to be mapped to the specified tenant group" }</span>
            </label>
            <button class="dos-auth-submit" type="button" disabled={!can_apply} onclick={apply}>
                { if state.applying { "Submitting..." } else { "Submit access mapping" } }
            </button>
            { render_local_group_admin_result("Submitted", state.submitted.as_ref()) }
            if let Some(error) = &state.error {
                <div class="dos-auth-error" role="alert">{ error.clone() }</div>
            }
        </section>
    }
}

#[cfg(target_arch = "wasm32")]
pub(super) fn render_local_group_admin_result(
    label: &str,
    response: Option<&LocalGroupAdminResponse>,
) -> Html {
    match response {
        Some(response) => html! {
            <section class="dos-plan-result" data-job-state="accepted">
                <span class="dos-card-label">{ label }</span>
                <p>{ format!("Job {} · {} · dry run {}", response.accepted.job_id, response.accepted.kind, response.accepted.dry_run) }</p>
                <code>{ format!("{} · group {}{}", response.operation, response.group_name, response.username.as_ref().map(|username| format!(" · user {username}")).unwrap_or_default()) }</code>
            </section>
        },
        None => Html::default(),
    }
}

#[cfg(target_arch = "wasm32")]
pub(super) fn render_users_groups_state_message(label: &str, title: &str, message: &str) -> Html {
    html! {
        <section class="dos-card dos-wide-card">
            <span class="dos-card-label">{ label }</span>
            <h2>{ title }</h2>
            <p>{ message }</p>
        </section>
    }
}

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Debug, Eq, PartialEq, Properties)]
pub struct ActivityPageProps {
    pub api_base_path: String,
}
