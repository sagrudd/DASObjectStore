use crate::server_cli::ServerCli;
use axum::extract::Path as AxumPath;
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::get;
use axum::Router;
use axum_server::tls_rustls::RustlsConfig;
use dasobjectstore_gui_api::{
    ensure_standalone_tls_assets, gui_api_router_for_host_mode, LocalAuthStore,
    LocalAuthStoreError, StandaloneAuthenticationConfig, StandaloneServerConfig,
    StandaloneServerConfigError, StandaloneTlsAssetError, StandaloneTlsAssetReport,
};
use std::fmt::{self, Display};
use std::io::{self, Write};
use std::path::{Component, PathBuf};
use std::sync::{Arc, OnceLock};
use tokio::sync::Semaphore;

const STATIC_ASSET_READ_PERMITS: usize = 4;

fn static_asset_read_permits() -> &'static Arc<Semaphore> {
    static PERMITS: OnceLock<Arc<Semaphore>> = OnceLock::new();
    PERMITS.get_or_init(|| Arc::new(Semaphore::new(STATIC_ASSET_READ_PERMITS)))
}

pub(crate) async fn run(cli: &ServerCli, writer: &mut impl Write) -> Result<(), ServerRunError> {
    let config = cli.server_config()?;
    config.validate()?;
    let tls_report = if cli.generate_missing_tls() {
        Some(ensure_standalone_tls_assets(&config)?)
    } else {
        None
    };

    if cli.check_config() {
        if cli.json() {
            write_json_config(&config, tls_report.as_ref(), writer)?;
            writer.write_all(b"\n")?;
        } else {
            write_pretty_config(&config, tls_report.as_ref(), writer)?;
        }
        return Ok(());
    }

    start_server(config, writer).await
}

async fn start_server(
    config: StandaloneServerConfig,
    writer: &mut impl Write,
) -> Result<(), ServerRunError> {
    let socket_addr = config.socket_addr()?;
    ensure_standalone_tls_assets(&config)?;
    let auth_store = LocalAuthStore::default_standalone();
    let revoked_sessions = auth_store.revoke_all_sessions()?;
    let tls =
        RustlsConfig::from_pem_file(&config.tls.certificate_path, &config.tls.private_key_path)
            .await?;
    writeln!(
        writer,
        "dasobjectstore-server listening on https://{}",
        socket_addr
    )?;
    if revoked_sessions > 0 {
        writeln!(
            writer,
            "dasobjectstore-server invalidated {revoked_sessions} existing session(s)"
        )?;
    }
    let web_root = config.product_root.join("web");
    let auth_root = auth_store.root().to_path_buf();
    axum_server::bind_rustls(socket_addr, tls)
        .serve(standalone_router(web_root, config.authentication, auth_root).into_make_service())
        .await?;
    Ok(())
}

fn standalone_router(
    web_root: PathBuf,
    authentication: StandaloneAuthenticationConfig,
    auth_root: PathBuf,
) -> Router {
    let index_root = web_root.clone();
    let index_root_with_slash = web_root.clone();
    let asset_root = web_root;
    let host_mode = authentication.gui_api_host_mode();
    let auth_store = LocalAuthStore::new(auth_root);
    let root_api = gui_api_router_for_host_mode(host_mode, auth_store.clone());
    let product_api = gui_api_router_for_host_mode(host_mode, auth_store);
    Router::new()
        .route("/", get(root_redirect))
        .route(
            "/products/dasobjectstore",
            get(move || serve_asset(index_root.join("index.html"), "text/html; charset=utf-8")),
        )
        .route(
            "/products/dasobjectstore/",
            get(move || {
                serve_asset(
                    index_root_with_slash.join("index.html"),
                    "text/html; charset=utf-8",
                )
            }),
        )
        .route(
            "/products/dasobjectstore/{*asset}",
            get(move |AxumPath(asset): AxumPath<String>| {
                serve_named_asset(asset_root.clone(), asset)
            }),
        )
        .merge(root_api)
        .nest("/products/dasobjectstore", product_api)
}

async fn root_redirect() -> Redirect {
    Redirect::temporary("/products/dasobjectstore/")
}

async fn serve_asset(path: PathBuf, content_type: &'static str) -> Response {
    let _permit = match static_asset_read_permits().clone().try_acquire_owned() {
        Ok(permit) => permit,
        Err(_) => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
    };
    let cache_control = static_asset_cache_control(&path);
    let bytes = match tokio::fs::read(&path).await {
        Ok(bytes) => bytes,
        Err(_) => return StatusCode::NOT_FOUND.into_response(),
    };
    bytes_response(content_type, bytes, cache_control)
}

async fn serve_named_asset(web_root: PathBuf, asset: String) -> Response {
    let Some(path) = static_asset_path(web_root, &asset) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let content_type = static_asset_content_type(&path);
    serve_asset(path, content_type).await
}

