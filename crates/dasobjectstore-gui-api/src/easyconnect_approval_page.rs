use super::*;

pub(crate) async fn easyconnect_pairing_status(
    State(state): State<EasyconnectPublicRouteState>,
    Path(pairing_id): Path<String>,
) -> Result<Json<RemoteEasyconnectPairingStatusResponse>, (StatusCode, Json<AuthRouteError>)> {
    let request = RemoteEasyconnectPairingStatusRequest {
        pairing_id: Some(pairing_id),
        browser_handoff_reference: None,
    };
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
    Extension(daemon_endpoint): Extension<EasyconnectDaemonEndpoint>,
    Query(query): Query<EasyconnectBrowserApprovalQuery>,
) -> Result<Response, (StatusCode, Json<AuthRouteError>)> {
    if query.handoff.trim().is_empty() || query.handoff.len() > 128 {
        return Err(route_error(
            StatusCode::BAD_REQUEST,
            "invalid_easyconnect_approval_page",
            "the browser handoff is unavailable",
        ));
    }
    let handoff = query.handoff.clone();
    let status = crate::daemon_bridge::DaemonBridge::shared_packaged()
        .call_message(move || {
            DaemonClient::new(UnixSocketDaemonTransport::for_bounded_bridge(
                daemon_endpoint.socket_path(),
            ))
            .remote_easyconnect_pairing_status(RemoteEasyconnectPairingStatusRequest {
                pairing_id: None,
                browser_handoff_reference: Some(handoff),
            })
            .map_err(|_| "browser handoff unavailable".to_string())
        })
        .await
        .map_err(|_| {
            route_error(
                StatusCode::GONE,
                "easyconnect_browser_handoff_unavailable",
                "this remote approval handoff is unavailable; return to the remote terminal and start a new pairing",
            )
        })?;
    let object_store = status.requested_object_store.as_deref().ok_or_else(|| {
        route_error(
            StatusCode::GONE,
            "easyconnect_browser_handoff_unavailable",
            "this remote approval handoff is unavailable; return to the remote terminal and start a new pairing",
        )
    })?;
    let intent = serde_json::to_string(&EasyconnectBrowserApprovalIntent {
        handoff: query.handoff.clone(),
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
    let object_store = html_escape(object_store);
    let nonce = csp_nonce(&verified.context().csrf_binding_sha256).map_err(|message| {
        route_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "easyconnect_approval_render_failed",
            message,
        )
    })?;
    let body = Html(approval_document(
        &principal,
        &object_store,
        nonce,
        &intent,
        &csrf,
        status.completion_mode,
    ));
    let mut response = body.into_response();
    let headers = response.headers_mut();
    headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert("referrer-policy", HeaderValue::from_static("no-referrer"));
    headers.insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    let form_action = match status.completion_mode {
        RemoteEasyconnectCompletionMode::Callback => "form-action http://127.0.0.1:*;",
        RemoteEasyconnectCompletionMode::Polling => "form-action 'none';",
    };
    let policy = format!(
        "default-src 'none'; script-src 'nonce-{nonce}'; style-src 'nonce-{nonce}'; img-src 'self'; connect-src 'self'; {form_action} base-uri 'none'; frame-ancestors 'none'"
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

/// Convert the host's `sha256:<hex>` CSRF binding into a CSP nonce.
///
/// CSP nonce sources cannot contain the `sha256:` prefix. Reusing it directly
/// caused browsers to block the only approval script, leaving the button inert.
fn csp_nonce(csrf_binding: &str) -> Result<&str, &'static str> {
    let nonce = csrf_binding
        .strip_prefix("sha256:")
        .ok_or("invalid host CSRF binding")?;
    if nonce.len() != 64 || !nonce.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("invalid host CSRF binding");
    }
    Ok(nonce)
}

fn approval_document(
    principal: &str,
    object_store: &str,
    nonce: &str,
    intent: &str,
    csrf: &str,
    completion: RemoteEasyconnectCompletionMode,
) -> String {
    let completion_script = match completion {
        RemoteEasyconnectCompletionMode::Callback => {
            r#"if(result.completion_mode!=="callback"||typeof result.callback_url!=="string"||typeof result.exchange_code!=="string"){throw new Error("The protected service returned an unexpected completion mode.");}status.textContent="Returning the approved session to the remote machine…";const form=document.createElement("form");form.method="POST";form.action=result.callback_url;for(const [name,value] of Object.entries({pairing_id:result.pairing_id,exchange_code:result.exchange_code})){const input=document.createElement("input");input.type="hidden";input.name=name;input.value=value;form.appendChild(input);}document.body.appendChild(form);form.submit();"#
        }
        RemoteEasyconnectCompletionMode::Polling => {
            r#"if(result.completion_mode!=="polling"||result.callback_url!==null||result.exchange_code!==null){throw new Error("The protected service returned an unexpected completion mode.");}const reference=typeof result.approval_reference==="string"&&/^[A-Za-z0-9:._-]{1,128}$/.test(result.approval_reference)?" Approval reference: "+result.approval_reference:"";status.textContent="Approval accepted. The remote command is securely collecting the session over its existing HTTPS connection. Return to the remote terminal."+reference;"#
        }
    };
    format!(
        r#"<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>Approve remote DASObjectStore connection</title><style nonce="{nonce}">
:root{{--mn-color-brand-footer:#1c2b0b;--mn-color-surface-canvas:#f6f7f5;--mn-color-surface-raised:#fff;--mn-color-text-primary:#111;--mn-color-text-muted:#5d6a72;--mn-color-action-primary-default:#0f6b78;--mn-color-action-primary-hover:#0b5964;--mn-color-border-subtle:#d9e0e3;--mn-color-status-danger:#8a3c25;--mn-font-family-sans:"Source Sans 3",system-ui,sans-serif;--mn-font-family-serif:"Crimson Pro",Georgia,serif}}*{{box-sizing:border-box}}body{{margin:0;background:var(--mn-color-surface-canvas);color:var(--mn-color-text-primary);font-family:var(--mn-font-family-sans)}}.shell{{min-height:100vh;display:flex;flex-direction:column}}main{{flex:1;display:grid;grid-template-columns:minmax(300px,.82fr) minmax(340px,480px);gap:22px;width:min(1040px,calc(100% - 32px));margin:auto;align-items:center;padding:32px 0}}.brand,.panel{{background:var(--mn-color-surface-raised);border:1px solid var(--mn-color-border-subtle);border-radius:8px;box-shadow:0 10px 28px #1e252b12;padding:32px}}.brand{{min-height:460px;display:flex;flex-direction:column;justify-content:center;gap:46px}}.wordmark-lockup{{display:flex;align-items:center;justify-content:center;min-height:142px}}.wordmark{{display:block;width:clamp(152px,16vw,184px);max-width:100%;max-height:122px;height:auto}}.eyebrow{{margin:0;color:var(--mn-color-text-muted);font-size:.76rem;font-weight:700;text-transform:uppercase;letter-spacing:.08em}}h1{{margin:12px 0;font-size:2.1rem;line-height:1.12}}h2{{margin:6px 0 20px;font-size:1.7rem;line-height:1.2}}.summary,.hint{{color:var(--mn-color-text-muted);line-height:1.55}}.context{{margin:20px 0;padding:0;border-top:1px solid var(--mn-color-border-subtle);border-bottom:1px solid var(--mn-color-border-subtle)}}.context div{{display:grid;grid-template-columns:minmax(120px,.45fr) 1fr;gap:16px;padding:13px 0}}.context div+div{{border-top:1px solid var(--mn-color-border-subtle)}}dt{{font-weight:700}}dd{{margin:0;overflow-wrap:anywhere}}button{{width:100%;min-height:44px;border:0;border-radius:6px;color:#fff;background:var(--mn-color-action-primary-default);font:700 1rem var(--mn-font-family-sans);cursor:pointer}}button:hover:not(:disabled){{background:var(--mn-color-action-primary-hover)}}button:focus-visible{{outline:3px solid #66b8c0;outline-offset:3px}}button:disabled{{cursor:wait;opacity:.7}}#status{{min-height:1.5rem;margin:14px 0 0;color:var(--mn-color-text-muted);font:inherit;white-space:pre-wrap}}#status[aria-live]{{font-weight:600}}footer{{position:relative;overflow:hidden;display:flex;align-items:center;gap:48px;background:var(--mn-color-brand-footer);color:#fff;padding:14px 32px;min-height:88px}}footer .product{{position:relative;z-index:1;font-weight:800}}footer .word-art{{position:relative;z-index:1;display:inline-flex;flex-direction:column;color:#fff;text-decoration:none;line-height:1}}footer .name{{font-family:var(--mn-font-family-serif);font-size:1.55rem;letter-spacing:.025em}}footer .subtitle{{margin:3px 0 0 2.2rem;color:#adada8;font-family:var(--mn-font-family-serif);font-size:.72rem;letter-spacing:.12em}}footer .mark{{position:absolute;right:-8px;bottom:-72px;height:170px;opacity:.38;pointer-events:none}}@media(max-width:760px){{main{{grid-template-columns:1fr}}.brand{{min-height:auto;gap:28px}}footer .product{{display:none}}footer .mark{{height:144px}}}}@media(max-width:480px){{.brand,.panel{{padding:16px}}h1{{font-size:1.64rem}}.context div{{grid-template-columns:1fr;gap:4px}}}}
</style></head><body><div class="shell"><main><aside class="brand" aria-labelledby="approval-purpose"><div class="wordmark-lockup"><img class="wordmark" src="/assets/mnemosyne/mnemosyne-biosciences-logo-master-mono.png" alt="Mnemosyne Biosciences"></div><div><p class="eyebrow">DASObjectStore remote access</p><h1 id="approval-purpose">Approve this short-lived connection.</h1><p class="summary">A fresh Pistis approval authorises one ObjectStore-scoped remote session. It does not disclose your GitHub credential to DASObjectStore.</p></div></aside><section class="panel" aria-labelledby="approval-title"><p class="eyebrow">Protected approval</p><h2 id="approval-title">Approve remote DASObjectStore connection</h2><dl class="context"><div><dt>Authority principal</dt><dd>{principal}</dd></div><div><dt>ObjectStore</dt><dd>{object_store}</dd></div></dl><p class="hint">Approve only if this is the connection you just initiated from your trusted remote machine.</p><button id="approve" type="button">Approve with Pistis</button><p id="status" role="status" aria-live="polite"></p></section></main><footer aria-label="Mnemosyne Biosciences provenance"><span class="product">DASObjectStore · protected by Monas v{version}</span><a class="word-art" href="https://mnemosyne.co.uk" aria-label="Mnemosyne Biosciences"><span class="name">Mnemosyne</span><span class="subtitle">Biosciences</span></a><img class="mark" src="/assets/mnemosyne/mnemosyne-biosciences-partial.png" alt="" aria-hidden="true"></footer></div><script nonce="{nonce}">
const intent={intent};const csrf={csrf};const button=document.getElementById("approve");const status=document.getElementById("status");button.onclick=async()=>{{button.disabled=true;status.textContent="Requesting protected approval…";try{{const response=await fetch("/products/dasobjectstore/api/v1/remote/easyconnect/pairings/approve",{{method:"POST",credentials:"same-origin",headers:{{"content-type":"application/json","x-dasobjectstore-csrf":csrf}},body:JSON.stringify(intent)}});let result;try{{result=await response.json();}}catch{{throw new Error("The protected service returned an invalid response.");}}if(!response.ok){{const code=typeof result.code==="string"&&/^[a-z0-9_]{{1,80}}$/.test(result.code)?" ("+result.code+")":"";throw new Error("Approval was not accepted"+code+". Keep the remote terminal open and start a new pairing if it has expired.");}}{completion_script}}}catch(error){{status.textContent=error instanceof Error?error.message:"Approval could not be completed.";button.disabled=false;}}}};
</script></body></html>"#,
        version = env!("CARGO_PKG_VERSION"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csp_nonce_removes_the_csrf_hash_prefix() {
        let binding = format!("sha256:{}", "a".repeat(64));
        assert_eq!(
            csp_nonce(&binding),
            Ok("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
        );
        assert!(csp_nonce("sha256:not-a-valid-binding").is_err());
    }

    #[test]
    fn approval_document_keeps_the_approved_script_and_brand_footer_under_one_nonce() {
        let page = approval_document(
            "principal",
            "alleleanchor_mvp",
            "aabb",
            r#"{\"pairing_id\":\"pairing\",\"object_store\":\"alleleanchor_mvp\"}"#,
            r#"\"sha256:binding\""#,
            RemoteEasyconnectCompletionMode::Callback,
        );
        assert!(page.contains(r#"style nonce="aabb""#));
        assert!(page.contains(r#"script nonce="aabb""#));
        assert!(page.contains("Approve with Pistis"));
        assert!(page.contains("mnemosyne-biosciences-partial.png"));
        assert!(page.contains("x-dasobjectstore-csrf"));
    }

    #[test]
    fn polling_approval_page_never_constructs_or_posts_a_loopback_form() {
        let page = approval_document(
            "principal",
            "alleleanchor_mvp",
            "aabb",
            r#"{\"pairing_id\":\"pairing\",\"object_store\":\"alleleanchor_mvp\"}"#,
            r#"\"sha256:binding\""#,
            RemoteEasyconnectCompletionMode::Polling,
        );

        assert!(page.contains("completion_mode!==\"polling\""));
        assert!(page.contains("securely collecting the session"));
        assert!(!page.contains("createElement(\"form\")"));
        assert!(!page.contains("exchange_code:result.exchange_code"));
    }
}
