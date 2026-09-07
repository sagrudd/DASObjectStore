// Shared synthetic source-test inputs, not a deployment companion.
use super::*;
pub(super) fn hash(c: char) -> String {
    format!("sha256:{}", c.to_string().repeat(64))
}
pub(super) fn fixture() -> (Value, Value) {
    let target = json!({"machine_identity_sha256":hash('a'),"endpoint":"192.168.0.193","os":"linux","architecture":"amd64"});
    let stores: Vec<_> = ["builder-corpus", "nuc-delivery", "terminal-receipt"].iter().enumerate().map(|(n, purpose)| {
        let definition = CustodyStoreDefinitionV1 {
            store_id: StoreId::new(format!("custody-{n}")).unwrap(),
            bucket_name: format!("custody-bucket-{n}"),
            profile: CustodyStoreProfileV1 {
                schema: CUSTODY_OVERLAY_SCHEMA_V1.into(), profile: CUSTODY_PROFILE_V1.into(),
                assurance_class: CustodyAssuranceClass::LocalTrustedAdministratorOverlay,
                retention: CustodyRetentionPolicyV1::required(), target_id: hash('a'),
                retention_until_utc: "2027-09-07T12:00:00Z".into(), legal_hold: true,
                provisioner_credential_reference: format!("provisioner-ref-{n}"), provisioner_identity: format!("provisioner-{n}"),
                writer_credential_reference: format!("writer-ref-{n}"), writer_identity: format!("writer-{n}"),
                reader_credential_reference: format!("reader-ref-{n}"), reader_identity: format!("reader-{n}"),
            },
        };
        json!({"purpose":purpose,"namespace":format!("namespace-{n}"),"definition":definition,
            "hold_authority_identity":format!("hold-{n}"),"inventory":[{"content_sha256":hash('b'),"size_bytes":12}]})
    }).collect();
    let m = json!({
        "schema":MANIFEST_SCHEMA,"transaction_id":"r239-synthetic","purpose":"r239-custody-bootstrap-plan",
        "issued_at_utc":"2026-09-07T12:00:00Z","expires_at_utc":"2026-09-07T13:00:00Z","maximum_runtime_seconds":600,
        "target":target,"source":{"repository":"https://github.com/sagrudd/DASObjectStore","revision":"a".repeat(40),
            "cargo_lock_sha256":hash('b'),"source_tree_sha256":hash('c'),"executable_sha256":hash('d'),
            "qualification_sha256":hash('e'),"image_digest":hash('f'),"compiler_identity":"rustc-1.85.0",
            "locked_commands":[{"operation":"build","arguments":["build","--locked"],"working_directory":"/qualified/source","output_sha256":hash('a'),"exit_status":0},
                {"operation":"test","arguments":["test","--locked"],"working_directory":"/qualified/source","output_sha256":hash('b'),"exit_status":0}]},
        "isolation":{"service_identity":"custody-runtime","unit":"custody-unit","namespace":"custody-net",
            "garage_project":"custody-project","garage_service":"custody-service","private_endpoint":"http://127.0.0.1:3901",
            "custody_endpoint":"https://192.168.0.193:4901","paths":{"data":"/var/lib/custody/data","metadata":"/var/lib/custody/metadata","catalog":"/var/lib/custody/catalog","ledger":"/var/lib/custody/ledger","configuration":"/etc/custody/config"},
            "marker_path":"/var/lib/external-marker/claim","ordinary_paths":["/var/lib/dasobjectstore","/srv/dasobjectstore"],
            "ordinary_endpoints":["http://127.0.0.1:3900"],"old_client_exclusion_evidence_sha256":hash('b'),"configuration_sha256":hash('c')},
        "stores":stores,"verifier":{"machine_identity_sha256":hash('b'),"executable_sha256":hash('c'),"authority_sha256":hash('d'),
            "journal_identity_sha256":hash('e'),"administration_exclusion_evidence_sha256":hash('f'),"consumer_identity":"expedition-consumer",
            "endpoint":"https://192.168.0.193:4901","tls_identity_sha256":hash('a'),"provenance_sha256":hash('b'),"journal_backup_exclusion_evidence_sha256":hash('c')},
        "reader_continuation":{"service_identity":"custody-readonly","configuration_sha256":hash('a'),"available_until_utc":"2027-09-07T12:00:00Z","read_only":true},
        "marker_exclusion_evidence_sha256":hash('a')
    });
    let o = json!({"schema":OBSERVATION_SCHEMA,"manifest_sha256":"rebound-by-test","observed_at_utc":"2026-09-07T12:01:00Z","target":target,
        "existing_paths":[],"existing_buckets":[],"existing_store_ids":[],"existing_namespaces":[],"symlink_paths":[],
        "ordinary_plane_excluded":true,"marker_excluded_from_backup":true,"verifier_independent":true,"reader_continuation_available":true});
    (m, o)
}
pub(super) fn bytes(m: &Value, o: &Value) -> (Vec<u8>, Vec<u8>) {
    let m = serde_json::to_vec(m).unwrap();
    let mut o = o.clone();
    o["manifest_sha256"] = json!(digest(&m));
    (m, serde_json::to_vec(&o).unwrap())
}
