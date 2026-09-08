use super::*;
use crate::runtime::service::{DaemonServiceRuntimeError, ServiceCommandOutput};
use std::sync::Mutex;

struct Observer {
    output: String,
    calls: Mutex<Vec<(String, String)>>,
}
impl ServiceCommandRunner for Observer {
    fn run(
        &self,
        _: &str,
        _: &[String],
    ) -> Result<ServiceCommandOutput, DaemonServiceRuntimeError> {
        panic!("unbound role");
    }
    fn run_with_display_args_and_env(
        &self,
        _: &str,
        args: &[String],
        _: &[String],
        environment: &[(String, String)],
    ) -> Result<ServiceCommandOutput, DaemonServiceRuntimeError> {
        let key = environment
            .iter()
            .find(|(k, _)| k == "AWS_ACCESS_KEY_ID")
            .unwrap()
            .1
            .clone();
        let op = args
            .iter()
            .find(|v| ["head-object", "put-object", "get-object"].contains(&v.as_str()))
            .unwrap()
            .clone();
        self.calls.lock().unwrap().push((op.clone(), key.clone()));
        assert_eq!(
            key,
            if op == "put-object" {
                "write-only"
            } else {
                "read-only"
            }
        );
        Ok(ServiceCommandOutput {
            stdout: self.output.clone(),
        })
    }
}
fn metadata() -> serde_json::Value {
    serde_json::json!({"dasobjectstore-sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "dasobjectstore-object-lock-policy":CUSTODY_OBJECT_LOCK_POLICY_ID,
        "dasobjectstore-object-lock-shortening-forbidden":"true",
        "dasobjectstore-object-lock-delete-forbidden":"true",
        "dasobjectstore-object-lock-hold-authority":CUSTODY_OBJECT_LOCK_HOLD_AUTHORITY,
        "dasobjectstore-object-lock-retention-until-utc":"2036-09-05T12:00:00Z"})
}
fn writer(runner: &Observer) -> GarageCustodyS3Writer<'_, Observer> {
    GarageCustodyS3Writer::new_with_object_lock(
        runner,
        "http://127.0.0.1:3901",
        "fixture",
        "writer",
        vec![("AWS_ACCESS_KEY_ID".into(), "write-only".into())],
        "/not-used",
        CustodyObjectLockPolicyV1::required(),
        "2036-09-05T12:00:00Z",
    )
    .unwrap()
}
fn reader(runner: &Observer) -> GarageCustodyS3Reader<'_, Observer> {
    GarageCustodyS3Reader::new(
        runner,
        "http://127.0.0.1:3901",
        "fixture",
        "reader",
        vec![("AWS_ACCESS_KEY_ID".into(), "read-only".into())],
        "/not-used",
    )
}

#[test]
fn both_observations_use_reader_and_actual_selected_writer_policy() {
    let runner = Observer {
        output: serde_json::json!({"ContentLength":4,"Metadata":metadata()}).to_string(),
        calls: Mutex::new(Vec::new()),
    };
    let mut writer = writer(&runner);
    let reader = reader(&runner);
    let mut view = RoleSeparatedWriter {
        writer: &mut writer,
        reader: &reader,
    };
    assert_eq!(view.identity(), "writer");
    for _ in 0..2 {
        assert!(matches!(
            view.object_state("key").unwrap(),
            CustodyObjectState::Existing {
                content_length: 4,
                ..
            }
        ));
    }
    assert_eq!(
        *runner.calls.lock().unwrap(),
        vec![("head-object".into(), "read-only".into()); 2]
    );
    writer.retention_until_utc = "2037-09-05T12:00:00Z".into();
    let mut changed_selection = RoleSeparatedWriter {
        writer: &mut writer,
        reader: &reader,
    };
    assert!(changed_selection.object_state("key").is_err());
}

#[test]
fn reader_observation_keeps_every_sealed_policy_field_required() {
    let valid = metadata();
    for key in valid.as_object().unwrap().keys() {
        for replacement in [None, Some(serde_json::json!("foreign"))] {
            // Arbitrary nonempty digest remains the retainer's exact digest comparison;
            // this parser is responsible for presence and all policy fields.
            if key == "dasobjectstore-sha256" && replacement.is_some() {
                continue;
            }
            let mut changed = valid.clone();
            if let Some(value) = replacement {
                changed[key] = value;
            } else {
                changed.as_object_mut().unwrap().remove(key);
            }
            let runner = Observer {
                output: serde_json::json!({"ContentLength":4,"Metadata":changed}).to_string(),
                calls: Mutex::new(Vec::new()),
            };
            let mut writer = writer(&runner);
            let reader = reader(&runner);
            let mut view = RoleSeparatedWriter {
                writer: &mut writer,
                reader: &reader,
            };
            assert!(view.object_state("key").is_err());
        }
    }
}

#[test]
fn foreign_endpoint_bucket_and_same_identity_deny_before_io() {
    for field in ["endpoint", "bucket", "identity"] {
        let runner = Observer {
            output: String::new(),
            calls: Mutex::new(Vec::new()),
        };
        let mut writer = writer(&runner);
        let mut reader = reader(&runner);
        match field {
            "endpoint" => reader.endpoint.push_str("-foreign"),
            "bucket" => reader.bucket.push_str("-foreign"),
            _ => reader.identity = writer.identity.clone(),
        }
        let input = CustodyObjectInputV1 {
            bytes: b"data".to_vec(),
            object_type: "application/test".into(),
            retained_at_utc: "2026-09-08T16:00:00Z".into(),
        };
        assert!(retain_garage_custody_object_with_readback(
            "/not-an-approved-ledger",
            input,
            &mut writer,
            &reader
        )
        .is_err());
        assert!(runner.calls.lock().unwrap().is_empty());
    }
}
