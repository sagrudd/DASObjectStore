use crate::actions::{
    action_catalog, plan_action as build_action_plan, GuiActionCatalog, GuiActionPlan,
    GuiActionPlanError, GuiActionPlanRequest,
};
use crate::dashboard::{EnclosuresPageView, HomeDashboardView, ObjectStoresPageView};
use crate::view::{api_health, api_liveness, ApiHealth, ApiLiveness};
use crate::workspaces::{
    ActivityWorkspaceView, DisksWorkspaceView, EndpointsWorkspaceView, ObjectsWorkspaceView,
    OverviewWorkspaceView, ProductBioinformaticsWorkspaceView, ProductEnclosuresWorkspaceView,
    ProductHomeWorkspaceView, ProductObjectStoresWorkspaceView, StoresWorkspaceView,
};
use crate::RemoteUploadWorkspaceView;
use axum::{extract::Query, http::StatusCode, routing::get, routing::post, Json, Router};
use dasobjectstore_daemon::api::ApplianceTelemetryWindow;
use serde::Deserialize;

pub fn gui_api_router() -> Router {
    gui_api_router_without_redesign_dashboards()
        .route("/api/v1/dashboard/home", get(home_dashboard))
        .route("/api/v1/dashboard/enclosures", get(enclosures_dashboard))
        .route(
            "/api/v1/dashboard/object-stores",
            get(object_stores_dashboard),
        )
        .route(
            "/api/v1/workspaces/remote-upload",
            get(remote_upload_workspace),
        )
}

pub(crate) fn gui_api_router_without_redesign_dashboards() -> Router {
    Router::new()
        .route("/api/v1/health", get(health))
        .route("/api/v1/liveness", get(liveness))
        .route("/api/v1/actions", get(actions))
        .route("/api/v1/actions/plan", post(plan_action))
        .route("/api/v1/workspaces/overview", get(overview_workspace))
        .route("/api/v1/workspaces/home", get(product_home_workspace))
        .route(
            "/api/v1/workspaces/enclosures",
            get(product_enclosures_workspace),
        )
        .route(
            "/api/v1/workspaces/objectstores",
            get(product_objectstores_workspace),
        )
        .route(
            "/api/v1/workspaces/bioinformatics",
            get(product_bioinformatics_workspace),
        )
        // Legacy operations workspace routes remain available for compatibility.
        // The browser console now exposes Home, Enclosures, ObjectStores, and
        // Bioinformatics through the redesigned dashboard navigation.
        .route("/api/v1/workspaces/disks", get(disks_workspace))
        .route("/api/v1/workspaces/stores", get(stores_workspace))
        .route("/api/v1/workspaces/objects", get(objects_workspace))
        .route("/api/v1/workspaces/endpoints", get(endpoints_workspace))
        .route("/api/v1/workspaces/activity", get(activity_workspace))
}

async fn health() -> Json<ApiHealth> {
    Json(api_health())
}

async fn liveness() -> Json<ApiLiveness> {
    Json(api_liveness())
}

async fn actions() -> Json<GuiActionCatalog> {
    Json(action_catalog())
}

#[derive(Clone, Debug, Default, Deserialize)]
struct HomeDashboardQuery {
    #[serde(default)]
    telemetry_window: Option<ApplianceTelemetryWindow>,
    #[serde(default)]
    window: Option<ApplianceTelemetryWindow>,
}

impl HomeDashboardQuery {
    fn selected_window(&self) -> ApplianceTelemetryWindow {
        self.telemetry_window
            .or(self.window)
            .unwrap_or_else(ApplianceTelemetryWindow::default)
    }
}

async fn home_dashboard(Query(query): Query<HomeDashboardQuery>) -> Json<HomeDashboardView> {
    Json(crate::home_aggregator::live_home_dashboard_for_window(
        query.selected_window(),
    ))
}

