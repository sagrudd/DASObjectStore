use axum::{
    body::Body,
    extract::Extension,
    http::{header::COOKIE, HeaderValue, Request, StatusCode},
    routing::{get, post},
    Router,
};
use dasobjectstore_gui_api::{
    accept_host_authenticated_context, AuthenticatedGuiActor, EasyconnectS3EndpointConfig,
    HostAuthenticatedContext, HostAuthenticationAuthority, HostAuthenticationContextVerifier,
    StandaloneS3ConnectionDescriptor, HOST_AUTH_AUDIENCE, HOST_AUTH_CONTEXT_SCHEMA_VERSION,
};
use dasobjectstore_mnemosyne::{
    accept_monas_host_session, accept_synoptikon_host_session, monas_dasobjectstore_api_router,
    monas_federated_router, preverified_dasobjectstore_router,
    preverified_dasobjectstore_web_router, synoptikon_federated_router, HostSessionAdapterError,
    MonasHostSessionIssue, StorageAuthority, SynoptikonHostRequestAuthentication,
    SynoptikonIntegratedAcceptedSession, SynoptikonIntegratedHostBoundaryContext,
    SynoptikonIntegratedSessionIssue, SynoptikonLiveSessionVerifier, DASOBJECTSTORE_PRODUCT_ID,
    FEDERATED_CSRF_HEADER, REQUEST_CONTEXT_SCHEMA_VERSION,
};
use prosopikon_core::ProsopikonAuthStore;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tower::ServiceExt;

#[tokio::test]
async fn preverified_router_rejects_legacy_credentials_and_missing_context() {
    let app = preverified_dasobjectstore_router(Router::new(), Some(test_s3_endpoint()));
    for request in [
        Request::builder()
            .uri("/api/v1/host-session")
            .header(COOKIE, "monas_session=operator:legacy-token")
            .body(Body::empty())
            .expect("cookie request"),
        Request::builder()
            .uri("/api/v1/remote/host-context")
            .header("authorization", "Bearer legacy-token")
            .body(Body::empty())
            .expect("bearer request"),
    ] {
        assert_eq!(
            app.clone()
                .oneshot(request)
                .await
                .expect("response")
                .status(),
            StatusCode::UNAUTHORIZED
        );
    }
}

