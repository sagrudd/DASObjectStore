//! Garage-backed application object deletion authority.

use super::{DaemonServiceRuntimeError, ServiceCommandRunner};
use dasobjectstore_core::ids::StoreId;
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApplicationObjectDeletion {
    pub store_id: StoreId,
    pub object_id: String,
    pub object_version: u64,
    pub object_key: String,
    pub expected_size_bytes: u64,
    pub expected_checksum: String,
    pub provider: String,
    pub bucket: String,
    pub endpoint_url: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApplicationObjectDeletionOutcome {
    Deleted,
    AlreadyAbsent,
}

pub struct GarageApplicationObjectDeletionAuthority<'a> {
    runner: &'a dyn ServiceCommandRunner,
    environment: Vec<(String, String)>,
    live_sqlite_path: PathBuf,
}

impl<'a> GarageApplicationObjectDeletionAuthority<'a> {
    pub fn new(
        runner: &'a dyn ServiceCommandRunner,
        environment: Vec<(String, String)>,
        live_sqlite_path: PathBuf,
    ) -> Self {
        Self {
            runner,
            environment,
            live_sqlite_path,
        }
    }

    pub fn delete(
        &self,
        deletion: &ApplicationObjectDeletion,
    ) -> Result<ApplicationObjectDeletionOutcome, DaemonServiceRuntimeError> {
        if deletion.provider != "garage" {
            return Err(delete_error(
                "application object deletion supports only the configured Garage provider",
            ));
        }
        let provider_object_key = format!("{}/{}", deletion.bucket, deletion.object_key);
        let evidence = dasobjectstore_metadata::ProfileCatalogueObjectWithdrawalRequest {
            profile_namespace: "provider:garage",
            store_id: &deletion.store_id,
            object_id: &deletion.object_id,
            object_version: deletion.object_version,
            expected_size_bytes: deletion.expected_size_bytes,
            expected_checksum: &deletion.expected_checksum,
            expected_provider: &deletion.provider,
            expected_provider_object_key: &provider_object_key,
        };
        let catalogued = dasobjectstore_metadata::profile_catalogue_object_matches(
            &self.live_sqlite_path,
            &evidence,
        )
        .map_err(|error| delete_error(error.to_string()))?;
        let provider_has_object = self.provider_has_exact_object(deletion)?;
        if !catalogued {
            return if provider_has_object {
                Err(delete_error(
                    "provider object exists without matching authoritative catalogue evidence",
                ))
            } else {
                Ok(ApplicationObjectDeletionOutcome::AlreadyAbsent)
            };
        }

        if provider_has_object {
            self.verify_provider_object(deletion)?;
            self.run_aws(&[
                "s3api",
                "delete-object",
                "--bucket",
                &deletion.bucket,
                "--key",
                &deletion.object_key,
                "--endpoint-url",
                &deletion.endpoint_url,
                "--output",
                "json",
            ])?;
            if self.provider_has_exact_object(deletion)? {
                return Err(delete_error(
                    "provider still reports the exact object after deletion",
                ));
            }
        }

        dasobjectstore_metadata::withdraw_profile_catalogue_object(
            &self.live_sqlite_path,
            evidence,
        )
        .map_err(|error| delete_error(error.to_string()))?;
        Ok(if provider_has_object {
            ApplicationObjectDeletionOutcome::Deleted
        } else {
            ApplicationObjectDeletionOutcome::AlreadyAbsent
        })
    }

    fn verify_provider_object(
        &self,
        deletion: &ApplicationObjectDeletion,
    ) -> Result<(), DaemonServiceRuntimeError> {
        let output = self.run_aws(&[
            "s3api",
            "head-object",
            "--bucket",
            &deletion.bucket,
            "--key",
            &deletion.object_key,
            "--endpoint-url",
            &deletion.endpoint_url,
            "--output",
            "json",
        ])?;
        let head: GarageHeadObject = serde_json::from_str(&output.stdout)
            .map_err(|error| delete_error(format!("invalid provider HEAD response: {error}")))?;
        if head.content_length != deletion.expected_size_bytes {
            return Err(delete_error(
                "provider size does not match exact deletion evidence",
            ));
        }
        let expected = deletion.expected_checksum.trim_start_matches("sha256:");
        if !head
            .metadata
            .dasobjectstore_sha256
            .as_deref()
            .is_some_and(|actual| actual.eq_ignore_ascii_case(expected))
        {
            return Err(delete_error(
                "provider checksum does not match exact deletion evidence",
            ));
        }
        Ok(())
    }

