use crate::actions::{
    action_catalog, plan_action as build_action_plan, GuiActionCatalog, GuiActionPlan,
    GuiActionPlanError, GuiActionPlanRequest,
};
use crate::view::{api_health, ApiHealth};
use axum::{http::StatusCode, routing::get, routing::post, Json, Router};

pub fn gui_api_router() -> Router {
    Router::new()
        .route("/api/v1/health", get(health))
        .route("/api/v1/actions", get(actions))
        .route("/api/v1/actions/plan", post(plan_action))
}

async fn health() -> Json<ApiHealth> {
    Json(api_health())
}

async fn actions() -> Json<GuiActionCatalog> {
    Json(action_catalog())
}

async fn plan_action(
    Json(request): Json<GuiActionPlanRequest>,
) -> Result<Json<GuiActionPlan>, (StatusCode, Json<GuiActionPlanError>)> {
    build_action_plan(request)
        .map(Json)
        .map_err(|err| (StatusCode::BAD_REQUEST, Json(err)))
}

#[cfg(test)]
mod tests {
    use super::gui_api_router;

    #[test]
    fn builds_gui_api_router() {
        let _router = gui_api_router();
    }
}
