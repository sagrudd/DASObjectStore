use super::*;

pub(crate) async fn easyconnect_pairing_status(
    State(state): State<EasyconnectPublicRouteState>,
    Path(pairing_id): Path<String>,
) -> Result<Json<RemoteEasyconnectPairingStatusResponse>, (StatusCode, Json<AuthRouteError>)> {
    let request = RemoteEasyconnectPairingStatusRequest { pairing_id };
    request.validate().map_err(|error| {
        route_error(
            StatusCode::BAD_REQUEST,
            "invalid_easyconnect_pairing_status",
            error.to_string(),
        )
    })?;
    crate::daemon_bridge::DaemonBridge::shared_packaged()
        .call_message(move || {
            DaemonClient::new(UnixSocketDaemonTransport::for_bounded_bridge(
                state.daemon_endpoint.socket_path(),
            ))
            .remote_easyconnect_pairing_status(request)
            .map_err(|error| error.to_string())
        })
        .await
        .map(Json)
        .map_err(remote_auth_bridge_error)
}

pub(crate) async fn easyconnect_browser_approval(
    actor: AuthenticatedGuiActor,
    Extension(verified): Extension<crate::VerifiedHostAuthenticatedContext>,
    Query(query): Query<EasyconnectBrowserApprovalQuery>,
) -> Result<Response, (StatusCode, Json<AuthRouteError>)> {
    if query.pairing_id.trim().is_empty()
        || query.object_store.trim().is_empty()
        || query.expires_at_utc.trim().is_empty()
        || query.pairing_id.len() > 128
        || query.object_store.len() > 256
        || query.expires_at_utc.len() > 64
    {
        return Err(route_error(
            StatusCode::BAD_REQUEST,
            "invalid_easyconnect_approval_page",
            "pairing, ObjectStore, and expiry are required",
        ));
    }
    let intent = serde_json::to_string(&EasyconnectBrowserApprovalIntent {
        pairing_id: query.pairing_id.clone(),
        object_store: query.object_store.clone(),
    })
    .map_err(|error| {
        route_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "easyconnect_approval_render_failed",
            error.to_string(),
        )
    })?;
    let csrf = serde_json::to_string(&verified.context().csrf_binding_sha256).map_err(|error| {
        route_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "easyconnect_approval_render_failed",
            error.to_string(),
        )
    })?;
    let principal = html_escape(&actor.subject_id);
    let object_store = html_escape(&query.object_store);
    let nonce = &verified.context().csrf_binding_sha256;
    let body = Html(format!(
        r#"<!doctype html><html lang="en"><meta charset="utf-8"><meta name="viewport" content="width=device-width"><title>Approve DASObjectStore connection</title><body><main><h1>Approve remote DASObjectStore connection</h1><dl><dt>Authority principal</dt><dd>{principal}</dd><dt>ObjectStore</dt><dd>{object_store}</dd></dl><p>This creates one short-lived, ObjectStore-scoped session. No GitHub credential is sent to DASObjectStore.</p><button id="approve" type="button">Approve connection</button><pre id="status" role="status"></pre></main><script nonce="{nonce}">const intent={intent};const csrf={csrf};document.getElementById("approve").onclick=async()=>{{const status=document.getElementById("status");status.textContent="Approving…";const response=await fetch("/products/dasobjectstore/api/v1/remote/easyconnect/pairings/approve",{{method:"POST",credentials:"same-origin",headers:{{"content-type":"application/json","x-dasobjectstore-csrf":csrf}},body:JSON.stringify(intent)}});const result=await response.json();if(!response.ok){{status.textContent="Approval failed.";return;}}const form=document.createElement("form");form.method="POST";form.action=result.callback_url;for(const [name,value] of Object.entries({{pairing_id:result.pairing_id,exchange_code:result.exchange_code}})){{const input=document.createElement("input");input.type="hidden";input.name=name;input.value=value;form.appendChild(input);}}document.body.appendChild(form);form.submit();}};</script></body></html>"#
    ));
    let mut response = body.into_response();
    let headers = response.headers_mut();
    headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert("referrer-policy", HeaderValue::from_static("no-referrer"));
    headers.insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    let policy = format!(
        "default-src 'none'; script-src 'nonce-{nonce}'; connect-src 'self'; form-action http://127.0.0.1:*; base-uri 'none'; frame-ancestors 'none'"
    );
    headers.insert(
        "content-security-policy",
        HeaderValue::from_str(&policy).map_err(|_| {
            route_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "easyconnect_approval_render_failed",
                "invalid content security policy",
            )
        })?,
    );
    Ok(response)
}
