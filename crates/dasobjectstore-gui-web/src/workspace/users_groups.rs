use super::*;

#[cfg(target_arch = "wasm32")]
use crate::components::{TaskPane, TaskPaneMode};

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
    let add_user_state = use_state(AddUserTaskState::default);
    let task_pane_mode = use_state(|| TaskPaneMode::Closed);
    let add_user_trigger_ref = use_node_ref();

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
                add_user_state,
                task_pane_mode,
                add_user_trigger_ref,
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
    add_user_state: UseStateHandle<AddUserTaskState>,
    task_pane_mode: UseStateHandle<TaskPaneMode>,
    add_user_trigger_ref: NodeRef,
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
                add_user_state,
                task_pane_mode,
                add_user_trigger_ref,
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
    add_user_state: UseStateHandle<AddUserTaskState>,
    task_pane_mode: UseStateHandle<TaskPaneMode>,
    add_user_trigger_ref: NodeRef,
    api_base_path: String,
) -> Html {
    html! {
        <>
            <section class="dos-metric-grid">
                { for users_groups_summary_cards(view).into_iter().map(render_metric_card) }
            </section>
            { render_users_groups_toolbar(view, task_pane_mode.clone(), add_user_state.clone(), add_user_trigger_ref.clone()) }
            { render_users_inventory(view, task_pane_mode.clone(), add_user_state.clone()) }
            { render_groups_context(view, task_pane_mode.clone()) }
            { render_users_groups_task_pane(
                view,
                users_groups_state,
                create_group_state,
                add_user_state,
                task_pane_mode,
                add_user_trigger_ref,
                api_base_path,
            ) }
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
fn render_users_groups_toolbar(
    view: &UsersGroupsWorkspaceResponse,
    task_pane_mode: UseStateHandle<TaskPaneMode>,
    add_user_state: UseStateHandle<AddUserTaskState>,
    add_user_trigger_ref: NodeRef,
) -> Html {
    let open_add_user = {
        let task_pane_mode = task_pane_mode.clone();
        let add_user_state = add_user_state.clone();
        let view = view.clone();
        Callback::from(move |_| {
            add_user_state.set(AddUserTaskState::from_view(&view, None));
            task_pane_mode.set(TaskPaneMode::Create);
        })
    };
    let open_create_group =
        Callback::from(move |_| task_pane_mode.set(TaskPaneMode::Edit("groups".to_string())));
    html! {
        <section class="dos-card dos-wide-card dos-users-toolbar" data-section="users-toolbar">
            <div>
                <span class="dos-card-label">{ "Users" }</span>
                <h2>{ "Local access inventory" }</h2>
                <p>{ "Qualify existing OS-recognized users for DASObjectStore access; the browser never creates operating-system accounts." }</p>
            </div>
            <div class="dos-job-actions">
                <button type="button" class="dos-auth-submit" ref={add_user_trigger_ref} onclick={open_add_user} disabled={!view.capabilities.administrator_actions_enabled}>
                    { "Add user" }
                </button>
                <button type="button" class="dos-secondary-action" onclick={open_create_group} disabled={!view.capabilities.administrator_actions_enabled}>
                    { "Create group" }
                </button>
            </div>
            if !view.capabilities.administrator_actions_enabled {
                <p class="dos-empty-state">{ "Add-user and group actions require sudo-derived administrator authority." }</p>
            }
        </section>
    }
}

#[cfg(target_arch = "wasm32")]
fn render_users_inventory(
    view: &UsersGroupsWorkspaceResponse,
    task_pane_mode: UseStateHandle<TaskPaneMode>,
    add_user_state: UseStateHandle<AddUserTaskState>,
) -> Html {
    html! {
        <section class="dos-card dos-wide-card dos-users-inventory" data-section="users-inventory">
            <div class="dos-card-row">
                <div>
                    <span class="dos-card-label">{ "Users" }</span>
                    <h2>{ format!("{} local user(s)", view.users.len()) }</h2>
                </div>
                <span class="dos-status-pill">{ if view.capabilities.administrator_actions_enabled { "managed" } else { "read only" } }</span>
            </div>
            if view.users.is_empty() {
                <p class="dos-empty-state">{ "No local users are registered. Add-user qualifies an existing OS account; it does not create one." }</p>
            } else {
                <div class="dos-table-wrap">
                    <table class="dos-table dos-dense-table dos-users-table">
                        <thead><tr><th>{ "User" }</th><th>{ "Qualification" }</th><th>{ "Access groups" }</th><th>{ "Administrator" }</th><th>{ "Sessions" }</th><th>{ "Action" }</th></tr></thead>
                        <tbody>{ for view.users.iter().map(|user| {
                            let username = user.username.clone();
                            let task_pane_mode = task_pane_mode.clone();
                            let add_user_state = add_user_state.clone();
                            let view_for_callback = view.clone();
                            let open_user = Callback::from(move |_| {
                                add_user_state.set(AddUserTaskState::from_view(&view_for_callback, Some(&username)));
                                task_pane_mode.set(TaskPaneMode::Edit(username.clone()));
                            });
                            html! {
                                <tr data-username={user.username.clone()}>
                                    <td><strong>{ user.username.clone() }</strong><small>{ if user.registered { "registered" } else { "not registered" } }</small></td>
                                    <td>{ user.qualification_state.clone() }</td>
                                    <td>{ if user.groups.is_empty() { "none".to_string() } else { user.groups.join(", ") } }</td>
                                    <td>{ if user.sudo_administrator { "yes" } else { "no" } }</td>
                                    <td>{ user.active_session_count }</td>
                                    <td><button type="button" class="dos-secondary-action" onclick={open_user.clone()} disabled={!view.capabilities.administrator_actions_enabled}>{ "Qualify" }</button></td>
                                </tr>
                            }
                        }) }</tbody>
                    </table>
                </div>
            }
        </section>
    }
}

#[cfg(target_arch = "wasm32")]
fn render_groups_context(
    view: &UsersGroupsWorkspaceResponse,
    task_pane_mode: UseStateHandle<TaskPaneMode>,
) -> Html {
    let open_groups =
        Callback::from(move |_| task_pane_mode.set(TaskPaneMode::Edit("groups".to_string())));
    html! {
        <section class="dos-card dos-wide-card dos-groups-context" data-section="groups-context">
            <div class="dos-card-row">
                <div><span class="dos-card-label">{ "Groups" }</span><h2>{ format!("{} access group(s)", view.writer_groups.len()) }</h2></div>
                <button type="button" class="dos-secondary-action" onclick={open_groups} disabled={!view.capabilities.administrator_actions_enabled}>{ "Manage groups" }</button>
            </div>
            <p>{ format!("Authoritative registry: {}", view.groups_file_path) }</p>
            <div class="dos-chip-row">{ for view.writer_groups.iter().map(|group| html! { <span class="dos-status-pill">{ format!("{} · {}", group.display_name, if group.current_user_member { "current user" } else { "available" }) }</span> }) }</div>
        </section>
    }
}

#[cfg(target_arch = "wasm32")]
fn render_users_groups_task_pane(
    view: &UsersGroupsWorkspaceResponse,
    users_groups_state: UseStateHandle<ApiLoadState<UsersGroupsWorkspaceResponse>>,
    create_group_state: UseStateHandle<CreateLocalGroupFormState>,
    add_user_state: UseStateHandle<AddUserTaskState>,
    task_pane_mode: UseStateHandle<TaskPaneMode>,
    add_user_trigger_ref: NodeRef,
    api_base_path: String,
) -> Html {
    let mode = (*task_pane_mode).clone();
    let on_close = {
        let task_pane_mode = task_pane_mode.clone();
        Callback::<()>::from(move |_| task_pane_mode.set(TaskPaneMode::Closed))
    };
    match mode {
        TaskPaneMode::Closed => Html::default(),
        TaskPaneMode::Create => render_add_user_task_pane(
            view,
            users_groups_state,
            add_user_state,
            task_pane_mode,
            add_user_trigger_ref,
            api_base_path,
        ),
        TaskPaneMode::Edit(ref context) if context == "groups" => html! {
            <TaskPane mode={TaskPaneMode::Edit("groups".to_string())} title="Create access group" selected_context={Some(view.groups_file_path.clone())} return_focus_to={Some(add_user_trigger_ref.clone())} on_close={on_close.clone()}>
                { render_create_local_group_card(view, users_groups_state, create_group_state, api_base_path) }
            </TaskPane>
        },
        TaskPaneMode::Edit(username) => render_add_user_task_pane_for_user(
            view,
            users_groups_state,
            add_user_state,
            task_pane_mode,
            add_user_trigger_ref,
            api_base_path,
            Some(username.clone()),
            TaskPaneMode::Edit(username),
        ),
        TaskPaneMode::Review => Html::default(),
    }
}

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct AddUserTaskState {
    username: String,
    selected_groups: Vec<String>,
    qualification_acknowledged: bool,
    applying: bool,
    submitted: Vec<LocalGroupAdminResponse>,
    error: Option<String>,
}

#[cfg(target_arch = "wasm32")]
impl Default for AddUserTaskState {
    fn default() -> Self {
        Self {
            username: String::new(),
            selected_groups: Vec::new(),
            qualification_acknowledged: false,
            applying: false,
            submitted: Vec::new(),
            error: None,
        }
    }
}

#[cfg(target_arch = "wasm32")]
impl AddUserTaskState {
    fn from_view(view: &UsersGroupsWorkspaceResponse, username: Option<&str>) -> Self {
        let username = username
            .filter(|username| view.users.iter().any(|user| user.username == *username))
            .map(str::to_string)
            .or_else(|| view.users.first().map(|user| user.username.clone()))
            .unwrap_or_default();
        Self {
            username,
            selected_groups: Vec::new(),
            qualification_acknowledged: false,
            applying: false,
            submitted: Vec::new(),
            error: None,
        }
    }
}

#[cfg(target_arch = "wasm32")]
fn render_add_user_task_pane(
    view: &UsersGroupsWorkspaceResponse,
    users_groups_state: UseStateHandle<ApiLoadState<UsersGroupsWorkspaceResponse>>,
    add_user_state: UseStateHandle<AddUserTaskState>,
    task_pane_mode: UseStateHandle<TaskPaneMode>,
    add_user_trigger_ref: NodeRef,
    api_base_path: String,
) -> Html {
    render_add_user_task_pane_for_user(
        view,
        users_groups_state,
        add_user_state,
        task_pane_mode,
        add_user_trigger_ref,
        api_base_path,
        None,
        TaskPaneMode::Create,
    )
}

#[cfg(target_arch = "wasm32")]
fn render_add_user_task_pane_for_user(
    view: &UsersGroupsWorkspaceResponse,
    users_groups_state: UseStateHandle<ApiLoadState<UsersGroupsWorkspaceResponse>>,
    add_user_state: UseStateHandle<AddUserTaskState>,
    task_pane_mode: UseStateHandle<TaskPaneMode>,
    add_user_trigger_ref: NodeRef,
    api_base_path: String,
    _username: Option<String>,
    pane_mode: TaskPaneMode,
) -> Html {
    let state = add_user_state;
    let state_value = (*state).clone();
    let selected_user = view
        .users
        .iter()
        .find(|user| user.username == state_value.username);
    let title = if state_value.username.is_empty() {
        "Add user"
    } else {
        "Qualify user"
    };
    let context =
        selected_user.map(|user| format!("{} · {}", user.username, user.qualification_state));
    let on_close = {
        let task_pane_mode = task_pane_mode.clone();
        Callback::<()>::from(move |_| task_pane_mode.set(TaskPaneMode::Closed))
    };
    let on_cancel = {
        let on_close = on_close.clone();
        Callback::from(move |_| on_close.emit(()))
    };
    let on_username = {
        let state = state.clone();
        Callback::from(move |event: Event| {
            let input: HtmlSelectElement = event.target_unchecked_into();
            let mut next = (*state).clone();
            next.username = input.value();
            next.qualification_acknowledged = false;
            next.submitted.clear();
            next.error = None;
            state.set(next);
        })
    };
    let on_qualification = {
        let state = state.clone();
        Callback::from(move |event: Event| {
            let input: HtmlInputElement = event.target_unchecked_into();
            let mut next = (*state).clone();
            next.qualification_acknowledged = input.checked();
            next.error = None;
            state.set(next);
        })
    };
    let apply = {
        let state = state.clone();
        let users_groups_state = users_groups_state.clone();
        let api_base_path = api_base_path.clone();
        Callback::from(move |_| {
            let mut pending = (*state).clone();
            pending.applying = true;
            pending.error = None;
            pending.submitted.clear();
            state.set(pending.clone());
            let state = state.clone();
            let users_groups_state = users_groups_state.clone();
            let api_base_path = api_base_path.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let mut responses = Vec::new();
                for group_name in &pending.selected_groups {
                    let group_name = group_name.clone();
                    let request = AssignLocalUserToGroupRequest {
                        username: pending.username.trim().to_string(),
                        group_name: group_name.clone(),
                        dry_run: false,
                        confirmation_marker: Some(LOCAL_GROUP_ADMIN_CONFIRMATION.to_string()),
                        client_request_id: None,
                    };
                    match crate::api::submit_assign_local_user_to_group(&api_base_path, &request)
                        .await
                    {
                        Ok(response) => responses.push(response),
                        Err(error) => {
                            for response in &responses {
                                let username = response
                                    .username
                                    .clone()
                                    .unwrap_or_else(|| pending.username.clone());
                                users_groups_state.set(users_groups_state_with_group_assignment(
                                    &*users_groups_state,
                                    &username,
                                    &response.group_name,
                                ));
                            }
                            let mut next = (*state).clone();
                            next.applying = false;
                            next.submitted = responses;
                            next.error = Some(format!(
                                "Group {group_name} was not applied: {}",
                                error.message
                            ));
                            state.set(next);
                            refresh_users_groups_workspace(api_base_path, users_groups_state);
                            return;
                        }
                    }
                }
                let mut next = (*state).clone();
                next.applying = false;
                next.qualification_acknowledged = false;
                next.submitted = responses.clone();
                state.set(next);
                for response in responses {
                    let username = response
                        .username
                        .clone()
                        .unwrap_or_else(|| pending.username.clone());
                    let group_name = response.group_name.clone();
                    users_groups_state.set(users_groups_state_with_group_assignment(
                        &*users_groups_state,
                        &username,
                        &group_name,
                    ));
                }
                refresh_users_groups_workspace(api_base_path, users_groups_state);
            });
        })
    };
    let selected_groups = state_value.selected_groups.clone();
    let can_apply = view.capabilities.administrator_actions_enabled
        && !state_value.username.trim().is_empty()
        && !selected_groups.is_empty()
        && state_value.qualification_acknowledged
        && !state_value.applying;
    let footer = html! {
        <>
            <button type="button" class="dos-secondary-action" onclick={on_cancel}>{ "Cancel" }</button>
            <button type="button" class="dos-auth-submit" disabled={!can_apply} onclick={apply}>{ if state_value.applying { "Applying..." } else { "Review and apply" } }</button>
        </>
    };
    html! {
        <TaskPane mode={pane_mode} title={title.to_string()} selected_context={context} return_focus_to={Some(add_user_trigger_ref)} on_close={on_close} footer_actions={footer}>
            <section class="dos-task-pane__section" data-step="identify-user">
                <span class="dos-card-label">{ "1 · Identify existing user" }</span>
                <label class="dos-form-field"><span>{ "OS-recognized/local user" }</span><select onchange={on_username} value={state_value.username.clone()}>
                    { for view.users.iter().map(|user| html! { <option value={user.username.clone()}>{ &user.username }</option> }) }
                </select></label>
                if let Some(user) = selected_user {
                    <p>{ format!("{} · {} · {} active session(s)", user.username, user.qualification_state, user.active_session_count) }</p>
                }
            </section>
            <section class="dos-task-pane__section" data-step="qualification">
                <span class="dos-card-label">{ "2 · Record qualification" }</span>
                <p>{ "Qualification confirms that the selected account is already recognized by the host. No Unix/OS account is created by this browser." }</p>
                <label class="dos-checkbox-row"><input type="checkbox" checked={state_value.qualification_acknowledged} onchange={on_qualification} /><span>{ "I confirm this existing local user is qualified for DASObjectStore access." }</span></label>
            </section>
            <section class="dos-task-pane__section" data-step="groups">
                <span class="dos-card-label">{ "3 · Select access groups" }</span>
                { for view.writer_groups.iter().map(|group| {
                    let state = state.clone();
                    let group_name = group.group_name.clone();
                    let checked = selected_groups.iter().any(|name| name == &group.group_name);
                    html! { <label class="dos-checkbox-row"><input type="checkbox" checked={checked} onchange={Callback::from(move |event: Event| { let input: HtmlInputElement = event.target_unchecked_into(); let mut next = (*state).clone(); if input.checked() { if !next.selected_groups.iter().any(|name| name == &group_name) { next.selected_groups.push(group_name.clone()); } } else { next.selected_groups.retain(|name| name != &group_name); } next.error = None; state.set(next); })} /><span>{ format!("{} ({})", group.display_name, group.group_name) }</span></label> }
                }) }
            </section>
            <section class="dos-task-pane__section" data-step="review">
                <span class="dos-card-label">{ "4 · Review and apply" }</span>
                <p>{ format!("{} → {}", state_value.username, if selected_groups.is_empty() { "no groups selected".to_string() } else { selected_groups.join(", ") }) }</p>
                if let Some(error) = &state_value.error { <div class="dos-auth-error" role="alert">{ error.clone() }</div> }
                { for state_value.submitted.iter().map(|response| render_local_group_admin_result("Applied", Some(response))) }
            </section>
        </TaskPane>
    }
}

#[cfg(target_arch = "wasm32")]
pub(super) fn render_create_local_group_card(
    view: &UsersGroupsWorkspaceResponse,
    users_groups_state: UseStateHandle<ApiLoadState<UsersGroupsWorkspaceResponse>>,
    create_group_state: UseStateHandle<CreateLocalGroupFormState>,
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
        let api_base_path = api_base_path.clone();
        Callback::from(move |_| {
            let mut pending = (*create_group_state).clone();
            pending.applying = true;
            pending.error = None;
            pending.submitted = None;
            create_group_state.set(pending.clone());

            let create_group_state = create_group_state.clone();
            let users_groups_state = users_groups_state.clone();
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