    fn provider_has_exact_object(
        &self,
        deletion: &ApplicationObjectDeletion,
    ) -> Result<bool, DaemonServiceRuntimeError> {
        let output = self.run_aws(&[
            "s3api",
            "list-objects-v2",
            "--bucket",
            &deletion.bucket,
            "--prefix",
            &deletion.object_key,
            "--max-items",
            "2",
            "--endpoint-url",
            &deletion.endpoint_url,
            "--output",
            "json",
        ])?;
        let listing: GarageObjectListing = serde_json::from_str(&output.stdout)
            .map_err(|error| delete_error(format!("invalid provider list response: {error}")))?;
        Ok(listing
            .contents
            .iter()
            .any(|entry| entry.key == deletion.object_key))
    }

    fn run_aws(
        &self,
        args: &[&str],
    ) -> Result<super::ServiceCommandOutput, DaemonServiceRuntimeError> {
        let args = args
            .iter()
            .map(|argument| (*argument).to_string())
            .collect::<Vec<_>>();
        self.runner
            .run_with_display_args_and_env("aws", &args, &args, &self.environment)
    }
}

fn delete_error(message: impl Into<String>) -> DaemonServiceRuntimeError {
    DaemonServiceRuntimeError::UnsupportedOperation {
        operation: message.into(),
    }
}

#[derive(Deserialize)]
struct GarageHeadObject {
    #[serde(rename = "ContentLength")]
    content_length: u64,
    #[serde(rename = "Metadata", default)]
    metadata: GarageHeadMetadata,
}

#[derive(Default, Deserialize)]
struct GarageHeadMetadata {
    #[serde(rename = "dasobjectstore-sha256", alias = "dasobjectstore_sha256")]
    dasobjectstore_sha256: Option<String>,
}

#[derive(Default, Deserialize)]
struct GarageObjectListing {
    #[serde(rename = "Contents", default)]
    contents: Vec<GarageObjectEntry>,
}

#[derive(Deserialize)]
struct GarageObjectEntry {
    #[serde(rename = "Key")]
    key: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::ServiceCommandOutput;
    use dasobjectstore_core::ids::{ObjectId, PlacementId};
    use dasobjectstore_core::object_catalogue::{
        ObjectDigest, PortableLifecycleState, PortableObjectCatalogue, PortableObjectVersion,
        PortablePlacement, PortablePlacementLocation, PortableProtectionState, PortableProvenance,
        PORTABLE_OBJECT_CATALOGUE_SCHEMA_VERSION,
    };
    use dasobjectstore_core::protection::ProtectionPolicy;
    use rusqlite::Connection;
    use std::sync::Mutex;

    struct FixtureRunner {
        outputs: Mutex<Vec<String>>,
        commands: Mutex<Vec<Vec<String>>>,
    }

    impl ServiceCommandRunner for FixtureRunner {
        fn run(
            &self,
            _program: &str,
            args: &[String],
        ) -> Result<ServiceCommandOutput, DaemonServiceRuntimeError> {
            self.commands.lock().expect("commands").push(args.to_vec());
            let stdout = self.outputs.lock().expect("outputs").remove(0);
            Ok(ServiceCommandOutput { stdout })
        }
    }