#[tokio::test]
async fn preverified_router_accepts_only_inserted_verified_context() {
    let root = temp_root("preverified-only");
    let store = registered_store(&root);
    let login = store
        .login_with_session_ttl_seconds("operator", "secret", Some(3_600))
        .expect("login");
    let issue = MonasHostSessionIssue {
        username: "operator".to_owned(),
        session_token: login.session_token,
        correlation_id: "corr-preverified-1".to_owned(),
        csrf_binding_sha256: csrf_binding(),
    };
    let verified = accept_monas_host_session(&store, &issue, unix_now()).expect("verified fixture");
    let app = preverified_dasobjectstore_router(Router::new(), Some(test_s3_endpoint()))
        .layer(Extension(verified));
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/host-session")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn preverified_web_router_requires_the_verified_host_context() {
    let host_routes = Router::new().route("/", get(|| async { "verified DAS shell" }));
    let denied = preverified_dasobjectstore_web_router(host_routes.clone())
        .oneshot(
            Request::builder()
                .uri("/")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(denied.status(), StatusCode::UNAUTHORIZED);

    let root = temp_root("preverified-web");
    let store = registered_store(&root);
    let login = store
        .login_with_session_ttl_seconds("operator", "secret", Some(3_600))
        .expect("login");
    let verified = accept_monas_host_session(
        &store,
        &MonasHostSessionIssue {
            username: "operator".to_owned(),
            session_token: login.session_token,
            correlation_id: "corr-preverified-web".to_owned(),
            csrf_binding_sha256: csrf_binding(),
        },
        unix_now(),
    )
    .expect("verified fixture");
    let accepted = preverified_dasobjectstore_web_router(host_routes)
        .layer(Extension(verified))
        .oneshot(
            Request::builder()
                .uri("/")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(accepted.status(), StatusCode::OK);
    cleanup(&root);
}

#[tokio::test]
async fn preverified_router_exposes_host_composed_api_only_with_verified_context() {
    let app = preverified_dasobjectstore_router(Router::new(), None);
    let denied = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/health")
                .header("x-dasobjectstore-username", "operator")
                .header("authorization", "Bearer legacy-token")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(denied.status(), StatusCode::UNAUTHORIZED);

    let root = temp_root("preverified-dashboard");
    let store = registered_store(&root);
    let login = store
        .login_with_session_ttl_seconds("operator", "secret", Some(3_600))
        .expect("login");
    let verified = accept_monas_host_session(
        &store,
        &MonasHostSessionIssue {
            username: "operator".to_owned(),
            session_token: login.session_token,
            correlation_id: "corr-preverified-dashboard".to_owned(),
            csrf_binding_sha256: csrf_binding(),
        },
        unix_now(),
    )
    .expect("verified fixture");
    let accepted = app
        .layer(Extension(verified))
        .oneshot(
            Request::builder()
                .uri("/api/v1/health")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(accepted.status(), StatusCode::OK);
    cleanup(&root);
}

/// Deletion-acceptance proof for the host composition. This is intentionally
/// an end-to-end route reachability audit rather than a source-text check: a
/// future router merge must not make the retained standalone local authority
/// routes reachable through a verified Monas/Pistis product mount.
#[tokio::test]
async fn preverified_host_composition_has_no_legacy_human_authority_reachability() {
    let verified_app = preverified_dasobjectstore_router(Router::new(), None)
        .layer(Extension(verified_host_context(&["storage_administrator"])));

    for (method, path) in [
        ("POST", "/api/register"),
        ("POST", "/api/login"),
        ("POST", "/api/logout"),
        ("POST", "/api/session"),
        ("POST", "/api/v1/remote/authenticate"),
        ("GET", "/api/v1/workspaces/users-groups"),
    ] {
        let response = verified_app
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(path)
                    .header(FEDERATED_CSRF_HEADER, csrf_binding())
                    .body(Body::from("{}"))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(
            response.status(),
            StatusCode::NOT_FOUND,
            "legacy local-authority route {method} {path} must not be mounted"
        );
    }

    // Raw cookies, bearer/session headers, and claimed OS identities cannot
    // establish a host actor or reach a daemon-backed operation. Only the
    // embedding Monas/Pistis middleware may insert the verified context.
    for (method, path) in [
        ("GET", "/api/v1/dashboard/home"),
        ("GET", "/api/v1/object-stores/store-1/browser"),
        ("POST", "/api/v1/workspaces/admin/ingest-control"),
    ] {
        let response = preverified_dasobjectstore_router(Router::new(), None)
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(path)
                    .header(COOKIE, "monas_session=root:raw-cookie")
                    .header("authorization", "Bearer raw-local-session")
                    .header("x-dasobjectstore-username", "root")
                    .header("x-dasobjectstore-session-token", "local-session")
                    .header(FEDERATED_CSRF_HEADER, csrf_binding())
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"action":"pause","reason":"raw authority","dry_run":true}"#,
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(
            response.status(),
            StatusCode::UNAUTHORIZED,
            "raw authority must not reach host route {method} {path}"
        );
    }
}

#[tokio::test]
async fn preverified_enclosures_dashboard_uses_closed_verified_pistis_roles() {
    let request = || {
        Request::builder()
            .uri("/api/v1/dashboard/enclosures")
            .body(Body::empty())
            .expect("request")
    };

    let denied = preverified_dasobjectstore_router(Router::new(), None)
        .oneshot(request())
        .await
        .expect("response");
    assert_eq!(denied.status(), StatusCode::UNAUTHORIZED);

    let unrelated_role = preverified_dasobjectstore_router(Router::new(), None)
        .layer(Extension(verified_host_context(&["authenticated"])))
        .oneshot(request())
        .await
        .expect("response");
    assert_eq!(unrelated_role.status(), StatusCode::FORBIDDEN);

    let viewer = preverified_dasobjectstore_router(Router::new(), None)
        .layer(Extension(verified_host_context(&["storage_viewer"])))
        .oneshot(request())
        .await
        .expect("response");
    assert_eq!(viewer.status(), StatusCode::OK);
    let viewer_body = axum::body::to_bytes(viewer.into_body(), 128 * 1024)
        .await
        .expect("viewer body");
    let viewer: serde_json::Value = serde_json::from_slice(&viewer_body).expect("viewer JSON");
    assert_eq!(viewer["add_enclosure"]["administrator"], false);

    let administrator = preverified_dasobjectstore_router(Router::new(), None)
        .layer(Extension(verified_host_context(&["storage_administrator"])))
        .oneshot(request())
        .await
        .expect("response");
    assert_eq!(administrator.status(), StatusCode::OK);
    let administrator_body = axum::body::to_bytes(administrator.into_body(), 128 * 1024)
        .await
        .expect("administrator body");
    let administrator: serde_json::Value =
        serde_json::from_slice(&administrator_body).expect("administrator JSON");
    assert_eq!(administrator["add_enclosure"]["administrator"], true);
}

#[tokio::test]
async fn preverified_object_stores_dashboard_does_not_derive_local_group_authority() {
    let request = || {
        Request::builder()
            .uri("/api/v1/dashboard/object-stores")
            .body(Body::empty())
            .expect("request")
    };

    let unrelated_role = preverified_dasobjectstore_router(Router::new(), None)
        .layer(Extension(verified_host_context(&["authenticated"])))
        .oneshot(request())
        .await
        .expect("response");
    assert_eq!(unrelated_role.status(), StatusCode::FORBIDDEN);

    let viewer = preverified_dasobjectstore_router(Router::new(), None)
        .layer(Extension(verified_host_context(&["storage_viewer"])))
        .oneshot(request())
        .await
        .expect("response");
    assert_eq!(viewer.status(), StatusCode::OK);
    let viewer_body = axum::body::to_bytes(viewer.into_body(), 128 * 1024)
        .await
        .expect("viewer body");
    let viewer: serde_json::Value = serde_json::from_slice(&viewer_body).expect("viewer JSON");
    assert_eq!(viewer["groups_file_path"], "pistis-managed");
    assert_eq!(viewer["groups"], serde_json::json!([]));
    assert_eq!(viewer["create_object_store"]["enabled"], false);
    assert!(viewer["warnings"]
        .as_array()
        .expect("warnings")
        .iter()
        .any(|warning| { warning["code"] == "pistis_writer_membership_not_evaluated" }));

    let administrator = preverified_dasobjectstore_router(Router::new(), None)
        .layer(Extension(verified_host_context(&["storage_administrator"])))
        .oneshot(request())
        .await
        .expect("response");
    assert_eq!(administrator.status(), StatusCode::OK);
    let administrator_body = axum::body::to_bytes(administrator.into_body(), 128 * 1024)
        .await
        .expect("administrator body");
    let administrator: serde_json::Value =
        serde_json::from_slice(&administrator_body).expect("administrator JSON");
    assert_ne!(
        administrator["create_object_store"]["state"],
        "admin_required"
    );
}

#[tokio::test]
async fn preverified_observability_dashboard_requires_a_verified_pistis_viewer() {
    let home_request = || {
        Request::builder()
            .uri("/api/v1/dashboard/home?telemetry_window=one_day")
            .body(Body::empty())
            .expect("request")
    };
    let denied_home = preverified_dasobjectstore_router(Router::new(), None)
        .layer(Extension(verified_host_context(&["authenticated"])))
        .oneshot(home_request())
        .await
        .expect("response");
    assert_eq!(denied_home.status(), StatusCode::FORBIDDEN);

    let viewer_home = preverified_dasobjectstore_router(Router::new(), None)
        .layer(Extension(verified_host_context(&["storage_viewer"])))
        .oneshot(home_request())
        .await
        .expect("response");
    assert_eq!(viewer_home.status(), StatusCode::OK);

    let capacity_request = || {
        Request::builder()
            .uri("/api/v1/dashboard/object-stores/store-1/capacity")
            .body(Body::empty())
            .expect("request")
    };
    let denied_capacity = preverified_dasobjectstore_router(Router::new(), None)
        .layer(Extension(verified_host_context(&["authenticated"])))
        .oneshot(capacity_request())
        .await
        .expect("response");
    assert_eq!(denied_capacity.status(), StatusCode::FORBIDDEN);

    // A verified viewer reaches only the bounded daemon bridge. The hermetic
    // test has no daemon socket, so it fails as a gateway error rather than
    // falling back to a local session or appliance-local identity.
    let unavailable_capacity = preverified_dasobjectstore_router(Router::new(), None)
        .layer(Extension(verified_host_context(&["storage_viewer"])))
        .oneshot(capacity_request())
        .await
        .expect("response");
    assert_eq!(unavailable_capacity.status(), StatusCode::BAD_GATEWAY);
}

#[tokio::test]
async fn preverified_ingest_control_requires_a_verified_das_administrator() {
    let request = || {
        Request::builder()
            .method("POST")
            .uri("/api/v1/workspaces/admin/ingest-control")
            .header(FEDERATED_CSRF_HEADER, csrf_binding())
            .header("content-type", "application/json")
            .body(Body::from(
                r#"{"action":"pause","reason":"controlled test","dry_run":true}"#,
            ))
            .expect("request")
    };

    let denied = preverified_dasobjectstore_router(Router::new(), None)
        .layer(Extension(verified_host_context(&["storage_operator"])))
        .oneshot(request())
        .await
        .expect("response");
    assert_eq!(denied.status(), StatusCode::FORBIDDEN);

    // An administrator reaches contract validation before a daemon bridge is
    // used. This proves the operational route is composed without touching a
    // local socket or any local-authentication state.
    let invalid = preverified_dasobjectstore_router(Router::new(), None)
        .layer(Extension(verified_host_context(&["storage_administrator"])))
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/workspaces/admin/ingest-control")
                .header(FEDERATED_CSRF_HEADER, csrf_binding())
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"action":"pause","reason":" ","dry_run":true}"#,
                ))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);

    let legacy_headers = preverified_dasobjectstore_router(Router::new(), None)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/workspaces/admin/ingest-control")
                .header("x-dasobjectstore-username", "operator")
                .header("x-dasobjectstore-session-token", "local-secret")
                .header(FEDERATED_CSRF_HEADER, csrf_binding())
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"action":"pause","reason":"controlled test","dry_run":true}"#,
                ))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(legacy_headers.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn preverified_enclosure_prepare_requires_a_verified_das_administrator() {
    let request = || {
        Request::builder()
            .method("POST")
            .uri("/api/v1/workspaces/enclosures/prepare")
            .header(FEDERATED_CSRF_HEADER, csrf_binding())
            .header("content-type", "application/json")
            .body(Body::from(
                r#"{"ssd_device":"/dev/disk/by-id/nvme-test","hdd_devices":[],"dry_run":true,"allow_format":true,"existing_data_acknowledged":true,"confirmation_marker":"confirm enclosure preparation"}"#,
            ))
            .expect("request")
    };

    let denied = preverified_dasobjectstore_router(Router::new(), None)
        .layer(Extension(verified_host_context(&["storage_operator"])))
        .oneshot(request())
        .await
        .expect("response");
    assert_eq!(denied.status(), StatusCode::FORBIDDEN);

    // A verified administrator reaches only request validation. This proves
    // the host route is protected by the preverified boundary before it can
    // access a daemon, local session store, or local identity mechanism.
    let invalid = preverified_dasobjectstore_router(Router::new(), None)
        .layer(Extension(verified_host_context(&["storage_administrator"])))
        .oneshot(request())
        .await
        .expect("response");
    assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);

    let legacy_headers = preverified_dasobjectstore_router(Router::new(), None)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/workspaces/enclosures/prepare")
                .header("x-dasobjectstore-username", "operator")
                .header("x-dasobjectstore-session-token", "local-secret")
                .header(FEDERATED_CSRF_HEADER, csrf_binding())
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"ssd_device":"/dev/disk/by-id/nvme-test","hdd_devices":[],"dry_run":true,"allow_format":true,"existing_data_acknowledged":true,"confirmation_marker":"confirm enclosure preparation"}"#,
                ))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(legacy_headers.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn monas_exposes_only_pairing_create_and_exchange_without_a_session() {
    let root = temp_root("monas-easyconnect-public");
    let store = registered_store(&root);
    let create = monas_dasobjectstore_api_router(store.clone())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/remote/easyconnect/pairings")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"client_name":"remote CLI","callback_url":"https://customer.example/callback","requested_object_store":"store-1","requested_session_lifetime_seconds":null,"client_request_id":"request-1"}"#,
                ))
                .expect("request builds"),
        )
        .await
        .expect("request completes");
    assert_eq!(create.status(), StatusCode::BAD_REQUEST);

    let approval = monas_dasobjectstore_api_router(store)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/remote/easyconnect/pairings/approve")
                .header("content-type", "application/json")
                .body(Body::from("{}"))
                .expect("request builds"),
        )
        .await
        .expect("request completes");
    // Approval is intentionally not part of the unauthenticated Monas API
    // composition. It is mounted only by a host-authenticated Pistis
    // composition that can supply the approval resolver and S3 descriptor.
    assert_eq!(approval.status(), StatusCode::METHOD_NOT_ALLOWED);
    cleanup(&root);
}

