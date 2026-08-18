use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use dasobjectstore_gui_api::s3_gateway_router;
use tower::ServiceExt;

#[tokio::test]
async fn fixed_projection_routes_fail_closed_without_the_host_provisioned_credential() {
    let router = s3_gateway_router(1);
    for (method, path) in [
        (Method::POST, "/v1/synoptikon-projection/intent"),
        (Method::PUT, "/v1/synoptikon-projection/bytes"),
        (Method::POST, "/v1/synoptikon-projection/readback"),
    ] {
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(path)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
}

#[tokio::test]
async fn projection_route_does_not_accept_path_selectors_or_wrong_methods() {
    let router = s3_gateway_router(1);
    for (method, path, expected) in [
        (
            Method::POST,
            "/v1/synoptikon-projection/intent/arbitrary",
            StatusCode::NOT_FOUND,
        ),
        (
            Method::GET,
            "/v1/synoptikon-projection/bytes",
            StatusCode::METHOD_NOT_ALLOWED,
        ),
        (
            Method::PUT,
            "/v1/synoptikon-projection/readback",
            StatusCode::METHOD_NOT_ALLOWED,
        ),
        (
            Method::POST,
            "/v1/synoptikon-projection/intent?bucket=other",
            StatusCode::BAD_REQUEST,
        ),
        (
            Method::PUT,
            "/v1/synoptikon-projection/bytes?path=other",
            StatusCode::BAD_REQUEST,
        ),
        (
            Method::POST,
            "/v1/synoptikon-projection/readback?redirect=https://example.invalid",
            StatusCode::BAD_REQUEST,
        ),
        (
            Method::POST,
            "/v1/synoptikon-projection/intent/",
            StatusCode::NOT_FOUND,
        ),
        (
            Method::PUT,
            "/v1/synoptikon-projection/bytes/",
            StatusCode::NOT_FOUND,
        ),
        (
            Method::POST,
            "/v1/synoptikon-projection/readback/",
            StatusCode::NOT_FOUND,
        ),
    ] {
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(path)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), expected);
        assert!(response.headers().get("location").is_none());
    }
}
