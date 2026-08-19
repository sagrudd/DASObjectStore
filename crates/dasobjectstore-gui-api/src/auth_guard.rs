use axum::{
    extract::FromRequestParts,
    http::{request::Parts, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthenticatedActorAuthority {
    MonasStandalone,
    SynoptikonIntegrated,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AuthenticatedGuiActor {
    pub subject_id: String,
    pub authority: AuthenticatedActorAuthority,
    pub roles: Vec<String>,
    pub expires_at_unix_seconds: Option<i64>,
    pub correlation_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FederatedHostSessionResponse {
    pub subject_id: String,
    pub authority: AuthenticatedActorAuthority,
    pub roles: Vec<String>,
    pub expires_at_unix_seconds: Option<i64>,
    pub correlation_id: Option<String>,
    /// Same-origin mutation token bound to the live host session. This is not
    /// a bearer credential and grants no storage authority on its own.
    pub csrf_token: String,
}

impl FederatedHostSessionResponse {
    pub fn from_host_actor(actor: AuthenticatedGuiActor, csrf_token: String) -> Self {
        Self {
            subject_id: actor.subject_id,
            authority: actor.authority,
            roles: actor.roles,
            expires_at_unix_seconds: actor.expires_at_unix_seconds,
            correlation_id: actor.correlation_id,
            csrf_token,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AuthGuardError {
    pub code: String,
    pub message: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthGuardRejection {
    pub status: StatusCode,
    pub error: AuthGuardError,
}

impl IntoResponse for AuthGuardRejection {
    fn into_response(self) -> Response {
        (self.status, Json(self.error)).into_response()
    }
}

impl<S> FromRequestParts<S> for AuthenticatedGuiActor
where
    S: Send + Sync,
{
    type Rejection = AuthGuardRejection;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        if let Some(verified) = parts
            .extensions
            .get::<crate::VerifiedHostAuthenticatedContext>()
        {
            let context = verified.context();
            let authority = match context.authority {
                crate::HostAuthenticationAuthority::MonasStandalone => {
                    AuthenticatedActorAuthority::MonasStandalone
                }
                crate::HostAuthenticationAuthority::SynoptikonIntegrated => {
                    AuthenticatedActorAuthority::SynoptikonIntegrated
                }
            };
            return Ok(Self {
                subject_id: context.subject_id.clone(),
                authority,
                roles: context.roles.clone(),
                expires_at_unix_seconds: Some(context.expires_at_unix_seconds),
                correlation_id: Some(context.correlation_id.clone()),
            });
        }
        Err(missing_auth_context())
    }
}

fn missing_auth_context() -> AuthGuardRejection {
    rejection(
        StatusCode::UNAUTHORIZED,
        "missing_auth_context",
        "authenticated actor context is required",
    )
}

fn rejection(
    status: StatusCode,
    code: impl Into<String>,
    message: impl Into<String>,
) -> AuthGuardRejection {
    AuthGuardRejection {
        status,
        error: AuthGuardError {
            code: code.into(),
            message: message.into(),
        },
    }
}