#[tokio::test]
async fn live_monas_session_drives_gui_actor_without_exposing_bearer() {
    let root = temp_root("monas-live");
    let store = registered_store(&root);
    let login = store
        .login_with_session_ttl_seconds("operator", "secret", Some(3_600))
        .expect("login succeeds");
    let now = unix_now();
    let issue = MonasHostSessionIssue {
        username: "operator".to_string(),
        session_token: login.session_token.clone(),
        correlation_id: "corr-monas-1".to_string(),
        csrf_binding_sha256: csrf_binding(),
    };
    let identity = store
        .verify_session_identity("operator", &login.session_token)
        .expect("immutable identity resolves");

    let verified = accept_monas_host_session(&store, &issue, now).expect("session accepted");
    let context = verified.context();
    assert_eq!(
        context.authority,
        HostAuthenticationAuthority::MonasStandalone
    );
    assert_eq!(context.subject_id, identity.principal_id.to_string());
    assert_eq!(context.session_id, identity.session_id.to_string());
    assert_eq!(context.roles, ["authenticated"]);
    assert!(context.expires_at_unix_seconds <= now + 300);
    let serialized = serde_json::to_string(context).expect("context serializes");
    assert!(!serialized.contains(&login.session_token));
    assert!(!serialized.contains("storage_binding"));
    assert_gui_accepts(verified).await;
    assert_monas_router_accepts(&store, &login.session_token, StatusCode::OK).await;
    assert_monas_product_api_accepts(&store, &login.session_token, StatusCode::OK).await;
    assert_monas_mutation_requires_session_bound_csrf(&store, &login.session_token).await;
    assert_monas_product_api_omits_intrinsic_login(&store, &login.session_token).await;
    assert_monas_html_navigation_redirects_to_host_login(&store).await;

    store
        .logout("operator", &login.session_token)
        .expect("logout succeeds");
    let rejection = accept_monas_host_session(&store, &issue, now).expect_err("logout revokes");
    assert!(matches!(
        &rejection,
        HostSessionAdapterError::MonasSession(_)
    ));
    assert!(!rejection.to_string().contains(&login.session_token));
    assert_monas_router_accepts(&store, &login.session_token, StatusCode::UNAUTHORIZED).await;
    assert_monas_product_api_accepts(&store, &login.session_token, StatusCode::UNAUTHORIZED).await;
    cleanup(&root);
}

