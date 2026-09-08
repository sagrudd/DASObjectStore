//! Actual legacy signatures and durable first-attempt journal through HTTP ingress.
use super::*;
use crate::custody_reader::wire::http;
const NOW: &str = "2026-09-05T10:01:00Z";
fn http_request(body: &[u8]) -> Vec<u8> {
    let mut bytes = format!("POST /custody/v1/read-object HTTP/1.1\r\nHost: reader.test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).into_bytes();
    bytes.extend_from_slice(body);
    bytes
}
#[test]
fn genuine_signed_ingress_runs_only_after_durable_attempt_and_never_replays() {
    let (key, authority) = authority();
    let body = request(1, None);
    let raw = sign(body.clone(), &key, &authority);
    let wire = http_request(&raw);
    let root = std::env::temp_dir().join(format!("das-wire-journal-{}", Uuid::new_v4()));
    let journal = CustodyOffNucJournal::create(root.join("journal.sqlite")).unwrap();
    journal
        .issue_pre_read_request(&raw, &authority, NOW)
        .unwrap();
    let calls = std::cell::Cell::new(0);
    let value = journal.perform_pre_read(&body.request_id, NOW, |_| {
        let parsed = http::decode_request(&wire, "reader.test", &authority, NOW).unwrap();
        assert_eq!(parsed.raw_jcs, raw);
        assert_eq!(parsed.record.body, body);
        assert_eq!(parsed.consumed, wire.len());
        assert_eq!(
            Connection::open(&journal.path)
                .unwrap()
                .query_row::<u64, _, _>("SELECT COUNT(*) FROM first_attempts", [], |r| r.get(0))
                .unwrap(),
            1
        );
        calls.set(calls.get() + 1);
        Ok(())
    });
    assert!(value.is_ok());
    let reopened = CustodyOffNucJournal::open_existing(&journal.path).unwrap();
    assert!(reopened
        .perform_pre_read(&body.request_id, NOW, |_| {
            calls.set(calls.get() + 1);
            Ok(())
        })
        .is_err());
    assert_eq!(calls.get(), 1);
    fs::remove_dir_all(root).unwrap();
}
#[test]
fn legacy_scalar_signature_and_canonical_validation_is_not_replaced() {
    let (key, authority) = authority();
    let mut body = request(u64::MAX, None);
    body.purpose = "p".repeat(300); // Existing contract allows this; new-record limit does not apply.
    let raw = sign(body, &key, &authority);
    assert!(http::decode_request(&http_request(&raw), "reader.test", &authority, NOW).is_ok());
    let mut value: serde_json::Value = serde_json::from_slice(&raw).unwrap();
    value["body"]["target_id"] = serde_json::json!("substituted");
    assert!(http::decode_request(
        &http_request(&serde_jcs::to_vec(&value).unwrap()),
        "reader.test",
        &authority,
        NOW
    )
    .is_err());
    let mut newline = raw.clone();
    newline.push(b'\n');
    assert!(http::decode_request(&http_request(&newline), "reader.test", &authority, NOW).is_err());
    assert!(http::decode_request(
        &http_request(&raw),
        "reader.test",
        &authority,
        "2026-09-05T12:00:00Z"
    )
    .is_err());
    let (_, mut wrong) = super::authority();
    wrong.authority_id = "other".into();
    assert!(http::decode_request(&http_request(&raw), "reader.test", &wrong, NOW).is_err());
}
#[test]
fn hostile_http_framing_denies_before_effect_and_pipeline_has_no_second_dispatch() {
    let (key, authority) = authority();
    let raw = sign(request(1, None), &key, &authority);
    let good = String::from_utf8(http_request(&raw)).unwrap();
    for (extra_count, allowed) in [(59, true), (60, true), (61, false)] {
        let extras = (0..extra_count)
            .map(|n| format!("X-{n}: x\r\n"))
            .collect::<String>();
        let bounded = good.replacen("\r\n\r\n", &format!("\r\n{extras}\r\n"), 1);
        assert_eq!(
            http::decode_request(bounded.as_bytes(), "reader.test", &authority, NOW).is_ok(),
            allowed
        );
    }
    let mut bad = vec![];
    bad.push(good.replacen(
        &format!("Content-Length: {}", raw.len()),
        "Content-Length: 184467440737095516160",
        1,
    ));
    for path in [
        "/custody/v1/read-object?x",
        "/custody/v1/read-object/",
        "//custody/v1/read-object",
        "/custody/v1/read-object#x",
    ] {
        bad.push(good.replacen("/custody/v1/read-object", path, 1));
    }
    for header in [
        "Host: other",
        "HOST: reader.test",
        "Content-Length: 0",
        "Content-Type: application/json",
        "Connection: close",
        "Transfer-Encoding: chunked",
        "Content-Encoding: gzip",
        "Expect: 100-continue",
        "Upgrade: websocket",
        " folded: bad",
    ] {
        bad.push(good.replacen("\r\n\r\n", &format!("\r\n{header}\r\n\r\n"), 1));
    }
    bad.push(good.replacen("POST ", "GET ", 1));
    bad.push(good.replacen("HTTP/1.1", "HTTP/2", 1));
    bad.push(good.replacen("Connection: close\r\n", "", 1));
    bad.push(good.replacen(
        &format!("Content-Length: {}", raw.len()),
        "Content-Length: +1",
        1,
    ));
    bad.push(good.replacen(
        &format!("Content-Length: {}", raw.len()),
        "Content-Length: 1048577",
        1,
    ));
    bad.push(good.replacen(
        "\r\n\r\n",
        &format!("\r\nX: {}\r\n\r\n", "x".repeat(16384)),
        1,
    ));
    let extras = (0..64).map(|n| format!("X-{n}: x\r\n")).collect::<String>();
    bad.push(good.replacen("\r\n\r\n", &format!("\r\n{extras}\r\n"), 1));
    let mut effects = 0;
    for bytes in bad {
        if http::decode_request(bytes.as_bytes(), "reader.test", &authority, NOW).is_ok() {
            effects += 1;
        }
    }
    assert_eq!(effects, 0);
    let pipeline = good.clone() + &good;
    let first = http::decode_request(pipeline.as_bytes(), "reader.test", &authority, NOW).unwrap();
    assert_eq!(first.consumed, good.len()); // Contract closes after first; does not assert future EOF.
    for end in 0..good.len() {
        assert!(
            http::decode_request(&good.as_bytes()[..end], "reader.test", &authority, NOW).is_err()
        );
    }
}
