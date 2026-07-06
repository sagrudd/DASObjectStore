use crate::actions::{
    action_catalog, plan_action as build_action_plan, GuiActionCatalog, GuiActionPlan,
    GuiActionPlanError, GuiActionPlanRequest,
};
use crate::view::{api_health, ApiHealth};
use crate::workspaces::{DisksWorkspaceView, OverviewWorkspaceView};
use axum::{http::StatusCode, routing::get, routing::post, Json, Router};

pub fn gui_api_router() -> Router {
    Router::new()
        .route("/api/v1/health", get(health))
        .route("/api/v1/actions", get(actions))
        .route("/api/v1/actions/plan", post(plan_action))
        .route("/api/v1/workspaces/overview", get(overview_workspace))
        .route("/api/v1/workspaces/disks", get(disks_workspace))
}

async fn health() -> Json<ApiHealth> {
    Json(api_health())
}

async fn actions() -> Json<GuiActionCatalog> {
    Json(action_catalog())
}

async fn overview_workspace() -> Json<OverviewWorkspaceView> {
    Json(OverviewWorkspaceView::empty())
}

async fn disks_workspace() -> Json<DisksWorkspaceView> {
    Json(DisksWorkspaceView::empty())
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
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    #[test]
    fn builds_gui_api_router() {
        let _router = gui_api_router();
    }

    #[tokio::test]
    async fn overview_route_returns_workspace_payload() {
        let response = gui_api_router()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/workspaces/overview")
                    .body(Body::empty())
                    .expect("request builds"),
            )
            .await
            .expect("overview response");

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body bytes");
        let encoded: serde_json::Value = serde_json::from_slice(&body).expect("json body");

        assert_eq!(encoded["endpoints"]["endpoint_count"], 0);
        assert_eq!(encoded["attention"]["action_count"], 0);
    }

    #[tokio::test]
    async fn disks_route_returns_workspace_payload() {
        let response = gui_api_router()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/workspaces/disks")
                    .body(Body::empty())
                    .expect("request builds"),
            )
            .await
            .expect("disks response");

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body bytes");
        let encoded: serde_json::Value = serde_json::from_slice(&body).expect("json body");

        assert_eq!(encoded["disks"].as_array().expect("disks").len(), 0);
        assert_eq!(encoded["selected_disk_id"], serde_json::Value::Null);
        assert_eq!(encoded["warnings"].as_array().expect("warnings").len(), 0);
    }
}