#[tokio::test]
async fn monas_adapter_rejects_unmigrated_legacy_identity() {
    let root = temp_root("monas-unmigrated");
    let store = registered_store(&root);
    let login = store
        .login_with_session_ttl_seconds("operator", "secret", Some(3_600))
        .expect("login succeeds");
    let mut document: serde_json::Value =
        serde_json::from_slice(&fs::read(store.registry_path()).expect("registry reads"))
            .expect("registry parses");
    let user = document["users"][0].as_object_mut().expect("user object");
    user.remove("principal_id");
    user["sessions"]
        .as_array_mut()
        .expect("sessions array")
        .iter_mut()
        .for_each(|session| {
            session
                .as_object_mut()
                .expect("session object")
                .remove("session_id");
        });
    fs::write(
        store.registry_path(),
        serde_json::to_vec_pretty(&document).expect("registry serializes"),
    )
    .expect("legacy fixture writes");
    let issue = MonasHostSessionIssue {
        username: "operator".to_string(),
        session_token: login.session_token.clone(),
        correlation_id: "corr-monas-unmigrated".to_string(),
        csrf_binding_sha256: csrf_binding(),
    };

    assert!(matches!(
        accept_monas_host_session(&store, &issue, unix_now()),
        Err(HostSessionAdapterError::MonasSession(_))
    ));
    assert_monas_product_api_accepts(&store, &login.session_token, StatusCode::UNAUTHORIZED).await;
    cleanup(&root);
}