    #[test]
    fn provider_delete_precedes_exact_catalogue_withdrawal() {
        let root = std::env::temp_dir().join(format!(
            "dasobjectstore-application-delete-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("root");
        let live = root.join("live.sqlite");
        seed_catalogue(&live);
        let runner = FixtureRunner {
            outputs: Mutex::new(vec![
                r#"{"Contents":[{"Key":"media/object-a"}]}"#.to_string(),
                format!(
                    r#"{{"ContentLength":42,"Metadata":{{"dasobjectstore-sha256":"{}"}}}}"#,
                    "a".repeat(64)
                ),
                "{}".to_string(),
                r#"{"Contents":[]}"#.to_string(),
            ]),
            commands: Mutex::new(Vec::new()),
        };
        let deletion = deletion();
        let outcome = GarageApplicationObjectDeletionAuthority::new(
            &runner,
            vec![("AWS_ACCESS_KEY_ID".into(), "redacted".into())],
            live.clone(),
        )
        .delete(&deletion)
        .expect("delete");
        assert_eq!(outcome, ApplicationObjectDeletionOutcome::Deleted);
        let commands = runner.commands.lock().expect("commands");
        assert_eq!(commands.len(), 4);
        assert_eq!(commands[0][1], "list-objects-v2");
        assert_eq!(commands[1][1], "head-object");
        assert_eq!(commands[2][1], "delete-object");
        assert_eq!(commands[3][1], "list-objects-v2");
        assert_eq!(
            Connection::open(&live)
                .expect("db")
                .query_row(
                    "SELECT COUNT(*) FROM profile_catalogue_objects",
                    [],
                    |row| row.get::<_, u64>(0)
                )
                .expect("count"),
            0
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn stale_evidence_stops_before_provider_mutation() {
        let root = std::env::temp_dir().join(format!(
            "dasobjectstore-application-delete-stale-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("root");
        let live = root.join("live.sqlite");
        seed_catalogue(&live);
        let runner = FixtureRunner {
            outputs: Mutex::new(Vec::new()),
            commands: Mutex::new(Vec::new()),
        };
        let mut deletion = deletion();
        deletion.expected_checksum = format!("sha256:{}", "b".repeat(64));
        GarageApplicationObjectDeletionAuthority::new(&runner, Vec::new(), live)
            .delete(&deletion)
            .expect_err("stale evidence");
        assert!(runner.commands.lock().expect("commands").is_empty());
        let _ = std::fs::remove_dir_all(root);
    }

    fn deletion() -> ApplicationObjectDeletion {
        ApplicationObjectDeletion {
            store_id: StoreId::new("pinakotheke-media").expect("store"),
            object_id: "media-1".to_string(),
            object_version: 1,
            object_key: "media/object-a".to_string(),
            expected_size_bytes: 42,
            expected_checksum: format!("sha256:{}", "a".repeat(64)),
            provider: "garage".to_string(),
            bucket: "pinakotheke-media".to_string(),
            endpoint_url: "http://127.0.0.1:3900".to_string(),
        }
    }

    fn seed_catalogue(path: &std::path::Path) {
        let connection = Connection::open(path).expect("db");
        connection
            .execute_batch(dasobjectstore_metadata::LIVE_SCHEMA_SQL)
            .expect("schema");
        connection
            .execute(
                "INSERT INTO pools VALUES ('pool-a', 'Clean', 'now', 'now')",
                [],
            )
            .expect("pool");
        connection
            .execute(
                "INSERT INTO stores VALUES (
                    'pinakotheke-media', 'pool-a', 'generated_data', '{}', 'now', 'now'
                )",
                [],
            )
            .expect("store");
        drop(connection);
        let store_id = StoreId::new("pinakotheke-media").expect("store");
        let digest = ObjectDigest {
            algorithm: "sha256".to_string(),
            value: "a".repeat(64),
        };
        let catalogue = PortableObjectCatalogue {
            schema_version: PORTABLE_OBJECT_CATALOGUE_SCHEMA_VERSION,
            store_id: store_id.clone(),
            objects: vec![PortableObjectVersion {
                object_id: ObjectId::new("media-1").expect("object"),
                version: 1,
                size_bytes: 42,
                checksum: digest.clone(),
                provenance: PortableProvenance {
                    source_kind: "remote_upload".to_string(),
                    locator: Some("media/object-a".to_string()),
                    revision: None,
                },
                lifecycle: PortableLifecycleState::HashVerified,
                protection_policy: ProtectionPolicy::ExternallyReplicated,
                protection_state: PortableProtectionState::Verified,
                placements: vec![PortablePlacement {
                    placement_id: PlacementId::new("provider-a").expect("placement"),
                    location: PortablePlacementLocation::Provider {
                        provider: "garage".to_string(),
                        object_key: "pinakotheke-media/media/object-a".to_string(),
                    },
                    checksum: digest,
                    verified_at_utc: None,
                }],
            }],
        };
        dasobjectstore_metadata::commit_profile_catalogue(
            path,
            dasobjectstore_metadata::ProfileCatalogueCommitRequest {
                transaction_id: "upload-1",
                profile_namespace: "provider:garage",
                store_id: &store_id,
                catalogue: &catalogue,
                source_retained: true,
                exact_snapshot: false,
                committed_at_utc: "2026-07-23T00:00:00Z",
            },
        )
        .expect("catalogue");
    }
}