async fn enclosures_dashboard() -> Json<EnclosuresPageView> {
    Json(crate::enclosures_aggregator::live_enclosures_dashboard())
}

async fn object_stores_dashboard() -> Json<ObjectStoresPageView> {
    Json(crate::object_stores_aggregator::live_object_stores_dashboard())
}

async fn overview_workspace() -> Json<OverviewWorkspaceView> {
    Json(OverviewWorkspaceView::empty())
}

async fn product_home_workspace() -> Json<ProductHomeWorkspaceView> {
    Json(ProductHomeWorkspaceView::bootstrap())
}

async fn product_enclosures_workspace() -> Json<ProductEnclosuresWorkspaceView> {
    Json(ProductEnclosuresWorkspaceView::bootstrap())
}

async fn product_objectstores_workspace() -> Json<ProductObjectStoresWorkspaceView> {
    Json(ProductObjectStoresWorkspaceView::bootstrap())
}

async fn product_bioinformatics_workspace() -> Json<ProductBioinformaticsWorkspaceView> {
    Json(ProductBioinformaticsWorkspaceView::bootstrap())
}

#[derive(Clone, Debug, Default, Deserialize)]
struct RemoteUploadWorkspaceQuery {
    #[serde(default)]
    store_id: Option<String>,
}

async fn remote_upload_workspace(
    Query(query): Query<RemoteUploadWorkspaceQuery>,
) -> Result<Json<RemoteUploadWorkspaceView>, StatusCode> {
    let Some(store_id) = query
        .store_id
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    else {
        return Err(StatusCode::BAD_REQUEST);
    };
    let view = crate::remote_upload_aggregator::live_remote_upload_workspace_for_user_targeted(
        "developer".to_string(),
        Vec::new(),
        false,
        Some(store_id),
    );
    if !view.stores.iter().any(|store| store.store_id == store_id) {
        return Err(StatusCode::FORBIDDEN);
    }
    Ok(Json(view))
}

async fn disks_workspace() -> Json<DisksWorkspaceView> {
    Json(DisksWorkspaceView::empty())
}

async fn stores_workspace() -> Json<StoresWorkspaceView> {
    Json(StoresWorkspaceView::empty())
}

async fn objects_workspace() -> Json<ObjectsWorkspaceView> {
    Json(ObjectsWorkspaceView::empty())
}

async fn endpoints_workspace() -> Json<EndpointsWorkspaceView> {
    Json(crate::endpoints_aggregator::live_endpoints_workspace())
}