#[tokio::test]
async fn live_synoptikon_session_drives_gui_actor_without_storage_grant() {
    let issue = synoptikon_issue();
    let verified =
        accept_synoptikon_host_session(&issue, csrf_binding(), 1_500, &LiveSynoptikon(true))
            .expect("session accepted");
    let context = verified.context();
    assert_eq!(
        context.authority,
        HostAuthenticationAuthority::SynoptikonIntegrated
    );
    assert_eq!(context.subject_id, "user-1");
    let serialized = serde_json::to_value(context).expect("context serializes");
    assert!(serialized.get("storage_binding_id").is_none());
    assert!(serialized.get("storage_authority").is_none());
    assert_gui_accepts(verified).await;

    let now = unix_now();
    let router_issue = synoptikon_issue_at(now - 1, now + 300);
    let app = synoptikon_federated_router(protected_router(), Arc::new(LiveSynoptikon(true)))
        .layer(Extension(SynoptikonHostRequestAuthentication {
            issue: router_issue.clone(),
            csrf_binding_sha256: csrf_binding(),
        }));
    assert_eq!(request(app, None).await, StatusCode::OK);
    assert_synoptikon_mutation_requires_bound_csrf(router_issue).await;
    let missing_context =
        synoptikon_federated_router(protected_router(), Arc::new(LiveSynoptikon(true)));
    assert_eq!(
        request(missing_context, None).await,
        StatusCode::UNAUTHORIZED
    );

    assert!(matches!(
        accept_synoptikon_host_session(&issue, csrf_binding(), 1_500, &LiveSynoptikon(false)),
        Err(HostSessionAdapterError::HostContext(_))
    ));
}

