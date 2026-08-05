use super::*;

/// The historical remote bootstrap exchanged an appliance-local password for
/// an S3 session.  It is intentionally retained as a narrow rejection point
/// so old invocations fail before they open a connection, prompt a terminal,
/// or write trust/configuration state.
pub(super) fn run_authenticate(
    _cli: &RemoteCli,
    _args: &AuthenticateArgs,
    _writer: &mut impl Write,
) -> Result<(), RemoteRunError> {
    Err(RemoteAuthenticateError::RetiredLocalPassword.into())
}

/// Compatibility entry point for legacy resync and recovery call sites.
///
/// There is no password-based recovery fallback.  Callers must instead use a
/// browser-approved Pistis EasyConnect ceremony or a site-issued passwordless
/// credential helper.
pub(super) fn run_authenticate_with_identity_policy(
    _cli: &RemoteCli,
    _args: &AuthenticateArgs,
    _writer: &mut impl Write,
    _allow_confirmed_identity_replacement: bool,
) -> Result<(), RemoteRunError> {
    Err(RemoteAuthenticateError::RetiredLocalPassword.into())
}

pub(super) fn run_s3_status(
    cli: &RemoteCli,
    store: &str,
    requested_profile: Option<&str>,
    _json: bool,
    writer: &mut impl Write,
) -> Result<(), RemoteRunError> {
    let config = read_optional_config(&config_path(cli)?)?.unwrap_or_else(empty_config);
    let binding = config.session_binding(store)?;
    let associations = config
        .s3_profiles
        .iter()
        .filter(|entry| {
            entry.store_id == store
                && requested_profile.is_none_or(|profile| profile == entry.profile)
        })
        .collect::<Vec<_>>();
    let association = match associations.as_slice() {
        [association] => *association,
        [] => {
            return Err(RemoteRunError::UploadRouting(format!(
                "no DASObjectStore-managed AWS profile is associated with ObjectStore {store}; complete Pistis EasyConnect with S3 profile installation"
            )))
        }
        _ => {
            return Err(RemoteRunError::UploadRouting(
                "profile_association_mismatch: multiple AWS profiles match the requested ObjectStore"
                    .to_string(),
            ))
        }
    };
    if binding.s3_profile.as_deref() != Some(association.profile.as_str())
        || binding.s3_endpoint_url != association.endpoint_url
        || binding.bucket != association.bucket
        || binding.session.expires_at != association.expires_at.clone().unwrap_or_default()
    {
        return Err(RemoteRunError::UploadRouting(
            "s3_control_generation_mismatch: AWS and HTTPS control state are not from the same committed generation; run `dasobjectstore-remote config repair --dry-run --json`"
                .to_string(),
        ));
    }
    serde_json::to_writer_pretty(&mut *writer, &s3_profile_status(association, true)?)?;
    writer.write_all(b"\n")?;
    Ok(())
}