async fn activity_workspace() -> Json<ActivityWorkspaceView> {
    let daemon_jobs = crate::daemon_bridge::DaemonBridge::shared_packaged()
        .call_message(move || {
            let client = dasobjectstore_daemon::DaemonClient::new(
                dasobjectstore_daemon::UnixSocketDaemonTransport::for_bounded_bridge(
                    dasobjectstore_daemon::DaemonRuntimeConfig::default_packaged().socket_path,
                ),
            );
            client
                .list_jobs(dasobjectstore_daemon::DaemonJobListRequest {
                    limit: Some(crate::activity_aggregator::ACTIVITY_JOB_LIMIT),
                })
                .map_err(|err| err.to_string())
        })
        .await
        .map_err(|error| match error {
            crate::daemon_bridge::DaemonBridgeError::Client(error) => error.message,
            crate::daemon_bridge::DaemonBridgeError::Busy => {
                "daemon control capacity is saturated; retry shortly".to_string()
            }
            crate::daemon_bridge::DaemonBridgeError::CircuitOpen => {
                "daemon control is temporarily degraded; retry shortly".to_string()
            }
            crate::daemon_bridge::DaemonBridgeError::Deadline => {
                "daemon activity request exceeded its deadline; retry shortly".to_string()
            }
            crate::daemon_bridge::DaemonBridgeError::Join(message) => message,
        });
    Json(crate::activity_aggregator::live_activity_workspace_with_daemon_result(daemon_jobs))
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
    use super::super::daemon_bridge::DaemonBridge;
    use super::gui_api_router;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use serde_json::json;
    use std::time::Duration;
    use tower::ServiceExt;

    #[test]
    fn builds_gui_api_router() {
        let _router = gui_api_router();
    }

    #[tokio::test]
    async fn home_dashboard_route_returns_redesign_payload() {
        let response = gui_api_router()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/dashboard/home")
                    .body(Body::empty())
                    .expect("request builds"),
            )
            .await
            .expect("home dashboard response");

        assert_eq!(response.status(), StatusCode::OK);
        let encoded = response_json(response).await;

        assert_eq!(encoded["schema_version"], "dasobjectstore.web_redesign.v1");
        assert_ne!(encoded["health"]["label"], "Inventory pending");
        assert!(encoded["health"]["last_checked_at_utc"].is_string());
        assert!(encoded["drives"]["mounted"].is_number());
        assert!(encoded["capacity"]["free_tib"].is_string());
        assert_eq!(encoded["telemetry_window"]["selected"], "one_hour");
        assert_eq!(encoded["throughput_7d"]["window_days"], 7);
        assert!(encoded["memory_stress"]["state"].is_string());
        assert_eq!(encoded["create_object_store"]["enabled"], false);
        assert_eq!(
            encoded["create_object_store"]["action_kind"],
            "store_create"
        );
    }

    #[tokio::test]
    async fn liveness_route_does_not_require_daemon_round_trip() {
        let response = gui_api_router()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/liveness")
                    .body(Body::empty())
                    .expect("request builds"),
            )
            .await
            .expect("liveness response");

        assert_eq!(response.status(), StatusCode::OK);
        let encoded = response_json(response).await;
        assert_eq!(encoded["status"], "ready");
        assert_eq!(encoded["service"], "dasobjectstore-gui-api");
        assert!(encoded["instance_id"].is_string());
    }

    #[tokio::test]
    async fn health_and_liveness_stay_responsive_while_daemon_bridge_is_saturated() {
        let bridge = DaemonBridge::with_capacity_and_deadline(1, Duration::from_millis(20));
        let (entered_sender, entered_receiver) = tokio::sync::oneshot::channel();
        let (release_sender, release_receiver) = tokio::sync::oneshot::channel();
        let blocked = {
            let bridge = bridge.clone();
            tokio::spawn(async move {
                bridge
                    .call_message(move || {
                        entered_sender.send(()).expect("blocked call entered");
                        release_receiver.blocking_recv().expect("release signal");
                        Ok(())
                    })
                    .await
            })
        };
        tokio::time::timeout(Duration::from_secs(1), entered_receiver)
            .await
            .expect("blocked call starts")
            .expect("entered signal");
        let response = gui_api_router()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/liveness")
                    .body(Body::empty())
                    .expect("liveness request builds"),
            )
            .await
            .expect("liveness response");
        assert_eq!(response.status(), StatusCode::OK);
        let health_response = gui_api_router()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/health")
                    .body(Body::empty())
                    .expect("health request builds"),
            )
            .await
            .expect("health response");
        assert_eq!(health_response.status(), StatusCode::OK);
        release_sender.send(()).expect("release blocked call");
        blocked
            .await
            .expect("blocked call joins")
            .expect("call succeeds");
    }

    #[tokio::test]
    async fn home_dashboard_route_accepts_telemetry_window_query() {
        let response = gui_api_router()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/dashboard/home?telemetry_window=three_months")
                    .body(Body::empty())
                    .expect("request builds"),
            )
            .await
            .expect("home dashboard response");

        assert_eq!(response.status(), StatusCode::OK);
        let encoded = response_json(response).await;

        assert_eq!(encoded["telemetry_window"]["selected"], "three_months");
        assert_eq!(encoded["telemetry_window"]["selected_label"], "3 months");
    }

    #[tokio::test]
    async fn enclosures_dashboard_route_returns_redesign_payload() {
        let response = gui_api_router()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/dashboard/enclosures")
                    .body(Body::empty())
                    .expect("request builds"),
            )
            .await
            .expect("enclosures dashboard response");

        assert_eq!(response.status(), StatusCode::OK);
        let encoded = response_json(response).await;

        assert_eq!(encoded["schema_version"], "dasobjectstore.web_redesign.v1");
        assert!(encoded["generated_at_utc"].is_string());
        assert_eq!(encoded["add_enclosure"]["action_kind"], "enclosure_add");
        assert!(encoded["add_enclosure"]["enabled"].is_boolean());
        assert!(encoded["enclosures"].is_array());
        assert!(encoded["warnings"].is_array());
        assert!(!encoded["warnings"]
            .as_array()
            .expect("warnings")
            .iter()
            .any(|warning| warning["code"] == "enclosure_inventory_pending"));
    }

    #[tokio::test]
    async fn object_stores_dashboard_route_returns_redesign_payload() {
        let response = gui_api_router()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/dashboard/object-stores")
                    .body(Body::empty())
                    .expect("request builds"),
            )
            .await
            .expect("object stores dashboard response");

        assert_eq!(response.status(), StatusCode::OK);
        let encoded = response_json(response).await;

        assert_eq!(encoded["schema_version"], "dasobjectstore.web_redesign.v1");
        assert!(encoded["generated_at_utc"].is_string());
        assert!(encoded["stores"].is_array());
        assert!(!encoded["warnings"]
            .as_array()
            .expect("warnings")
            .iter()
            .any(|warning| warning["code"] == "object_store_inventory_pending"));
        assert_eq!(encoded["create_object_store"]["enabled"], false);
        assert_eq!(
            encoded["create_object_store"]["defaults"]["required_copies"],
            2
        );
        assert_eq!(
            encoded["create_object_store"]["confirmation_required"],
            true
        );
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
    async fn product_home_route_returns_dashboard_contract() {
        let response = gui_api_router()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/workspaces/home")
                    .body(Body::empty())
                    .expect("request builds"),
            )
            .await
            .expect("home response");

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body bytes");
        let encoded: serde_json::Value = serde_json::from_slice(&body).expect("json body");

        assert_eq!(
            encoded["schema_version"],
            "dasobjectstore.product_workspaces.v1"
        );
        assert_eq!(encoded["health"]["drive_count"], 0);
        assert_eq!(encoded["capacity"]["available_bytes"], 0);
        assert_eq!(encoded["smart_warnings"].as_array().unwrap().len(), 0);
        assert_eq!(encoded["warnings"].as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn product_enclosures_route_returns_admin_gated_workflow() {
        let response = gui_api_router()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/workspaces/enclosures")
                    .body(Body::empty())
                    .expect("request builds"),
            )
            .await
            .expect("enclosures response");

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body bytes");
        let encoded: serde_json::Value = serde_json::from_slice(&body).expect("json body");

        assert_eq!(encoded["administrator_actions_enabled"], false);
        assert_eq!(encoded["add_enclosure"]["enabled"], false);
        assert_eq!(
            encoded["add_enclosure"]["steps"].as_array().unwrap().len(),
            5
        );
        assert_eq!(encoded["enclosures"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn product_objectstores_route_returns_group_policy_contract() {
        let response = gui_api_router()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/workspaces/objectstores")
                    .body(Body::empty())
                    .expect("request builds"),
            )
            .await
            .expect("objectstores response");

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body bytes");
        let encoded: serde_json::Value = serde_json::from_slice(&body).expect("json body");

        assert_eq!(
            encoded["groups_file_path"],
            "/opt/dasobjectstore/groups.json"
        );
        assert_eq!(encoded["create"]["requires_sudo_administrator"], true);
        assert!(encoded["create"]["supported_store_types"]
            .as_array()
            .unwrap()
            .contains(&json!("pod5")));
        assert_eq!(encoded["object_stores"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn product_bioinformatics_route_returns_readiness_cards() {
        let response = gui_api_router()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/workspaces/bioinformatics")
                    .body(Body::empty())
                    .expect("request builds"),
            )
            .await
            .expect("bioinformatics response");

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body bytes");
        let encoded: serde_json::Value = serde_json::from_slice(&body).expect("json body");

        assert_eq!(encoded["available"], true);
        assert!(encoded["supported_object_types"]
            .as_array()
            .unwrap()
            .contains(&json!("POD5")));
        assert!(encoded["readiness_cards"]
            .as_array()
            .unwrap()
            .iter()
            .any(|card| card["label"] == json!("FASTQ / FASTQ.GZ")));
        assert!(encoded["readiness_cards"]
            .as_array()
            .unwrap()
            .iter()
            .any(|card| card["label"] == json!("ENA / SRA")));
        assert!(encoded["derivation_sources"]
            .as_array()
            .unwrap()
            .iter()
            .any(|source| source["source_kind"] == json!("object_store_metadata")));
        assert!(encoded["derivation_sources"]
            .as_array()
            .unwrap()
            .iter()
            .any(|source| source["source_kind"] == json!("subobject_metadata")));
        assert!(encoded["derivation_sources"]
            .as_array()
            .unwrap()
            .iter()
            .any(|source| source["source_kind"] == json!("mneion_binding")));
        assert_eq!(
            encoded["sequencing_runs"][0]["label"],
            json!("Sequencing run provenance")
        );
        assert_eq!(
            encoded["object_lineage"][0]["label"],
            json!("Object lineage")
        );
        assert!(encoded["workflow_handoffs"]
            .as_array()
            .unwrap()
            .iter()
            .any(|card| card["label"] == json!("Genome/transcriptome handoff")));
        assert_eq!(
            encoded["governance_bindings"][0]["state"],
            json!("binding_required")
        );
    }

    #[tokio::test]
    async fn remote_upload_workspace_route_requires_explicit_target() {
        let response = gui_api_router()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/workspaces/remote-upload")
                    .body(Body::empty())
                    .expect("request builds"),
            )
            .await
            .expect("remote upload response");

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn remote_upload_workspace_route_rejects_unavailable_target() {
        let response = gui_api_router()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/workspaces/remote-upload?store_id=codex")
                    .body(Body::empty())
                    .expect("request builds"),
            )
            .await
            .expect("remote upload response");

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
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

    #[tokio::test]
    async fn stores_route_returns_workspace_payload() {
        let response = gui_api_router()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/workspaces/stores")
                    .body(Body::empty())
                    .expect("request builds"),
            )
            .await
            .expect("stores response");

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body bytes");
        let encoded: serde_json::Value = serde_json::from_slice(&body).expect("json body");

        assert_eq!(encoded["stores"].as_array().expect("stores").len(), 0);
        assert_eq!(encoded["selected_store_id"], serde_json::Value::Null);
        assert_eq!(encoded["warnings"].as_array().expect("warnings").len(), 0);
    }

    #[tokio::test]
    async fn objects_route_returns_workspace_payload() {
        let response = gui_api_router()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/workspaces/objects")
                    .body(Body::empty())
                    .expect("request builds"),
            )
            .await
            .expect("objects response");

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body bytes");
        let encoded: serde_json::Value = serde_json::from_slice(&body).expect("json body");

        assert_eq!(encoded["objects"].as_array().expect("objects").len(), 0);
        assert_eq!(encoded["selected_object_id"], serde_json::Value::Null);
        assert_eq!(encoded["filters"]["store_id"], serde_json::Value::Null);
        assert_eq!(encoded["warnings"].as_array().expect("warnings").len(), 0);
    }

    #[tokio::test]
    async fn endpoints_route_returns_workspace_payload() {
        let response = gui_api_router()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/workspaces/endpoints")
                    .body(Body::empty())
                    .expect("request builds"),
            )
            .await
            .expect("endpoints response");

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body bytes");
        let encoded: serde_json::Value = serde_json::from_slice(&body).expect("json body");

        assert!(encoded["inventory"]["endpoint_count"].is_number());
        assert!(encoded["inventory"]["degraded_endpoint_count"].is_number());
        assert!(encoded["inventory"]["binding_count"].is_number());
        assert!(encoded["inventory"]["warnings"].as_array().is_some());
    }

    #[tokio::test]
    async fn activity_route_returns_workspace_payload() {
        let response = gui_api_router()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/workspaces/activity")
                    .body(Body::empty())
                    .expect("request builds"),
            )
            .await
            .expect("activity response");

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body bytes");
        let encoded: serde_json::Value = serde_json::from_slice(&body).expect("json body");

        assert_eq!(encoded["ingest"], serde_json::Value::Null);
        assert_eq!(encoded["destage"], serde_json::Value::Null);
        assert_eq!(
            encoded["categories"].as_array().expect("categories").len(),
            8
        );
        assert_eq!(encoded["categories"][0]["kind"], "system_administration");
        assert!(encoded["tasks"].as_array().is_some());
        assert!(encoded["warnings"].as_array().is_some());
    }

    #[tokio::test]
    async fn liveness_remains_independent_while_activity_daemon_round_trip_degrades() {
        let liveness = gui_api_router().oneshot(
            Request::builder()
                .uri("/api/v1/liveness")
                .body(Body::empty())
                .expect("liveness request builds"),
        );
        let activity = gui_api_router().oneshot(
            Request::builder()
                .uri("/api/v1/workspaces/activity")
                .body(Body::empty())
                .expect("activity request builds"),
        );

        let (liveness, activity) = tokio::join!(liveness, activity);
        let liveness = liveness.expect("liveness response");
        let activity = activity.expect("activity response");
        assert_eq!(liveness.status(), StatusCode::OK);
        assert_eq!(activity.status(), StatusCode::OK);

        let body = axum::body::to_bytes(activity.into_body(), usize::MAX)
            .await
            .expect("activity body bytes");
        let encoded: serde_json::Value = serde_json::from_slice(&body).expect("activity json");
        assert!(encoded["warnings"].as_array().is_some());
        assert_eq!(
            encoded["categories"].as_array().expect("categories").len(),
            8
        );
    }

    #[tokio::test]
    async fn actions_route_advertises_store_subobject_and_enclosure_preparation() {
        let response = gui_api_router()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/actions")
                    .body(Body::empty())
                    .expect("request builds"),
            )
            .await
            .expect("actions response");

        assert_eq!(response.status(), StatusCode::OK);
        let encoded = response_json(response).await;
        let actions = encoded["actions"].as_array().expect("actions");

        assert!(actions.iter().any(|action| {
            action["kind"] == "store_create"
                && action["safety"] == "configuration_mutation"
                && action["confirmation_required"] == true
        }));
        assert!(actions.iter().any(|action| {
            action["kind"] == "store_configure"
                && action["safety"] == "configuration_mutation"
                && action["required_fields"]
                    == json!([
                        "store_id",
                        "store_class",
                        "store_copies",
                        "writer_group",
                        "capacity_behavior",
                        "retention",
                        "endpoint_export_mode"
                    ])
                && action["confirmation_required"] == true
        }));
        assert!(actions.iter().any(|action| {
            action["kind"] == "subobject_create"
                && action["safety"] == "configuration_mutation"
                && action["confirmation_required"] == true
        }));
        assert!(actions.iter().any(|action| {
            action["kind"] == "enclosure_prepare"
                && action["safety"] == "destructive_storage_preparation"
                && action["required_fields"]
                    == json!([
                        "ssd_device",
                        "hdd_devices",
                        "allow_format",
                        "confirmation_phrase"
                    ])
                && action["confirmation_required"] == true
        }));
    }

    #[tokio::test]
    async fn action_plan_route_returns_store_create_plan() {
        let response = post_json(
            "/api/v1/actions/plan",
            json!({
                "action": "store_create",
                "store_id": "generated-data",
                "store_class": "generated_data",
                "store_copies": 2,
                "writer_group": "mnemosyne",
                "ssd_root": "/srv/dasobjectstore/ssd"
            }),
        )
        .await;

        assert_eq!(response.status(), StatusCode::OK);
        let encoded = response_json(response).await;

        assert_eq!(encoded["action"], "store_create");
        assert_eq!(encoded["confirmation_required"], true);
        assert_eq!(encoded["mutates_pool"], false);
        assert_eq!(
            encoded["argv"],
            json!([
                "dasobjectstore",
                "store",
                "create",
                "generated-data",
                "--class",
                "generated_data",
                "--copies",
                "2",
                "--writer-group",
                "mnemosyne",
                "--ssd-root",
                "/srv/dasobjectstore/ssd",
                "--json"
            ])
        );
    }

    #[tokio::test]
    async fn action_plan_route_returns_store_configure_plan() {
        let response = post_json(
            "/api/v1/actions/plan",
            json!({
                "action": "store_configure",
                "store_id": "generated-data",
                "store_class": "generated_data",
                "store_copies": 2,
                "writer_group": "mnemosyne",
                "ssd_root": "/srv/dasobjectstore/ssd",
                "public": false,
                "writeable": true,
                "capacity_behavior": "backpressure_by_priority",
                "retention": "tombstone_then_gc",
                "endpoint_export_mode": "s3"
            }),
        )
        .await;

        assert_eq!(response.status(), StatusCode::OK);
        let encoded = response_json(response).await;

        assert_eq!(encoded["action"], "store_configure");
        assert_eq!(encoded["confirmation_required"], true);
        assert_eq!(encoded["mutates_pool"], false);
        assert_eq!(
            encoded["argv"],
            json!([
                "dasobjectstore",
                "store",
                "configure",
                "generated-data",
                "--class",
                "generated_data",
                "--copies",
                "2",
                "--writer-group",
                "mnemosyne",
                "--capacity-behavior",
                "backpressure_by_priority",
                "--retention",
                "tombstone_then_gc",
                "--export-mode",
                "s3",
                "--public",
                "false",
                "--writeable",
                "true",
                "--ssd-root",
                "/srv/dasobjectstore/ssd",
                "--json"
            ])
        );
    }

    #[tokio::test]
    async fn action_plan_route_rejects_invalid_store_create_request() {
        let response = post_json(
            "/api/v1/actions/plan",
            json!({
                "action": "store_create",
                "store_id": "generated-data"
            }),
        )
        .await;

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let encoded = response_json(response).await;

        assert_eq!(encoded["action"], "store_create");
        assert_eq!(encoded["missing_fields"], json!(["store_class"]));
    }

    #[tokio::test]
    async fn action_plan_route_rejects_invalid_store_create_policy_values() {
        let response = post_json(
            "/api/v1/actions/plan",
            json!({
                "action": "store_create",
                "store_id": "generated-data",
                "store_class": "unknown",
                "store_copies": 4,
                "capacity_behavior": "fast",
                "retention": "forever",
                "endpoint_export_mode": "ftp"
            }),
        )
        .await;

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let encoded = response_json(response).await;

        assert_eq!(encoded["action"], "store_create");
        assert_eq!(
            encoded["missing_fields"],
            json!([
                "store_class",
                "store_copies",
                "capacity_behavior",
                "retention",
                "endpoint_export_mode"
            ])
        );
    }

    #[tokio::test]
    async fn action_plan_route_returns_subobject_create_plan() {
        let response = post_json(
            "/api/v1/actions/plan",
            json!({
                "action": "subobject_create",
                "subobject_name": "Vervet",
                "parent_subobject_name": "Xenognostikon",
                "subobject_inherits_object_type": false,
                "subobject_object_type": "pod5",
                "subobject_s3_routing": "dedicated_prefix"
            }),
        )
        .await;

        assert_eq!(response.status(), StatusCode::OK);
        let encoded = response_json(response).await;

        assert_eq!(encoded["action"], "subobject_create");
        assert_eq!(encoded["confirmation_required"], true);
        assert_eq!(
            encoded["argv"],
            json!([
                "dasobjectstore",
                "subobject",
                "create",
                "Vervet",
                "--parent",
                "Xenognostikon"
            ])
        );
    }

    #[tokio::test]
    async fn action_plan_route_rejects_invalid_subobject_review_policy() {
        let response = post_json(
            "/api/v1/actions/plan",
            json!({
                "action": "subobject_create",
                "subobject_name": "Vervet",
                "parent_subobject_name": "Xenognostikon",
                "subobject_inherits_object_type": false,
                "subobject_object_type": "not_a_real_type",
                "subobject_s3_routing": "ftp"
            }),
        )
        .await;

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let encoded = response_json(response).await;

        assert_eq!(encoded["action"], "subobject_create");
        assert_eq!(
            encoded["missing_fields"],
            json!(["subobject_object_type", "subobject_s3_routing"])
        );
    }

    #[tokio::test]
    async fn action_plan_route_rejects_invalid_subobject_create_request() {
        let response = post_json(
            "/api/v1/actions/plan",
            json!({
                "action": "subobject_create",
                "subobject_name": "Vervet",
                "parent_store_id": "ENA",
                "parent_subobject_name": "Xenognostikon"
            }),
        )
        .await;

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let encoded = response_json(response).await;

        assert_eq!(encoded["action"], "subobject_create");
        assert_eq!(
            encoded["missing_fields"],
            json!(["parent_store_id_or_parent_subobject_name"])
        );
    }

    #[tokio::test]
    async fn action_plan_route_returns_enclosure_prepare_plan() {
        let response = post_json(
            "/api/v1/actions/plan",
            json!({
                "action": "enclosure_prepare",
                "ssd_device": "/dev/disk/by-id/nvme-ssd",
                "hdd_devices": [
                    "qnap-1057=/dev/disk/by-id/usb-qnap-1057",
                    "qnap-1058=/dev/disk/by-id/usb-qnap-1058"
                ],
                "mount_root": "/srv/dasobjectstore",
                "filesystem": "ext4",
                "allow_format": true,
                "existing_data_acknowledged": true,
                "confirmation_phrase": "confirm prepare das"
            }),
        )
        .await;

        assert_eq!(response.status(), StatusCode::OK);
        let encoded = response_json(response).await;

        assert_eq!(encoded["action"], "enclosure_prepare");
        assert_eq!(encoded["mutates_pool"], true);
        assert_eq!(encoded["confirmation_required"], true);
        assert_eq!(
            encoded["argv"],
            json!([
                "dasobjectstore",
                "disk",
                "prepare-das",
                "--ssd-device",
                "/dev/disk/by-id/nvme-ssd",
                "--hdd-device",
                "qnap-1057=/dev/disk/by-id/usb-qnap-1057",
                "--hdd-device",
                "qnap-1058=/dev/disk/by-id/usb-qnap-1058",
                "--mount-root",
                "/srv/dasobjectstore",
                "--filesystem",
                "ext4",
                "--allow-format",
                "--acknowledge-existing-data",
                "--confirm",
                "confirm prepare das"
            ])
        );
    }

    async fn post_json(path: &str, body: serde_json::Value) -> axum::response::Response {
        gui_api_router()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(path)
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .expect("request builds"),
            )
            .await
            .expect("post response")
    }

    async fn response_json(response: axum::response::Response) -> serde_json::Value {
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body bytes");
        serde_json::from_slice(&body).expect("json body")
    }
}