async fn assert_synoptikon_mutation_requires_bound_csrf(issue: SynoptikonIntegratedSessionIssue) {
    async fn mutate(_actor: AuthenticatedGuiActor) -> StatusCode {
        StatusCode::NO_CONTENT
    }
    async fn status(issue: SynoptikonIntegratedSessionIssue, csrf: Option<String>) -> StatusCode {
        let app = synoptikon_federated_router(
            Router::new().route("/mutate", post(mutate)),
            Arc::new(LiveSynoptikon(true)),
        )
        .layer(Extension(SynoptikonHostRequestAuthentication {
            issue,
            csrf_binding_sha256: csrf_binding(),
        }));
        let mut request = Request::builder().method("POST").uri("/mutate");
        if let Some(csrf) = csrf {
            request = request.header(FEDERATED_CSRF_HEADER, csrf);
        }
        app.oneshot(request.body(Body::empty()).expect("request builds"))
            .await
            .expect("request completes")
            .status()
    }
    assert_eq!(status(issue.clone(), None).await, StatusCode::FORBIDDEN);
    assert_eq!(
        status(issue.clone(), Some("sha256:wrong".to_string())).await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        status(issue, Some(csrf_binding())).await,
        StatusCode::NO_CONTENT
    );
}

#[test]
fn synoptikon_adapter_rejects_invalid_boundary_and_overlong_context() {
    let mut invalid = synoptikon_issue();
    invalid.context.central_audit_enabled = false;
    assert!(matches!(
        accept_synoptikon_host_session(&invalid, csrf_binding(), 1_500, &LiveSynoptikon(true)),
        Err(HostSessionAdapterError::SynoptikonSession(_))
    ));

    let mut overlong = synoptikon_issue();
    overlong.expires_at_unix_seconds = overlong.issued_at_unix_seconds + 8 * 60 * 60 + 1;
    assert!(matches!(
        accept_synoptikon_host_session(&overlong, csrf_binding(), 1_500, &LiveSynoptikon(true)),
        Err(HostSessionAdapterError::HostContext(_))
    ));
}

async fn assert_gui_accepts(verified: dasobjectstore_gui_api::VerifiedHostAuthenticatedContext) {
    let app = protected_router().layer(Extension(verified));
    assert_eq!(request(app, None).await, StatusCode::OK);
}

async fn assert_monas_router_accepts(
    store: &ProsopikonAuthStore,
    session_token: &str,
    expected: StatusCode,
) {
    let app = monas_federated_router(protected_router(), store.clone());
    let cookie = HeaderValue::from_str(&format!("monas_session=operator:{session_token}"))
        .expect("cookie header");
    assert_eq!(request(app, Some(cookie)).await, expected);
}

