//! Garage bucket and per-store key provisioning plans.

use crate::credentials::StoreServiceCredential;
use crate::provider::ObjectServiceError;
use std::collections::BTreeSet;
use std::fmt;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GarageProvisioningPlan {
    pub commands: Vec<GarageProvisioningCommand>,
}

impl GarageProvisioningPlan {
    pub fn bucket_count(&self) -> usize {
        self.commands
            .iter()
            .filter(|command| command.kind == GarageProvisioningCommandKind::CreateBucket)
            .count()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GarageProvisioningCommand {
    pub kind: GarageProvisioningCommandKind,
    pub idempotent: bool,
    args: Vec<GarageCliArgument>,
}

impl GarageProvisioningCommand {
    pub fn argv(&self) -> Vec<String> {
        self.args
            .iter()
            .map(|argument| argument.value.clone())
            .collect()
    }

    pub fn redacted_argv(&self) -> Vec<String> {
        self.args
            .iter()
            .map(GarageCliArgument::redacted_value)
            .collect()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GarageProvisioningCommandKind {
    ImportKey,
    CreateBucket,
    AllowBucket,
}

#[derive(Clone, Eq, PartialEq)]
struct GarageCliArgument {
    value: String,
    sensitive: bool,
}

impl GarageCliArgument {
    fn public(value: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            sensitive: false,
        }
    }

    fn sensitive(value: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            sensitive: true,
        }
    }

    fn redacted_value(&self) -> String {
        if self.sensitive {
            "<redacted>".to_string()
        } else {
            self.value.clone()
        }
    }
}

impl fmt::Debug for GarageCliArgument {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.redacted_value())
    }
}

pub fn plan_garage_provisioning(
    credentials: &[StoreServiceCredential],
) -> Result<GarageProvisioningPlan, ObjectServiceError> {
    validate_credentials(credentials)?;

    let mut commands = Vec::with_capacity(credentials.len() * 3);
    for credential in credentials {
        let key_name = format!("dasobjectstore:{}", credential.store_id);
        commands.push(GarageProvisioningCommand {
            kind: GarageProvisioningCommandKind::ImportKey,
            idempotent: true,
            args: vec![
                GarageCliArgument::public("key"),
                GarageCliArgument::public("import"),
                GarageCliArgument::public("--yes"),
                // Garage 2.3 accepts the key name through the short `-n`
                // option; the long `--name` form is not available in the
                // appliance image and causes provisioning to abort.
                GarageCliArgument::public("-n"),
                GarageCliArgument::public(key_name),
                GarageCliArgument::public(&credential.access_key_id),
                GarageCliArgument::sensitive(credential.secret_access_key.expose_secret()),
            ],
        });
        commands.push(GarageProvisioningCommand {
            kind: GarageProvisioningCommandKind::CreateBucket,
            idempotent: true,
            args: vec![
                GarageCliArgument::public("bucket"),
                GarageCliArgument::public("create"),
                GarageCliArgument::public(&credential.bucket_name),
            ],
        });
        commands.push(GarageProvisioningCommand {
            kind: GarageProvisioningCommandKind::AllowBucket,
            idempotent: true,
            args: vec![
                GarageCliArgument::public("bucket"),
                GarageCliArgument::public("allow"),
                GarageCliArgument::public("--read"),
                GarageCliArgument::public("--write"),
                GarageCliArgument::public("--owner"),
                GarageCliArgument::public(&credential.bucket_name),
                GarageCliArgument::public("--key"),
                GarageCliArgument::public(&credential.access_key_id),
            ],
        });
    }

    Ok(GarageProvisioningPlan { commands })
}

