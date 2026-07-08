use crate::mount::{FrontendHost, FrontendMount};
use crate::session::{AppState, StableState};
use crate::workspace::{
    BioinformaticsPage, EnclosuresPage, HomeDashboard, ObjectStoresPage, WorkspacePage,
    PRIMARY_NAVIGATION,
};
use crate::{api, storage};
use web_sys::HtmlInputElement;
use yew::prelude::*;

const DASOBJECTSTORE_VERSION: &str = env!("CARGO_PKG_VERSION");

#[function_component(App)]
pub fn app() -> Html {
    let mount = FrontendMount::default_for(FrontendHost::Standalone);
    let api_base_path = mount.api_base_path.clone();
    let auth_base_path = mount.auth_base_path();
    let initial_session = storage::stored_session();
    let stable_state = use_state(|| StableState::Disconnected);
    let app_state = use_state(|| {
        if initial_session.is_some() {
            AppState::CheckingSession
        } else {
            AppState::Disconnected
        }
    });
    let username = use_state(|| {
        initial_session
            .as_ref()
            .map(|(username, _)| username.clone())
            .unwrap_or_default()
    });
    let password = use_state(String::new);

    {
        let auth_base_path = auth_base_path.clone();
        let stable_state = stable_state.clone();
        let app_state = app_state.clone();
        let username = username.clone();
        use_effect_with((), move |_| {
            if let Some((stored_username, session_token)) = storage::stored_session() {
                wasm_bindgen_futures::spawn_local(async move {
                    match api::verify_session(&auth_base_path, stored_username, session_token).await
                    {
                        Ok(response) if response.valid => {
                            username.set(response.username);
                            stable_state.set(StableState::Connected);
                            app_state.set(AppState::Connected);
                        }
                        Ok(_) => {
                            storage::clear_session();
                            stable_state.set(StableState::Disconnected);
                            app_state.set(AppState::Disconnected);
                        }
                        Err(error) => {
                            storage::clear_session();
                            stable_state.set(StableState::Disconnected);
                            app_state.set(AppState::Error(error.message));
                        }
                    }
                });
            }
            || ()
        });
    }

    let on_username = input_callback(username.clone());
    let on_password = input_callback(password.clone());

    let on_login = {
        let auth_base_path = auth_base_path.clone();
        let stable_state = stable_state.clone();
        let app_state = app_state.clone();
        let username = username.clone();
        let password = password.clone();
        Callback::from(move |event: SubmitEvent| {
            event.prevent_default();
            app_state.set(AppState::Connecting);
            let auth_base_path = auth_base_path.clone();
            let stable_state = stable_state.clone();
            let app_state = app_state.clone();
            let username = username.clone();
            let password = password.clone();
            wasm_bindgen_futures::spawn_local(async move {
                match api::login(&auth_base_path, (*username).clone(), (*password).clone()).await {
                    Ok(response) => {
                        storage::store_session(&response.username, &response.session_token);
                        username.set(response.username);
                        password.set(String::new());
                        stable_state.set(StableState::Connected);
                        app_state.set(AppState::Connected);
                    }
                    Err(error) => {
                        storage::clear_session();
                        stable_state.set(StableState::Disconnected);
                        app_state.set(AppState::Error(error.message));
                    }
                }
            });
        })
    };

    let on_logout = {
        let auth_base_path = auth_base_path.clone();
        let stable_state = stable_state.clone();
        let app_state = app_state.clone();
        let username = username.clone();
        Callback::from(move |_| {
            app_state.set(AppState::Disconnecting);
            let auth_base_path = auth_base_path.clone();
            let stable_state = stable_state.clone();
            let app_state = app_state.clone();
            let username = username.clone();
            wasm_bindgen_futures::spawn_local(async move {
                if let Some((stored_username, session_token)) = storage::stored_session() {
                    let _ = api::logout(&auth_base_path, stored_username, session_token).await;
                }
                storage::clear_session();
                username.set(String::new());
                stable_state.set(StableState::Disconnected);
                app_state.set(AppState::Disconnected);
            });
        })
    };

    html! {
        <div class="dos-app-shell">
            <main
                data-host={mount.host.name()}
                data-api-base={api_base_path.clone()}
                data-auth-base={auth_base_path.clone()}
                data-initial-workspace={crate::entrypoint::POST_LOGIN_WORKSPACE_ID}
            >
                {match *stable_state {
                    StableState::Disconnected => html! {
                        <LandingPage
                            username={(*username).clone()}
                            password={(*password).clone()}
                            error_message={(*app_state).error_message()}
                            busy_label={(*app_state).busy_label().map(str::to_string)}
                            on_username={on_username}
                            on_password={on_password}
                            on_login={on_login}
                        />
                    },
                    StableState::Connected => html! {
                        <AuthenticatedWorkspace
                            username={(*username).clone()}
                            busy_label={(*app_state).busy_label().map(str::to_string)}
                            api_base_path={api_base_path}
                            on_logout={on_logout}
                        />
                    },
                }}
            </main>
            <footer class="dos-app-footer">
                {format!("DASObjectStore v{DASOBJECTSTORE_VERSION}")}
                {" · Mnemosyne Biosciences Ltd"}
            </footer>
        </div>
    }
}

