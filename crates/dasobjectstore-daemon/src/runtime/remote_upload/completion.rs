//! Provider verification and atomic metadata publication for remote uploads.

use super::{
    RemoteUploadCompletionCommit, RemoteUploadCompletionCommitError,
    RemoteUploadCompletionMetadata, RemoteUploadCompletionRecord,
};
use crate::runtime::service::ServiceCommandRunner;
use dasobjectstore_core::ids::{ObjectId, PlacementId, StoreId};
use dasobjectstore_core::object_catalogue::{
    ObjectDigest, PortableLifecycleState, PortableObjectCatalogue, PortableObjectVersion,
    PortablePlacement, PortablePlacementLocation, PortableProtectionState, PortableProvenance,
    PORTABLE_OBJECT_CATALOGUE_SCHEMA_VERSION,
};
use dasobjectstore_core::protection::ProtectionPolicy;
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, serde::Serialize)]
pub struct RemoteUploadProviderCompletion {
    pub upload_id: String,
    pub provider: String,
    pub bucket: String,
    pub object_id: String,
    pub object_version: u64,
    pub object_key: String,
    pub expected_checksum: String,
    pub endpoint_url: String,
}

impl RemoteUploadProviderCompletion {
    pub fn transfer_metadata(&self, expected_size_bytes: u64) -> RemoteUploadCompletionMetadata {
        RemoteUploadCompletionMetadata {
            upload_id: self.upload_id.clone(),
            object_key: self.object_key.clone(),
            expected_size_bytes,
            expected_checksum: self.expected_checksum.clone(),
        }
    }
}

pub struct GarageRemoteUploadCompletionAuthority<'a> {
    runner: &'a dyn ServiceCommandRunner,
    environment: Vec<(String, String)>,
    live_sqlite_path: PathBuf,
    committed_at_utc: String,
    completion: RemoteUploadProviderCompletion,
}

impl<'a> GarageRemoteUploadCompletionAuthority<'a> {
    pub fn new(
        runner: &'a dyn ServiceCommandRunner,
        environment: Vec<(String, String)>,
        live_sqlite_path: PathBuf,
        committed_at_utc: String,
        completion: RemoteUploadProviderCompletion,
    ) -> Self {
        Self {
            runner,
            environment,
            live_sqlite_path,
            committed_at_utc,
            completion,
        }
    }

    pub fn verify_provider(
        &self,
        record: &RemoteUploadCompletionRecord,
    ) -> Result<(), RemoteUploadCompletionCommitError> {
        let metadata = record.metadata.as_ref().ok_or_else(|| {
            RemoteUploadCompletionCommitError::new("remote upload completion metadata is required")
        })?;
        let args = vec![
            "s3api".to_string(),
            "head-object".to_string(),
            "--bucket".to_string(),
            self.completion.bucket.clone(),
            "--key".to_string(),
            self.completion.object_key.clone(),
            "--endpoint-url".to_string(),
            self.completion.endpoint_url.clone(),
            "--output".to_string(),
            "json".to_string(),
        ];
        let output = self
            .runner
            .run_with_display_args_and_env("aws", &args, &args, &self.environment)
            .map_err(|error| {
                RemoteUploadCompletionCommitError::new(format!(
                    "provider completion verification failed: {error}"
                ))
            })?;
        let head: GarageHeadObject = serde_json::from_str(&output.stdout).map_err(|error| {
            RemoteUploadCompletionCommitError::new(format!(
                "provider completion response is invalid: {error}"
            ))
        })?;
        if head.content_length != metadata.expected_size_bytes {
            return Err(RemoteUploadCompletionCommitError::new(
                "provider completion size does not match admitted upload",
            ));
        }
        let expected = metadata.expected_checksum.trim_start_matches("sha256:");
        if !head
            .metadata
            .dasobjectstore_sha256
            .as_deref()
            .is_some_and(|actual| actual.eq_ignore_ascii_case(expected))
        {
            return Err(RemoteUploadCompletionCommitError::new(
                "provider completion checksum metadata does not match admitted upload",
            ));
        }
        Ok(())
    }

