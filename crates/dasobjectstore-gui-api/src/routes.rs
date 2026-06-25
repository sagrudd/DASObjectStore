use crate::view::{api_health, ApiHealth};
use axum::{routing::get, Json, Router};

pub fn gui_api_router() -> Router {
    Router::new().route("/api/v1/health", get(health))
}

async fn health() -> Json<ApiHealth> {
    Json(api_health())
}

#[cfg(test)]
mod tests {
    use super::gui_api_router;

    #[test]
    fn builds_gui_api_router() {
        let _router = gui_api_router();
    }
}
