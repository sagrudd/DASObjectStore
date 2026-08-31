use super::*;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemoteEasyconnectPairingStatusRequest {
    pub pairing_id: Option<String>,
    /// Opaque, single-purpose browser handoff. It is resolved only by the
    /// package-owned pairing store and is never copied into an approval error.
    pub browser_handoff_reference: Option<String>,
}

impl RemoteEasyconnectPairingStatusRequest {
    pub fn validate(&self) -> Result<(), RemoteEasyconnectValidationError> {
        match (
            self.pairing_id
                .as_deref()
                .filter(|value| !value.trim().is_empty()),
            self.browser_handoff_reference
                .as_deref()
                .filter(|value| !value.trim().is_empty()),
        ) {
            (Some(_), None) | (None, Some(_)) => Ok(()),
            _ => Err(RemoteEasyconnectValidationError::InvalidPairingStatusReference),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::RemoteEasyconnectPairingStatusRequest;

    #[test]
    fn status_requires_exactly_one_pairing_or_browser_handoff_reference() {
        assert!(RemoteEasyconnectPairingStatusRequest {
            pairing_id: Some("pairing-1".to_string()),
            browser_handoff_reference: None,
        }
        .validate()
        .is_ok());
        assert!(RemoteEasyconnectPairingStatusRequest {
            pairing_id: None,
            browser_handoff_reference: Some("handoff-1".to_string()),
        }
        .validate()
        .is_ok());
        assert!(RemoteEasyconnectPairingStatusRequest {
            pairing_id: Some("pairing-1".to_string()),
            browser_handoff_reference: Some("handoff-1".to_string()),
        }
        .validate()
        .is_err());
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteEasyconnectPairingState {
    Pending,
    Approved,
    Exchanged,
    Expired,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemoteEasyconnectPairingStatusResponse {
    pub pairing_id: String,
    pub state: RemoteEasyconnectPairingState,
    pub expires_at_utc: String,
    pub requested_object_store: Option<String>,
    pub completion_mode: RemoteEasyconnectCompletionMode,
    /// Returned only while an unexpired approval is ready for the holder of
    /// the 256-bit pairing capability.
    pub exchange_code: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemoteEasyconnectApprovePairingRequest {
    pub pairing_id: String,
    pub approval_context: RemoteEasyconnectApprovalContext,
}

impl RemoteEasyconnectApprovePairingRequest {
    pub fn validate(&self) -> Result<(), RemoteEasyconnectValidationError> {
        require_non_blank("pairing_id", &self.pairing_id)?;
        self.approval_context.validate()
    }
}

/// Credential-free authority and authorization facts supplied by the
/// embedding host after it verifies the live Pistis session.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RemoteEasyconnectApprovalContext {
    pub authority_id: String,
    pub principal_id: String,
    pub session_id: String,
    pub auth_provider: RemoteEasyconnectAuthProvider,
    pub allowed_object_stores: Vec<RemoteEasyconnectObjectStoreGrant>,
    pub host_session_expires_at_utc: String,
    pub correlation_id: String,
    pub audit_identity: String,
}

impl RemoteEasyconnectApprovalContext {
    pub fn validate(&self) -> Result<(), RemoteEasyconnectValidationError> {
        for (field, value) in [
            ("authority_id", self.authority_id.as_str()),
            ("principal_id", self.principal_id.as_str()),
            ("session_id", self.session_id.as_str()),
            (
                "host_session_expires_at_utc",
                self.host_session_expires_at_utc.as_str(),
            ),
            ("correlation_id", self.correlation_id.as_str()),
            ("audit_identity", self.audit_identity.as_str()),
        ] {
            require_non_blank(field, value)?;
        }
        if !matches!(
            self.auth_provider,
            RemoteEasyconnectAuthProvider::StandaloneLocalUser
                | RemoteEasyconnectAuthProvider::Pistis
        ) {
            return Err(RemoteEasyconnectValidationError::InvalidApprovalProvider);
        }
        if self.allowed_object_stores.is_empty() {
            return Err(RemoteEasyconnectValidationError::EmptyObjectStoreGrants);
        }
        for grant in &self.allowed_object_stores {
            grant.validate()?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemoteEasyconnectApprovePairingResponse {
    pub pairing_id: String,
    /// The exchange capability is returned only to a same-host callback flow.
    /// A polling client obtains it from the capability-protected status route.
    pub exchange_code: Option<String>,
    pub callback_url: Option<String>,
    pub completion_mode: RemoteEasyconnectCompletionMode,
    /// Safe, durable host correlation reference for attended support. This is
    /// never an exchange capability, credential, or identity secret.
    pub approval_reference: String,
    pub expires_at_utc: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemoteEasyconnectExchangePairingRequest {
    pub pairing_id: String,
    pub exchange_code: String,
    pub client_request_id: Option<String>,
}

impl RemoteEasyconnectExchangePairingRequest {
    pub fn validate(&self) -> Result<(), RemoteEasyconnectValidationError> {
        require_non_blank("pairing_id", &self.pairing_id)?;
        require_non_blank("exchange_code", &self.exchange_code)?;
        validate_optional_non_blank("client_request_id", self.client_request_id.as_deref())?;
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemoteEasyconnectExchangePairingResponse {
    pub appliance_id: String,
    pub appliance_base_url: String,
    pub approved_actor: String,
    pub auth_provider: RemoteEasyconnectAuthProvider,
    pub session: RemoteEasyconnectSession,
    pub object_stores: Vec<RemoteEasyconnectObjectStoreGrant>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemoteEasyconnectS3ConnectionDescriptor {
    pub endpoint_url: String,
    pub region: String,
    pub addressing_style: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemoteEasyconnectExchangeConnectionResponse {
    pub schema_version: String,
    #[serde(flatten)]
    pub exchange: RemoteEasyconnectExchangePairingResponse,
    pub s3: RemoteEasyconnectS3ConnectionDescriptor,
}