    pub fn commit_catalogue(
        &self,
        record: &RemoteUploadCompletionRecord,
    ) -> Result<(), RemoteUploadCompletionCommitError> {
        let metadata = record.metadata.as_ref().expect("verified metadata");
        let store_id = StoreId::new(record.object_store.clone()).map_err(|error| {
            RemoteUploadCompletionCommitError::new(format!("invalid ObjectStore id: {error}"))
        })?;
        let digest = ObjectDigest {
            algorithm: "sha256".to_string(),
            value: metadata.expected_checksum[7..].to_ascii_lowercase(),
        };
        let catalogue = PortableObjectCatalogue {
            schema_version: PORTABLE_OBJECT_CATALOGUE_SCHEMA_VERSION,
            store_id: store_id.clone(),
            objects: vec![PortableObjectVersion {
                object_id: ObjectId::new(self.completion.object_id.clone()).map_err(|error| {
                    RemoteUploadCompletionCommitError::new(format!("invalid object id: {error}"))
                })?,
                version: self.completion.object_version,
                size_bytes: metadata.expected_size_bytes,
                checksum: digest.clone(),
                provenance: PortableProvenance {
                    source_kind: "remote_upload".to_string(),
                    locator: Some(self.completion.object_key.clone()),
                    revision: None,
                },
                lifecycle: PortableLifecycleState::HashVerified,
                protection_policy: ProtectionPolicy::ExternallyReplicated,
                protection_state: PortableProtectionState::Verified,
                placements: vec![PortablePlacement {
                    placement_id: PlacementId::new(format!(
                        "provider-{}",
                        &metadata.expected_checksum[7..31]
                    ))
                    .map_err(|error| RemoteUploadCompletionCommitError::new(error.to_string()))?,
                    location: PortablePlacementLocation::Provider {
                        provider: self.completion.provider.clone(),
                        object_key: format!(
                            "{}/{}",
                            self.completion.bucket, self.completion.object_key
                        ),
                    },
                    checksum: digest,
                    // The enclosing catalogue transaction records commit time.
                    // Keep immutable placement identity independent of retry time.
                    verified_at_utc: None,
                }],
            }],
        };
        dasobjectstore_metadata::commit_profile_catalogue(
            &self.live_sqlite_path,
            dasobjectstore_metadata::ProfileCatalogueCommitRequest {
                transaction_id: &self.completion.upload_id,
                profile_namespace: "provider:garage",
                store_id: &store_id,
                catalogue: &catalogue,
                source_retained: true,
                exact_snapshot: false,
                committed_at_utc: &self.committed_at_utc,
            },
        )
        .map_err(|error| {
            RemoteUploadCompletionCommitError::new(format!(
                "remote upload catalogue commit failed: {error}"
            ))
        })?;
        Ok(())
    }
}

