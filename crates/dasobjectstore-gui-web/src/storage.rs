const USERNAME_KEY: &str = "dasobjectstore.username";
const SESSION_TOKEN_KEY: &str = "dasobjectstore.session_token";

pub fn stored_session() -> Option<(String, String)> {
    let storage = local_storage()?;
    let username = storage.get_item(USERNAME_KEY).ok().flatten()?;
    let token = storage.get_item(SESSION_TOKEN_KEY).ok().flatten()?;
    if username.trim().is_empty() || token.trim().is_empty() {
        return None;
    }
    Some((username, token))
}

pub fn store_session(username: &str, session_token: &str) {
    if let Some(storage) = local_storage() {
        let _ = storage.set_item(USERNAME_KEY, username);
        let _ = storage.set_item(SESSION_TOKEN_KEY, session_token);
    }
}

pub fn clear_session() {
    if let Some(storage) = local_storage() {
        let _ = storage.remove_item(USERNAME_KEY);
        let _ = storage.remove_item(SESSION_TOKEN_KEY);
    }
}

fn local_storage() -> Option<web_sys::Storage> {
    web_sys::window().and_then(|window| window.local_storage().ok().flatten())
}