async fn assert_monas_product_api_accepts(
    store: &ProsopikonAuthStore,
    session_token: &str,
    expected: StatusCode,
) {
    let app = monas_dasobjectstore_api_router(store.clone());
    let cookie = HeaderValue::from_str(&format!("monas_session=operator:{session_token}"))
        .expect("cookie header");
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/host-session")
                .header(COOKIE, cookie)
                .body(Body::empty())
                .expect("request builds"),
        )
        .await
        .expect("request completes");
    assert_eq!(response.status(), expected);
    if expected == StatusCode::OK {
        let response = monas_dasobjectstore_api_router(store.clone())
            .oneshot(
                Request::builder()
                    .uri("/api/v1/host-session")
                    .header(COOKIE, format!("monas_session=operator:{session_token}"))
                    .body(Body::empty())
                    .expect("request builds"),
            )
            .await
            .expect("request completes");
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 16 * 1024)
            .await
            .expect("session response body");
        let session: serde_json::Value = serde_json::from_slice(&body).expect("session JSON");
        let identity = store
            .verify_session_identity("operator", session_token)
            .expect("immutable identity resolves");
        assert_eq!(session["subject_id"], identity.principal_id.to_string());
        assert_eq!(session["authority"], "monas_standalone");
        assert_eq!(session["csrf_token"], monas_csrf_binding(session_token));
        assert!(session.get("session_token").is_none());
    }
}

async fn assert_monas_product_api_omits_intrinsic_login(
    store: &ProsopikonAuthStore,
    session_token: &str,
) {
    let app = monas_dasobjectstore_api_router(store.clone());
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/login")
                .header(COOKIE, format!("monas_session=operator:{session_token}"))
                .header(FEDERATED_CSRF_HEADER, monas_csrf_binding(session_token))
                .body(Body::empty())
                .expect("request builds"),
        )
        .await
        .expect("request completes");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

async fn assert_monas_mutation_requires_session_bound_csrf(
    store: &ProsopikonAuthStore,
    session_token: &str,
) {
    async fn mutate(_actor: AuthenticatedGuiActor) -> StatusCode {
        StatusCode::NO_CONTENT
    }
    let cookie = format!("monas_session=operator:{session_token}");
    async fn status(
        store: ProsopikonAuthStore,
        cookie: String,
        csrf: Option<String>,
    ) -> StatusCode {
        let mut request = Request::builder()
            .method("POST")
            .uri("/mutate")
            .header(COOKIE, cookie);
        if let Some(csrf) = csrf {
            request = request.header(FEDERATED_CSRF_HEADER, csrf);
        }
        monas_federated_router(Router::new().route("/mutate", post(mutate)), store)
            .oneshot(request.body(Body::empty()).expect("request builds"))
            .await
            .expect("request completes")
            .status()
    }
    assert_eq!(
        status(store.clone(), cookie.clone(), None).await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        status(
            store.clone(),
            cookie.clone(),
            Some("sha256:wrong".to_string())
        )
        .await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        status(
            store.clone(),
            cookie,
            Some(monas_csrf_binding(session_token))
        )
        .await,
        StatusCode::NO_CONTENT
    );
}

async fn assert_monas_html_navigation_redirects_to_host_login(store: &ProsopikonAuthStore) {
    let host_routes = Router::new().route("/", get(|| async { "web app" }));
    let app = dasobjectstore_mnemosyne::monas_dasobjectstore_router(host_routes, store.clone());
    let response = app
        .oneshot(
            Request::builder()
                .uri("/")
                .header("accept", "text/html,application/xhtml+xml")
                .body(Body::empty())
                .expect("request builds"),
        )
        .await
        .expect("request completes");
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        response
            .headers()
            .get("location")
            .and_then(|value| value.to_str().ok()),
        Some("/login?return_to=/")
    );
}

async fn request(app: Router, cookie: Option<HeaderValue>) -> StatusCode {
    let mut builder = Request::builder().uri("/protected");
    if let Some(cookie) = cookie {
        builder = builder.header(COOKIE, cookie);
    }
    let response = app
        .oneshot(builder.body(Body::empty()).expect("request builds"))
        .await
        .expect("request completes");
    response.status()
}

