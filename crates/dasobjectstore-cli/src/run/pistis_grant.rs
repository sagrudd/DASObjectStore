use super::CliError;
use crate::cli::{PistisGrantInspectArgs, PistisGrantMutationArgs, PistisGrantRevokeArgs};
use dasobjectstore_core::store::ExportPolicy;
use dasobjectstore_mnemosyne::{PistisGrantPolicyStore, PistisObjectStoreGrantRegistry};
use dasobjectstore_object_service::read_store_registry;
use prosopikon_core::{PrincipalStatus, ProsopikonAuthority, SqliteProsopikonAuthority};
use std::collections::BTreeSet;
use std::io::Write;
use uuid::Uuid;

pub(super) fn inspect(
    args: &PistisGrantInspectArgs,
    writer: &mut impl Write,
) -> Result<(), CliError> {
    require_absolute(args.grant_registry(), "grant registry")?;
    let registry = PistisGrantPolicyStore::new(args.grant_registry())
        .inspect()
        .map_err(policy_error)?;
    write_registry(writer, registry.as_ref())
}

pub(super) fn grant(
    args: &PistisGrantMutationArgs,
    writer: &mut impl Write,
) -> Result<(), CliError> {
    require_absolute(args.authority(), "Prosopikon authority")?;
    require_absolute(args.grant_registry(), "grant registry")?;
    require_absolute(args.store_registry(), "ObjectStore registry")?;
    validate_object_store(args.store_registry(), args.object_store())?;
    let (authority_id, principal_id) = resolve_principal(args.authority(), args.email())?;
    let allowed_prefixes = normalize_prefixes(args.allowed_prefixes())?;
    let registry = PistisGrantPolicyStore::new(args.grant_registry())
        .grant(
            args.expected_revision(),
            authority_id,
            principal_id,
            args.object_store().to_owned(),
            args.read(),
            args.write(),
            allowed_prefixes,
        )
        .map_err(policy_error)?;
    write_registry(writer, Some(&registry))
}

pub(super) fn revoke(
    args: &PistisGrantRevokeArgs,
    writer: &mut impl Write,
) -> Result<(), CliError> {
    require_absolute(args.authority(), "Prosopikon authority")?;
    require_absolute(args.grant_registry(), "grant registry")?;
    let (authority_id, principal_id) = resolve_principal(args.authority(), args.email())?;
    let registry = PistisGrantPolicyStore::new(args.grant_registry())
        .revoke(
            args.expected_revision(),
            authority_id,
            principal_id,
            args.object_store(),
        )
        .map_err(policy_error)?;
    write_registry(writer, Some(&registry))
}

fn resolve_principal(
    authority_path: &std::path::Path,
    email: &str,
) -> Result<(Uuid, Uuid), CliError> {
    let email = email.trim();
    if email.is_empty() {
        return Err(CliError::CommandFailed(
            "email provisioning selector must not be empty".to_owned(),
        ));
    }
    let authority = SqliteProsopikonAuthority::open(authority_path);
    let snapshot = authority
        .snapshot()
        .map_err(|error| CliError::CommandFailed(error.to_string()))?;
    let matches = snapshot
        .principals
        .iter()
        .filter(|principal| {
            principal.status == PrincipalStatus::Active
                && principal
                    .email
                    .as_deref()
                    .is_some_and(|candidate| candidate.eq_ignore_ascii_case(email))
        })
        .collect::<Vec<_>>();
    let [principal] = matches.as_slice() else {
        return Err(CliError::CommandFailed(if matches.is_empty() {
            "email did not resolve to an active Prosopikon principal".to_owned()
        } else {
            "email resolved to more than one active Prosopikon principal".to_owned()
        }));
    };
    Ok((snapshot.metadata.authority_id, principal.principal_id))
}

fn validate_object_store(
    registry_path: &std::path::Path,
    object_store_id: &str,
) -> Result<(), CliError> {
    if object_store_id.trim() != object_store_id || object_store_id.is_empty() {
        return Err(CliError::CommandFailed(
            "ObjectStore ID must be exact and non-empty".to_owned(),
        ));
    }
    let definitions = read_store_registry(registry_path)?;
    let matches = definitions
        .iter()
        .filter(|definition| definition.store_id.as_str() == object_store_id)
        .collect::<Vec<_>>();
    let [definition] = matches.as_slice() else {
        return Err(CliError::CommandFailed(if matches.is_empty() {
            "exact ObjectStore ID was not found".to_owned()
        } else {
            "ObjectStore registry contains an ambiguous ID".to_owned()
        }));
    };
    if definition.policy.export_policy != ExportPolicy::S3 {
        return Err(CliError::CommandFailed(
            "ObjectStore is not exported through the S3 service".to_owned(),
        ));
    }
    Ok(())
}