#[derive(Clone, Debug, PartialEq, Properties)]
struct LandingPageProps {
    username: String,
    password: String,
    error_message: Option<String>,
    busy_label: Option<String>,
    on_username: Callback<InputEvent>,
    on_password: Callback<InputEvent>,
    on_login: Callback<SubmitEvent>,
}

#[function_component(LandingPage)]
fn landing_page(props: &LandingPageProps) -> Html {
    html! {
        <section class="dos-auth-shell">
            <aside class="dos-auth-brand">
                <div class="dos-brand-lockup" aria-label="Mnemosyne Biosciences DASObjectStore">
                    <span class="dos-brand-mark">{ "M" }</span>
                    <div>
                        <strong>{ "Mnemosyne Biosciences" }</strong>
                        <span>{ "DASObjectStore" }</span>
                    </div>
                </div>
                <div class="dos-auth-summary">
                    <p>{ "Local appliance access" }</p>
                    <h1>{ "Sign in to manage directly attached object storage." }</h1>
                    <span>{ "Storage, ingest, and service controls for the DASObjectStore appliance." }</span>
                </div>
                <footer class="dos-auth-brand-footer">
                    { "Mnemosyne Biosciences Ltd · Local operator console" }
                </footer>
            </aside>
            <section class="dos-auth-panel">
                <div class="dos-auth-panel-header">
                    <p>{ "Secure session" }</p>
                    <h2>{ "DASObjectStore login" }</h2>
                </div>
                if let Some(message) = &props.error_message {
                    <div class="dos-auth-error" role="alert">{ message.clone() }</div>
                }
                <form class="dos-auth-form" onsubmit={props.on_login.clone()}>
                    <label>
                        <span>{ "Username" }</span>
                        <input
                            type="text"
                            autocomplete="username"
                            value={props.username.clone()}
                            oninput={props.on_username.clone()}
                        />
                    </label>
                    <label>
                        <span>{ "Password" }</span>
                        <input
                            type="password"
                            autocomplete="current-password"
                            value={props.password.clone()}
                            oninput={props.on_password.clone()}
                        />
                    </label>
                    <button class="dos-auth-submit" type="submit" disabled={props.busy_label.is_some()}>
                        { props.busy_label.clone().unwrap_or_else(|| "Sign in".to_string()) }
                    </button>
                </form>
            </section>
        </section>
    }
}

#[derive(Clone, Debug, PartialEq, Properties)]
struct AuthenticatedWorkspaceProps {
    username: String,
    busy_label: Option<String>,
    api_base_path: String,
    on_logout: Callback<MouseEvent>,
}

#[function_component(AuthenticatedWorkspace)]
fn authenticated_workspace(props: &AuthenticatedWorkspaceProps) -> Html {
    let active_page = use_state(|| WorkspacePage::Home);

    html! {
        <section class="dos-workspace-shell">
            <header class="dos-topbar">
                <div class="dos-topbar-left">
                    <div class="dos-topbar-brand">
                        <span class="dos-brand-mark dos-brand-mark--small">{ "M" }</span>
                        <strong>{ "DASObjectStore" }</strong>
                    </div>
                    <nav class="dos-primary-nav" aria-label="Primary">
                        { for PRIMARY_NAVIGATION.into_iter().map(|page| {
                            let is_active = *active_page == page;
                            let active_page = active_page.clone();
                            html! {
                                <button
                                    type="button"
                                    class={classes!(is_active.then_some("is-active"))}
                                    aria-current={is_active.then_some("page")}
                                    data-page={page.id()}
                                    onclick={Callback::from(move |_| active_page.set(page))}
                                >
                                    { page.label() }
                                </button>
                            }
                        }) }
                    </nav>
                </div>
                <div class="dos-session-controls">
                    <span>{ props.username.clone() }</span>
                    <button type="button" onclick={props.on_logout.clone()} disabled={props.busy_label.is_some()}>
                        { props.busy_label.clone().unwrap_or_else(|| "Logout".to_string()) }
                    </button>
                </div>
            </header>
            { match *active_page {
                WorkspacePage::Home => html! {
                    <HomeDashboard api_base_path={props.api_base_path.clone()} />
                },
                WorkspacePage::Enclosures => html! {
                    <EnclosuresPage api_base_path={props.api_base_path.clone()} />
                },
                WorkspacePage::ObjectStores => html! {
                    <ObjectStoresPage api_base_path={props.api_base_path.clone()} />
                },
                WorkspacePage::Bioinformatics => html! {
                    <BioinformaticsPage api_base_path={props.api_base_path.clone()} />
                },
            } }
        </section>
    }
}

fn input_callback(state: UseStateHandle<String>) -> Callback<InputEvent> {
    Callback::from(move |event: InputEvent| {
        let input: HtmlInputElement = event.target_unchecked_into();
        state.set(input.value());
    })
}