fn static_asset_path(web_root: PathBuf, asset: &str) -> Option<PathBuf> {
    let relative = PathBuf::from(asset.trim_start_matches('/'));
    if relative.as_os_str().is_empty() {
        return Some(web_root.join("index.html"));
    }
    let mut resolved = web_root;
    for component in relative.components() {
        match component {
            Component::Normal(part) => resolved.push(part),
            Component::CurDir => {}
            _ => return None,
        }
    }
    Some(resolved)
}

fn static_asset_content_type(path: &std::path::Path) -> &'static str {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("css") => "text/css; charset=utf-8",
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "application/javascript",
        Some("json") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("wasm") => "application/wasm",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("ico") => "image/x-icon",
        _ => "application/octet-stream",
    }
}

fn static_asset_cache_control(path: &std::path::Path) -> &'static str {
    let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
        return "no-cache";
    };
    if file_name == "index.html" {
        return "no-cache";
    }
    let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
        return "no-cache";
    };
    let Some((_, fingerprint)) = stem.rsplit_once('-') else {
        return "no-cache";
    };
    if fingerprint.len() >= 6 && fingerprint.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    }
}

fn bytes_response(
    content_type: &'static str,
    bytes: Vec<u8>,
    cache_control: &'static str,
) -> Response {
    match HeaderValue::from_str(content_type) {
        Ok(content_type) => (
            [
                (header::CONTENT_TYPE, content_type),
                (
                    header::CACHE_CONTROL,
                    HeaderValue::from_static(cache_control),
                ),
            ],
            bytes,
        )
            .into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

#[derive(Debug)]
pub(crate) enum ServerRunError {
    Config(StandaloneServerConfigError),
    Tls(StandaloneTlsAssetError),
    Auth(LocalAuthStoreError),
    Io(io::Error),
    Json(serde_json::Error),
}

impl Display for ServerRunError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Config(err) => write!(formatter, "{err}"),
            Self::Tls(err) => write!(formatter, "{err}"),
            Self::Auth(err) => write!(formatter, "{err}"),
            Self::Io(err) => write!(formatter, "server output failed: {err}"),
            Self::Json(err) => write!(formatter, "server JSON output failed: {err}"),
        }
    }
}

impl std::error::Error for ServerRunError {}

impl From<StandaloneServerConfigError> for ServerRunError {
    fn from(err: StandaloneServerConfigError) -> Self {
        Self::Config(err)
    }
}

impl From<StandaloneTlsAssetError> for ServerRunError {
    fn from(err: StandaloneTlsAssetError) -> Self {
        Self::Tls(err)
    }
}

impl From<LocalAuthStoreError> for ServerRunError {
    fn from(err: LocalAuthStoreError) -> Self {
        Self::Auth(err)
    }
}

impl From<io::Error> for ServerRunError {
    fn from(err: io::Error) -> Self {
        Self::Io(err)
    }
}

impl From<serde_json::Error> for ServerRunError {
    fn from(err: serde_json::Error) -> Self {
        Self::Json(err)
    }
}

fn write_pretty_config(
    config: &StandaloneServerConfig,
    tls_report: Option<&StandaloneTlsAssetReport>,
    writer: &mut impl Write,
) -> Result<(), ServerRunError> {
    writeln!(writer, "DASObjectStore standalone server configuration OK")?;
    writeln!(writer, "bind: {}", config.socket_addr()?)?;
    writeln!(writer, "public_base_url: {}", config.public_base_url)?;
    writeln!(writer, "product_root: {}", config.product_root.display())?;
    writeln!(
        writer,
        "tls_certificate_path: {}",
        config.tls.certificate_path.display()
    )?;
    writeln!(
        writer,
        "tls_private_key_path: {}",
        config.tls.private_key_path.display()
    )?;
    if let Some(tls_report) = tls_report {
        writeln!(writer, "tls_generated: {}", tls_report.generated)?;
    }
    Ok(())
}