fn validate_credentials(credentials: &[StoreServiceCredential]) -> Result<(), ObjectServiceError> {
    if credentials.is_empty() {
        return Err(ObjectServiceError::InvalidConfiguration(
            "at least one Garage credential is required for provisioning".to_string(),
        ));
    }

    let mut store_ids = BTreeSet::new();
    let mut bucket_names = BTreeSet::new();
    let mut access_keys = BTreeSet::new();
    for credential in credentials {
        if !store_ids.insert(credential.store_id.as_str()) {
            return Err(ObjectServiceError::InvalidConfiguration(format!(
                "duplicate Garage credential for store: {}",
                credential.store_id
            )));
        }
        if !bucket_names.insert(credential.bucket_name.as_str()) {
            return Err(ObjectServiceError::InvalidConfiguration(format!(
                "duplicate Garage credential for bucket: {}",
                credential.bucket_name
            )));
        }
        if !access_keys.insert(credential.access_key_id.as_str()) {
            return Err(ObjectServiceError::InvalidConfiguration(format!(
                "duplicate Garage access key id: {}",
                credential.access_key_id
            )));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{plan_garage_provisioning, GarageProvisioningCommandKind};
    use crate::credentials::{
        generate_per_store_credentials, CredentialEntropy, StoreCredentialRequest,
    };
    use crate::provider::ObjectServiceError;
    use dasobjectstore_core::ids::StoreId;

    #[test]
    fn plans_import_create_and_allow_commands_per_store() {
        let credentials = credentials();

        let plan = plan_garage_provisioning(&credentials).expect("plan created");

        assert_eq!(plan.bucket_count(), 2);
        assert_eq!(plan.commands.len(), 6);
        assert_eq!(
            plan.commands[0].kind,
            GarageProvisioningCommandKind::ImportKey
        );
        assert_eq!(plan.commands[0].argv()[0..2], ["key", "import"]);
        assert_eq!(
            plan.commands[0].argv()[2..5],
            ["--yes", "-n", "dasobjectstore:generated"]
        );
        assert_eq!(
            plan.commands[1].argv(),
            ["bucket", "create", "dos-generated"]
        );
        assert_eq!(
            plan.commands[2].argv(),
            [
                "bucket",
                "allow",
                "--read",
                "--write",
                "--owner",
                "dos-generated",
                "--key",
                credentials[0].access_key_id.as_str()
            ]
        );
    }

    #[test]
    fn redacts_imported_secret_key_from_debug_and_redacted_args() {
        let credentials = credentials();
        let secret = credentials[0].secret_access_key.expose_secret().to_string();

        let plan = plan_garage_provisioning(&credentials).expect("plan created");

        assert!(plan.commands[0].argv().contains(&secret));
        assert!(!plan.commands[0].redacted_argv().contains(&secret));
        assert!(!format!("{:?}", plan.commands[0]).contains(&secret));
    }

    #[test]
    fn rejects_duplicate_access_keys() {
        let mut credentials = credentials();
        credentials[1].access_key_id = credentials[0].access_key_id.clone();

        let err = plan_garage_provisioning(&credentials).expect_err("duplicate rejected");

        assert!(matches!(
            err,
            ObjectServiceError::InvalidConfiguration(message)
                if message.contains("duplicate Garage access key id")
        ));
    }

    fn credentials() -> Vec<crate::credentials::StoreServiceCredential> {
        generate_per_store_credentials(
            &[
                request("generated", "dos-generated"),
                request("critical", "dos-critical"),
            ],
            &mut FixedEntropy::default(),
        )
        .expect("credentials generated")
    }

    fn request(store_id: &str, bucket_name: &str) -> StoreCredentialRequest {
        StoreCredentialRequest {
            store_id: StoreId::new(store_id).expect("store id"),
            bucket_name: bucket_name.to_string(),
        }
    }

    #[derive(Default)]
    struct FixedEntropy {
        next: u8,
    }

    impl CredentialEntropy for FixedEntropy {
        fn fill(&mut self, bytes: &mut [u8]) -> Result<(), ObjectServiceError> {
            for byte in bytes {
                *byte = self.next;
                self.next = self.next.wrapping_add(1);
            }
            Ok(())
        }
    }
}