impl RemoteUploadCompletionCommit for GarageRemoteUploadCompletionAuthority<'_> {
    fn commit(
        &self,
        record: &RemoteUploadCompletionRecord,
    ) -> Result<(), RemoteUploadCompletionCommitError> {
        let metadata = record.metadata.as_ref().ok_or_else(|| {
            RemoteUploadCompletionCommitError::new("remote upload completion metadata is required")
        })?;
        if metadata.upload_id != self.completion.upload_id
            || metadata.object_key != self.completion.object_key
            || metadata.expected_checksum != self.completion.expected_checksum
        {
            return Err(RemoteUploadCompletionCommitError::new(
                "remote upload completion identity does not match the transfer",
            ));
        }
        self.verify_provider(record)?;
        self.commit_catalogue(record)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::{DaemonServiceRuntimeError, ServiceCommandOutput};
    use rusqlite::Connection;
    use std::sync::Mutex;

    struct HeadRunner {
        stdout: String,
        calls: Mutex<Vec<Vec<String>>>,
    }

    impl ServiceCommandRunner for HeadRunner {
        fn run(
            &self,
            _program: &str,
            args: &[String],
        ) -> Result<ServiceCommandOutput, DaemonServiceRuntimeError> {
            self.calls.lock().expect("calls lock").push(args.to_vec());
            Ok(ServiceCommandOutput {
                stdout: self.stdout.clone(),
            })
        }
    }

    fn completion() -> RemoteUploadProviderCompletion {
        RemoteUploadProviderCompletion {
            upload_id: "upload-42".to_string(),
            provider: "garage".to_string(),
            bucket: "store-a".to_string(),
            object_id: "object-42".to_string(),
            object_version: 1,
            object_key: "reads/object-42.fastq".to_string(),
            expected_checksum: format!("sha256:{}", "a".repeat(64)),
            endpoint_url: "http://127.0.0.1:3900".to_string(),
        }
    }

    fn record(completion: &RemoteUploadProviderCompletion) -> RemoteUploadCompletionRecord {
        RemoteUploadCompletionRecord {
            job_id: "job-42".to_string(),
            object_store: "store-a".to_string(),
            source_bytes: 42,
            metadata: Some(completion.transfer_metadata(42)),
        }
    }

    fn initialize_store(db: &std::path::Path) {
        let connection = Connection::open(db).expect("open metadata");
        connection
            .execute_batch(dasobjectstore_metadata::schema::LIVE_SCHEMA_SQL)
            .expect("initialize schema");
        connection
            .execute_batch(
                "INSERT INTO pools VALUES ('pool-a', 'active', 'now', 'now');
                 INSERT INTO stores VALUES ('store-a', 'pool-a', 'folder', '{}', 'now', 'now');",
            )
            .expect("initialize store");
    }

    #[test]
    fn garage_head_verification_precedes_atomic_catalogue_publication() {
        let root = std::env::temp_dir().join(format!(
            "dasobjectstore-provider-completion-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("test root");
        let db = root.join("live.sqlite");
        initialize_store(&db);
        let completion = completion();
        let runner = HeadRunner {
            stdout: format!(
                "{{\"ContentLength\":42,\"Metadata\":{{\"dasobjectstore-sha256\":\"{}\"}}}}",
                "a".repeat(64)
            ),
            calls: Mutex::new(Vec::new()),
        };
        let authority = GarageRemoteUploadCompletionAuthority::new(
            &runner,
            vec![("AWS_ACCESS_KEY_ID".to_string(), "redacted".to_string())],
            db.clone(),
            "2026-07-15T12:00:00Z".to_string(),
            completion.clone(),
        );

        authority.commit(&record(&completion)).expect("completion");
        authority
            .commit(&record(&completion))
            .expect("idempotent completion");

        let calls = runner.calls.lock().expect("calls lock");
        assert_eq!(calls.len(), 2);
        assert!(calls[0]
            .windows(2)
            .any(|args| args == ["--bucket", "store-a"]));
        let connection = Connection::open(db).expect("open metadata");
        let transactions: u64 = connection
            .query_row(
                "SELECT COUNT(*) FROM profile_catalogue_transactions",
                [],
                |row| row.get(0),
            )
            .expect("transaction count");
        let objects: u64 = connection
            .query_row(
                "SELECT COUNT(*) FROM profile_catalogue_objects",
                [],
                |row| row.get(0),
            )
            .expect("object count");
        assert_eq!((transactions, objects), (1, 1));
        drop(calls);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn checksum_mismatch_fails_before_catalogue_publication() {
        let root = std::env::temp_dir().join(format!(
            "dasobjectstore-provider-completion-mismatch-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("test root");
        let db = root.join("live.sqlite");
        let completion = completion();
        let runner = HeadRunner {
            stdout: format!(
                "{{\"ContentLength\":42,\"Metadata\":{{\"dasobjectstore-sha256\":\"{}\"}}}}",
                "b".repeat(64)
            ),
            calls: Mutex::new(Vec::new()),
        };
        let authority = GarageRemoteUploadCompletionAuthority::new(
            &runner,
            Vec::new(),
            db.clone(),
            "2026-07-15T12:00:00Z".to_string(),
            completion.clone(),
        );

        let error = authority
            .commit(&record(&completion))
            .expect_err("checksum mismatch rejected");
        assert!(error.to_string().contains("checksum metadata"));
        assert!(!db.exists());
        let _ = std::fs::remove_dir_all(root);
    }
}