fn protected_router() -> Router {
    async fn protected(_actor: AuthenticatedGuiActor) -> StatusCode {
        StatusCode::OK
    }
    Router::new().route("/protected", get(protected))
}

struct LiveSynoptikon(bool);

impl SynoptikonLiveSessionVerifier for LiveSynoptikon {
    fn verify_live_session(
        &self,
        _session: &SynoptikonIntegratedAcceptedSession,
    ) -> Result<(), String> {
        self.0.then_some(()).ok_or_else(|| "revoked".to_string())
    }
}

struct LiveHostContext;

impl HostAuthenticationContextVerifier for LiveHostContext {
    fn verify_live_session(&self, _context: &HostAuthenticatedContext) -> Result<(), String> {
        Ok(())
    }
}

fn verified_host_context(
    roles: &[&str],
) -> dasobjectstore_gui_api::VerifiedHostAuthenticatedContext {
    accept_host_authenticated_context(
        HostAuthenticatedContext {
            schema_version: HOST_AUTH_CONTEXT_SCHEMA_VERSION.to_owned(),
            authority: HostAuthenticationAuthority::MonasStandalone,
            issuer: "monas".to_owned(),
            audience: HOST_AUTH_AUDIENCE.to_owned(),
            subject_id: "pistis-subject-1".to_owned(),
            session_id: "pistis-session-1".to_owned(),
            roles: roles.iter().map(|role| (*role).to_owned()).collect(),
            issued_at_unix_seconds: 1_000,
            expires_at_unix_seconds: 2_000,
            correlation_id: "corr-host-ingest-control-1".to_owned(),
            csrf_binding_sha256: csrf_binding(),
        },
        1_500,
        &LiveHostContext,
    )
    .expect("verified host context")
}

fn synoptikon_issue() -> SynoptikonIntegratedSessionIssue {
    synoptikon_issue_at(1_000, 2_000)
}

fn synoptikon_issue_at(
    issued_at_unix_seconds: i64,
    expires_at_unix_seconds: i64,
) -> SynoptikonIntegratedSessionIssue {
    SynoptikonIntegratedSessionIssue {
        request_id: "request-1".to_string(),
        issued_at_unix_seconds,
        expires_at_unix_seconds,
        context: SynoptikonIntegratedHostBoundaryContext {
            request_context_schema_version: REQUEST_CONTEXT_SCHEMA_VERSION.to_string(),
            product_id: DASOBJECTSTORE_PRODUCT_ID.to_string(),
            tenant_id: "tenant-1".to_string(),
            account_id: "account-1".to_string(),
            user_id: "user-1".to_string(),
            project_id: "project-1".to_string(),
            entitlement_id: "entitlement-1".to_string(),
            roles: vec!["storage_operator".to_string()],
            correlation_id: "corr-synoptikon-1".to_string(),
            central_audit_enabled: true,
            storage_authority: StorageAuthority::SynoptikonStorageBinding,
            storage_binding_id: "binding-1".to_string(),
        },
    }
}

fn registered_store(root: &Path) -> ProsopikonAuthStore {
    let store = ProsopikonAuthStore::new(root);
    store.create_user("operator").expect("user created");
    let registration = store
        .issue_registration_token("operator", 1)
        .expect("registration issued");
    store
        .register_with_token("operator", &registration, "secret")
        .expect("registration succeeds");
    store
}

fn csrf_binding() -> String {
    format!("sha256:{}", "a".repeat(64))
}

fn test_s3_endpoint() -> EasyconnectS3EndpointConfig {
    EasyconnectS3EndpointConfig {
        descriptor: StandaloneS3ConnectionDescriptor {
            endpoint_url: "https://objects.example.test:3900".to_string(),
            region: "test-region".to_string(),
            addressing_style: "path".to_string(),
        },
        tls_certificate_path: PathBuf::from("/test/appliance-fullchain.pem"),
    }
}

fn monas_csrf_binding(session_token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"dasobjectstore:monas:csrf-binding:v1\0");
    hasher.update(session_token.as_bytes());
    format!("sha256:{:x}", hasher.finalize())
}

fn temp_root(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "dasobjectstore-host-adapter-{label}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos()
    ))
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after epoch")
        .as_secs() as i64
}

fn cleanup(root: &Path) {
    let _ = fs::remove_dir_all(root);
}