fn normalize_prefixes(prefixes: Vec<String>) -> Result<Vec<String>, CliError> {
    let mut normalized = BTreeSet::new();
    for prefix in prefixes {
        if prefix.trim() != prefix || prefix.is_empty() || prefix.starts_with('/') {
            return Err(CliError::CommandFailed(
                "allowed prefixes must be non-empty relative key prefixes".to_owned(),
            ));
        }
        normalized.insert(prefix);
    }
    Ok(normalized.into_iter().collect())
}

fn require_absolute(path: &std::path::Path, label: &str) -> Result<(), CliError> {
    if path.is_absolute() {
        Ok(())
    } else {
        Err(CliError::CommandFailed(format!(
            "{label} path must be absolute"
        )))
    }
}

fn policy_error(error: impl std::fmt::Display) -> CliError {
    CliError::CommandFailed(error.to_string())
}

fn write_registry(
    writer: &mut impl Write,
    registry: Option<&PistisObjectStoreGrantRegistry>,
) -> Result<(), CliError> {
    let value = match registry {
        Some(registry) => serde_json::to_value(registry)?,
        None => serde_json::json!({
            "schema_version": "dasobjectstore.pistis-grant-registry.v1",
            "revision": 0,
            "records": []
        }),
    };
    serde_json::to_writer_pretty(&mut *writer, &value)?;
    writeln!(writer)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{DateTime, Utc};
    use prosopikon_core::{
        AuthorityMetadata, AuthoritySnapshot, Principal, PrincipalKind, AUTHORITY_CONTRACT_VERSION,
    };
    use std::fs;

    fn authority_with_principals(principals: Vec<Principal>) -> (std::path::PathBuf, Uuid) {
        let root = std::env::temp_dir().join(format!("das-pistis-cli-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        }
        let authority_id = Uuid::from_u128(40);
        SqliteProsopikonAuthority::initialize(
            root.join("authority.sqlite3"),
            AuthoritySnapshot {
                metadata: AuthorityMetadata {
                    contract_version: AUTHORITY_CONTRACT_VERSION.to_owned(),
                    authority_id,
                    revision: 0,
                    generated_at_utc: DateTime::<Utc>::from_timestamp(1_750_000_000, 0).unwrap(),
                },
                principals,
                tenants: Vec::new(),
                memberships: Vec::new(),
                role_assignments: Vec::new(),
                product_entitlements: Vec::new(),
                sessions: Vec::new(),
                pistis_bindings: Vec::new(),
            },
        )
        .unwrap();
        (root, authority_id)
    }

    fn principal(id: u128, email: Option<&str>, status: PrincipalStatus) -> Principal {
        let now = DateTime::<Utc>::from_timestamp(1_750_000_000, 0).unwrap();
        Principal {
            principal_id: Uuid::from_u128(id),
            username: format!("principal-{id}"),
            display_name: None,
            email: email.map(ToOwned::to_owned),
            kind: PrincipalKind::Person,
            status,
            created_at_utc: now,
            updated_at_utc: now,
        }
    }

    #[test]
    fn email_resolves_once_to_immutable_authority_and_principal() {
        let (root, authority_id) = authority_with_principals(vec![
            principal(41, Some("stephen@mnemosyne.co.uk"), PrincipalStatus::Active),
            principal(
                42,
                Some("archived@mnemosyne.co.uk"),
                PrincipalStatus::Archived,
            ),
        ]);
        let resolved =
            resolve_principal(&root.join("authority.sqlite3"), "Stephen@Mnemosyne.co.uk").unwrap();
        assert_eq!(resolved, (authority_id, Uuid::from_u128(41)));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn absent_ambiguous_and_inactive_email_resolution_fails_closed() {
        for principals in [
            vec![principal(
                41,
                Some("stephen@mnemosyne.co.uk"),
                PrincipalStatus::Archived,
            )],
            vec![
                principal(41, Some("stephen@mnemosyne.co.uk"), PrincipalStatus::Active),
                principal(42, Some("STEPHEN@MNEMOSYNE.CO.UK"), PrincipalStatus::Active),
            ],
        ] {
            let (root, _) = authority_with_principals(principals);
            assert!(
                resolve_principal(&root.join("authority.sqlite3"), "stephen@mnemosyne.co.uk")
                    .is_err()
            );
            fs::remove_dir_all(root).unwrap();
        }
    }
}