fn write_json_config(
    config: &StandaloneServerConfig,
    tls_report: Option<&StandaloneTlsAssetReport>,
    writer: &mut impl Write,
) -> Result<(), ServerRunError> {
    serde_json::to_writer_pretty(
        &mut *writer,
        &serde_json::json!({
            "server": config,
            "auth_host_mode": config.gui_api_host_mode(),
            "tls_assets": tls_report,
        }),
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{run, standalone_router};
    use crate::server_cli::ServerCli;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use clap::Parser;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};
    use tower::ServiceExt;

    #[tokio::test]
    async fn emits_pretty_check_config() {
        let cli = ServerCli::try_parse_from(["dasobjectstore-server", "--check-config"])
            .expect("server CLI parses");
        let mut output = Vec::new();

        run(&cli, &mut output).await.expect("check config runs");

        let output = String::from_utf8(output).expect("utf8 output");
        assert!(output.contains("DASObjectStore standalone server configuration OK"));
        assert!(output.contains("bind: 127.0.0.1:8448"));
    }

    #[tokio::test]
    async fn emits_json_check_config() {
        let cli = ServerCli::try_parse_from(["dasobjectstore-server", "--check-config", "--json"])
            .expect("server CLI parses");
        let mut output = Vec::new();

        run(&cli, &mut output).await.expect("check config runs");

        let output: serde_json::Value =
            serde_json::from_slice(&output).expect("server config JSON parses");
        assert_eq!(output["server"]["bind_address"], "127.0.0.1");
        assert_eq!(output["server"]["https_port"], 8448);
        assert_eq!(output["tls_assets"], serde_json::Value::Null);
    }

    #[tokio::test]
    async fn generates_missing_tls_assets_when_requested() {
        let root = temp_root("server-run-generate");
        let cli = ServerCli::try_parse_from([
            "dasobjectstore-server",
            "--check-config",
            "--generate-missing-tls",
            "--product-root",
            root.to_str().expect("root path"),
        ])
        .expect("server CLI parses");
        let mut output = Vec::new();

        run(&cli, &mut output).await.expect("check config runs");

        let output = String::from_utf8(output).expect("utf8 output");
        assert!(output.contains("tls_generated: true"));
        assert!(root.join("tls/server.crt").exists());
        assert!(root.join("tls/server.key").exists());

        cleanup(&root);
    }

    #[tokio::test]
    async fn standalone_router_serves_product_mount_and_api() {
        let root = temp_root("server-run-web");
        write_web_asset(
            &root,
            "index.html",
            "<!doctype html><title>DASObjectStore</title>",
        );
        write_web_asset(&root, "dasobjectstore-gui-web-abcdef.js", "export {};");
        write_web_asset(&root, "dasobjectstore-gui-web-abcdef_bg.wasm", "wasm");
        write_web_asset(&root, "styles-abcdef.css", "body{}");

        let auth_root = temp_root("server-run-auth");
        let response = standalone_router(root.clone(), Default::default(), auth_root.clone())
            .oneshot(
                Request::builder()
                    .uri("/products/dasobjectstore/")
                    .body(Body::empty())
                    .expect("request builds"),
            )
            .await
            .expect("index response");

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers().get("cache-control").unwrap(), "no-cache");

        let response = standalone_router(root.clone(), Default::default(), auth_root.clone())
            .oneshot(
                Request::builder()
                    .uri("/products/dasobjectstore/dasobjectstore-gui-web-abcdef.js")
                    .body(Body::empty())
                    .expect("request builds"),
            )
            .await
            .expect("asset response");

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get("cache-control").unwrap(),
            "public, max-age=31536000, immutable"
        );

        let response = standalone_router(root.clone(), Default::default(), auth_root.clone())
            .oneshot(
                Request::builder()
                    .uri("/products/dasobjectstore/api/v1/health")
                    .body(Body::empty())
                    .expect("request builds"),
            )
            .await
            .expect("api response");

        assert_eq!(response.status(), StatusCode::OK);
        cleanup(&auth_root);
        cleanup(&root);
    }

    #[tokio::test]
    async fn standalone_router_mounts_local_auth_when_configured() {
        let root = temp_root("server-run-auth-web");
        let auth_root = temp_root("server-run-auth-state");
        write_web_asset(&root, "index.html", "<!doctype html>");

        let response = standalone_router(root.clone(), Default::default(), auth_root.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/products/dasobjectstore/api/session")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"username":"admin","session_token":"invalid"}"#,
                    ))
                    .expect("request builds"),
            )
            .await
            .expect("auth response");

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        cleanup(&auth_root);
        cleanup(&root);
    }

    #[tokio::test]
    async fn standalone_router_rejects_asset_traversal() {
        let root = temp_root("server-run-web-traversal");
        let auth_root = temp_root("server-run-web-traversal-auth");
        write_web_asset(&root, "index.html", "<!doctype html>");

        let response = standalone_router(root.clone(), Default::default(), auth_root.clone())
            .oneshot(
                Request::builder()
                    .uri("/products/dasobjectstore/../secret")
                    .body(Body::empty())
                    .expect("request builds"),
            )
            .await
            .expect("asset response");

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        cleanup(&auth_root);
        cleanup(&root);
    }

    #[test]
    fn static_asset_cache_policy_requires_a_hex_fingerprint() {
        assert_eq!(
            super::static_asset_cache_control(Path::new("styles-abcdef.css")),
            "public, max-age=31536000, immutable"
        );
        assert_eq!(
            super::static_asset_cache_control(Path::new("styles.css")),
            "no-cache"
        );
        assert_eq!(
            super::static_asset_cache_control(Path::new("index.html")),
            "no-cache"
        );
        assert_eq!(
            super::static_asset_cache_control(Path::new("styles-not-a-hash.css")),
            "no-cache"
        );
    }

    fn temp_root(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "dasobjectstore-server-run-{label}-{}-{unique}",
            std::process::id()
        ))
    }

    fn cleanup(root: &Path) {
        let _ = fs::remove_dir_all(root);
    }

    fn write_web_asset(root: &Path, name: &str, contents: impl AsRef<[u8]>) {
        fs::create_dir_all(root).expect("web root created");
        fs::write(root.join(name), contents).expect("web asset written");
    }
}
