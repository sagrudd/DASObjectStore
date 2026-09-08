//! Advertise only the poll route belonging to the actual supported API mount.
use axum::http::Uri;

const CREATE: &str = "/api/v1/remote/easyconnect/pairings";
const PRODUCT: &str = "/products/dasobjectstore";

pub(super) struct PollingMount(&'static str);

impl PollingMount {
    /// OriginalUri is supplied by Axum routing, never by forwarding headers.
    pub(super) fn from_create(uri: &Uri) -> Result<Self, &'static str> {
        if uri.scheme().is_some() || uri.authority().is_some() || uri.query().is_some() {
            return Err("unsupported EasyConnect create route");
        }
        match uri.path() {
            CREATE => Ok(Self("")),
            "/products/dasobjectstore/api/v1/remote/easyconnect/pairings" => Ok(Self(PRODUCT)),
            _ => Err("unsupported EasyConnect create route"),
        }
    }

    pub(super) fn polling_url(
        &self,
        public_origin: &str,
        pairing_id: &str,
        daemon_path: &str,
    ) -> Result<String, &'static str> {
        // The ID remains the daemon's opaque capability, but must be exactly
        // one unescaped URL path segment. Never normalize a substituted path.
        if pairing_id.is_empty()
            || matches!(pairing_id, "." | "..")
            || !pairing_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-._~".contains(&b))
            || daemon_path != format!("{CREATE}/{pairing_id}")
        {
            return Err("daemon returned an invalid EasyConnect polling route");
        }
        let origin =
            reqwest::Url::parse(public_origin).map_err(|_| "invalid EasyConnect public origin")?;
        if origin.scheme() != "https"
            || origin.host_str().is_none()
            || !origin.username().is_empty()
            || origin.password().is_some()
            || origin.query().is_some()
            || origin.fragment().is_some()
            || origin.path() != "/"
        {
            return Err("invalid EasyConnect public origin");
        }
        Ok(format!(
            "{}{prefix}{daemon_path}",
            public_origin.trim_end_matches('/'),
            prefix = self.0
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::{to_bytes, Body},
        extract::OriginalUri,
        http::{Request, StatusCode},
        routing::{get, post},
        Router,
    };
    use tower::ServiceExt;

    #[tokio::test]
    async fn advertised_polling_route_resolves_under_both_actual_axum_mounts() {
        async fn create(OriginalUri(uri): OriginalUri) -> String {
            PollingMount::from_create(&uri)
                .unwrap()
                .polling_url(
                    "https://site.example:8443",
                    "pairing-test",
                    &format!("{CREATE}/pairing-test"),
                )
                .unwrap()
        }
        for prefix in ["", PRODUCT] {
            let routes = Router::new().route(CREATE, post(create)).route(
                &format!("{CREATE}/{{pairing_id}}"),
                get(|| async { StatusCode::NO_CONTENT }),
            );
            let app = if prefix.is_empty() {
                routes
            } else {
                Router::new().nest(prefix, routes)
            };
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri(format!("{prefix}{CREATE}"))
                        .header("host", "untrusted.example")
                        .header("x-forwarded-prefix", "/wrong")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            let raw = to_bytes(response.into_body(), 4096).await.unwrap();
            let advertised = std::str::from_utf8(&raw).unwrap();
            assert_eq!(
                advertised,
                format!("https://site.example:8443{prefix}{CREATE}/pairing-test")
            );
            let path = reqwest::Url::parse(advertised).unwrap().path().to_owned();
            let polled = app
                .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(polled.status(), StatusCode::NO_CONTENT);
        }
    }

    #[test]
    fn aliases_unknown_mounts_and_daemon_binding_substitutions_deny() {
        for path in [
            "/other/api/v1/remote/easyconnect/pairings",
            "/api/v1/remote/easyconnect/pairings/",
            "/api/v1/remote/easyconnect/pairings?mount=other",
            "/products//dasobjectstore/api/v1/remote/easyconnect/pairings",
            "https://attacker.example/api/v1/remote/easyconnect/pairings",
        ] {
            assert!(PollingMount::from_create(&path.parse().unwrap()).is_err());
        }
        let mount = PollingMount::from_create(&CREATE.parse().unwrap()).unwrap();
        let correct = format!("{CREATE}/pairing-test");
        for path in [
            format!("{CREATE}/other"),
            format!("{correct}?x=1"),
            format!("{correct}#fragment"),
            format!("https://attacker.example{correct}"),
            format!("{PRODUCT}{correct}"),
            format!("{correct}/"),
        ] {
            assert!(mount
                .polling_url("https://site.example", "pairing-test", &path)
                .is_err());
        }
        for id in ["", ".", "..", "a/b", "a?b", "%61", "a#b", "white space"] {
            assert!(mount
                .polling_url("https://site.example", id, &format!("{CREATE}/{id}"))
                .is_err());
        }
        for origin in [
            "http://site.example",
            "https://user@site.example",
            "https://site.example/path",
            "https://site.example?query=1",
            "https://site.example#fragment",
        ] {
            assert!(mount.polling_url(origin, "pairing-test", &correct).is_err());
        }
    }
}
