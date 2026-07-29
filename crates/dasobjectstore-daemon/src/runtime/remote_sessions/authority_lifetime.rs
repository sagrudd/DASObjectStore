use super::{
    RemoteEasyconnectAuthProvider, RemoteEasyconnectPairedSessionRecord,
    RemoteEasyconnectPairedSessionStoreError,
};

pub(super) fn ensure_session_usable(
    session: &RemoteEasyconnectPairedSessionRecord,
    now_utc: &str,
) -> Result<(), RemoteEasyconnectPairedSessionStoreError> {
    validate_originating_authority_lifetime(session)?;
    if let Some(revoked_at_utc) = &session.revoked_at_utc {
        return Err(RemoteEasyconnectPairedSessionStoreError::SessionRevoked {
            session_id: session.session_id.clone(),
            revoked_at_utc: revoked_at_utc.clone(),
        });
    }
    if session.expires_at_utc.as_str() <= now_utc {
        return Err(RemoteEasyconnectPairedSessionStoreError::SessionExpired {
            session_id: session.session_id.clone(),
            expires_at_utc: session.expires_at_utc.clone(),
        });
    }
    Ok(())
}

pub(super) fn validate_originating_authority_lifetime(
    session: &RemoteEasyconnectPairedSessionRecord,
) -> Result<(), RemoteEasyconnectPairedSessionStoreError> {
    if session.originating_authority_expires_at_utc.is_none() {
        return if session.auth_provider == RemoteEasyconnectAuthProvider::Pistis {
            Err(
                RemoteEasyconnectPairedSessionStoreError::MissingOriginatingAuthorityExpiry {
                    session_id: session.session_id.clone(),
                },
            )
        } else {
            Ok(())
        };
    }
    ensure_renewal_within_originating_authority(session, &session.expires_at_utc)
}

pub(super) fn ensure_renewal_within_originating_authority(
    session: &RemoteEasyconnectPairedSessionRecord,
    requested_expires_at_utc: &str,
) -> Result<(), RemoteEasyconnectPairedSessionStoreError> {
    let requested_expiry = canonical_utc_seconds("expires_at_utc", requested_expires_at_utc)?;
    if let Some(originating_expiry) = session.originating_authority_expires_at_utc.as_deref() {
        let originating_expiry_seconds =
            canonical_utc_seconds("originating_authority_expires_at_utc", originating_expiry)?;
        if requested_expiry <= originating_expiry_seconds {
            return Ok(());
        }
        return Err(
            RemoteEasyconnectPairedSessionStoreError::OriginatingAuthorityExpiryExceeded {
                session_id: session.session_id.clone(),
                originating_authority_expires_at_utc: originating_expiry.to_string(),
            },
        );
    }
    Ok(())
}

fn canonical_utc_seconds(
    field: &'static str,
    value: &str,
) -> Result<i64, RemoteEasyconnectPairedSessionStoreError> {
    dasobjectstore_core::utc::parse_canonical_utc_timestamp_seconds(value)
        .ok_or(RemoteEasyconnectPairedSessionStoreError::InvalidTimestamp { field })
}
