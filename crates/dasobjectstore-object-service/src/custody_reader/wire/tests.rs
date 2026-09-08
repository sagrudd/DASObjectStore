use super::*;
const READ: &[u8] =
    include_bytes!("../../../../../docs/adr/fixtures/0011-reader-wire/readback.jcs.json");
const RESULT: &[u8] =
    include_bytes!("../../../../../docs/adr/fixtures/0011-reader-wire/readback-result.jcs.json");
const SEAL: &[u8] =
    include_bytes!("../../../../../docs/adr/fixtures/0011-reader-wire/seal-request.jcs.json");
const HTTP: &[u8] =
    include_bytes!("../../../../../docs/adr/fixtures/0011-reader-wire/http-result.jcs.json");
fn unhex(value: &str) -> Vec<u8> {
    value
        .trim()
        .as_bytes()
        .chunks_exact(2)
        .map(|c| u8::from_str_radix(std::str::from_utf8(c).unwrap(), 16).unwrap())
        .collect()
}
#[test]
fn frozen_private_and_http_frames_match_exact_bytes() {
    let read = BootstrapReadV1::decode(READ).unwrap();
    let request = BootstrapRequest::Read(read.clone());
    let frame = unhex(include_str!(
        "../../../../../docs/adr/fixtures/0011-reader-wire/readback-frame.hex"
    ));
    assert_eq!(request.encode_frame().unwrap(), frame);
    assert_eq!(
        BootstrapRequest::decode_frame(&frame, true).unwrap(),
        request
    );
    let result = BootstrapReadResultV1::decode(RESULT).unwrap();
    let response = unhex(include_str!(
        "../../../../../docs/adr/fixtures/0011-reader-wire/readback-result-frame.hex"
    ));
    assert_eq!(encode_readback(&result, b"abc", 3).unwrap(), response);
    assert_eq!(decode_readback(&response, true, &read, 3).unwrap(), b"abc");
    let error = unhex(include_str!(
        "../../../../../docs/adr/fixtures/0011-reader-wire/error-frame.hex"
    ));
    assert_eq!(encode_denied().unwrap(), error);
    decode_denied(&error, true).unwrap();
    let http = ReaderResultV1::decode(HTTP).unwrap();
    let response = unhex(include_str!(
        "../../../../../docs/adr/fixtures/0011-reader-wire/http-response.hex"
    ));
    assert_eq!(http::encode_result(&http, b"abc", 3).unwrap(), response);
    assert_eq!(
        http::decode_result(&response, true, &http, &raw_sha256(b"abc"), 3).unwrap(),
        b"abc"
    );
    let seal_request = BootstrapSealV1::decode(SEAL).unwrap();
    let seal = ReaderSealV1::decode(include_bytes!(
        "../../../../../docs/adr/fixtures/0011-reader-wire/seal.jcs.json"
    ))
    .unwrap();
    assert_eq!(
        decode_seal(&encode_seal(&seal).unwrap(), true, &seal_request).unwrap(),
        seal
    );
}
#[test]
fn closed_records_reject_resealed_unknown_duplicate_scalar_and_noncanonical() {
    let decoders: [fn(&[u8]) -> bool; 4] = [
        |r| BootstrapReadV1::decode(r).is_ok(),
        |r| BootstrapSealV1::decode(r).is_ok(),
        |r| BootstrapReadResultV1::decode(r).is_ok(),
        |r| ReaderResultV1::decode(r).is_ok(),
    ];
    for (raw, accepts) in [READ, SEAL, RESULT, HTTP].into_iter().zip(decoders) {
        assert!(accepts(raw));
        let original: serde_json::Value = serde_json::from_slice(raw).unwrap();
        for field in original.as_object().unwrap().keys() {
            let mut v = original.clone();
            v[field] = serde_json::Value::Null;
            assert!(!accepts(&serde_jcs::to_vec(&v).unwrap()), "{field}");
            let duplicate = format!(
                "{{\"{field}\":null,{}",
                std::str::from_utf8(&raw[1..]).unwrap()
            );
            assert!(!accepts(duplicate.as_bytes()));
        }
        for (field, value) in [
            ("unknown", serde_json::json!(true)),
            ("schema", serde_json::json!("wrong")),
        ] {
            let mut v = original.clone();
            v[field] = value;
            assert!(!accepts(&serde_jcs::to_vec(&v).unwrap()));
        }
        let mut newline = raw.to_vec();
        newline.push(b'\n');
        assert!(!accepts(&newline));
        assert!(!accepts(b"\xff"));
        if original.get("content_length").is_some() {
            for bad in [0, 9_007_199_254_740_992, u64::MAX] {
                let mut v = original.clone();
                v["content_length"] = serde_json::json!(bad);
                assert!(!accepts(&serde_jcs::to_vec(&v).unwrap()));
            }
        }
    }
}

#[cfg(unix)]
#[test]
fn real_private_socket_fragmentation_half_close_and_missing_eof() {
    use std::io::{Read, Write};
    use std::net::Shutdown;
    use std::os::unix::net::UnixStream;
    let request = BootstrapRequest::Read(BootstrapReadV1::decode(READ).unwrap());
    for close in [true, false] {
        let (mut sender, mut receiver) = UnixStream::pair().unwrap();
        receiver
            .set_read_timeout(Some(std::time::Duration::from_millis(100)))
            .unwrap();
        let frame = request.encode_frame().unwrap();
        for part in frame.chunks(3) {
            sender.write_all(part).unwrap();
        }
        if close {
            sender.shutdown(Shutdown::Write).unwrap();
        }
        let mut bytes = Vec::new();
        let complete = Read::by_ref(&mut receiver)
            .take((FRAME_LIMIT + 5) as u64)
            .read_to_end(&mut bytes)
            .is_ok();
        assert_eq!(complete, close);
        assert_eq!(
            BootstrapRequest::decode_frame(&bytes, complete).is_ok(),
            close
        );
        if close {
            receiver.write_all(&encode_denied().unwrap()).unwrap();
            receiver.shutdown(Shutdown::Write).unwrap();
            let mut response = Vec::new();
            sender.read_to_end(&mut response).unwrap();
            decode_denied(&response, true).unwrap();
        }
    }
}
#[test]
fn truncated_extra_no_eof_and_wrong_selected_bytes_deny() {
    let read = BootstrapReadV1::decode(READ).unwrap();
    let request = BootstrapRequest::Read(read.clone()).encode_frame().unwrap();
    for end in 0..request.len() {
        assert!(BootstrapRequest::decode_frame(&request[..end], true).is_err());
    }
    assert!(BootstrapRequest::decode_frame(&request, false).is_err());
    let mut extra = request.clone();
    extra.push(0);
    assert!(BootstrapRequest::decode_frame(&extra, true).is_err());
    extra = request.clone();
    extra.extend_from_slice(&request);
    assert!(BootstrapRequest::decode_frame(&extra, true).is_err());
    let result = BootstrapReadResultV1::decode(RESULT).unwrap();
    let response = encode_readback(&result, b"abc", 3).unwrap();
    for end in 0..response.len() {
        assert!(decode_readback(&response[..end], true, &read, 3).is_err());
    }
    assert!(decode_readback(&response, false, &read, 3).is_err());
    assert!(decode_readback(&response, true, &read, 2).is_err());
    let mut changed = response.clone();
    *changed.last_mut().unwrap() ^= 1;
    assert!(decode_readback(&changed, true, &read, 3).is_err());
    let mut selected = read.clone();
    selected.content_length += 1;
    assert!(decode_readback(&response, true, &selected, 4).is_err());
    let metadata = ReaderResultV1::decode(HTTP).unwrap();
    let response = http::encode_result(&metadata, b"abc", 3).unwrap();
    for end in 0..response.len() {
        assert!(
            http::decode_result(&response[..end], true, &metadata, &raw_sha256(b"abc"), 3).is_err()
        );
    }
    let mut extra = response.clone();
    extra.push(0);
    assert!(http::decode_result(&extra, true, &metadata, &raw_sha256(b"abc"), 3).is_err());
    assert!(http::decode_result(&response, false, &metadata, &raw_sha256(b"abc"), 3).is_err());
    for field in [
        "request_sha256",
        "receipt_jcs_sha256",
        "configuration_sha256",
        "ledger_head_sha256",
    ] {
        let mut v: serde_json::Value = serde_json::from_slice(HTTP).unwrap();
        v[field] = serde_json::json!("f".repeat(64));
        let wrong = ReaderResultV1::decode(&serde_jcs::to_vec(&v).unwrap()).unwrap();
        assert!(http::decode_result(&response, true, &wrong, &raw_sha256(b"abc"), 3).is_err());
    }
}

#[test]
fn denial_records_and_response_header_corruption_are_closed() {
    for length in [4097_u32, u32::MAX] {
        assert!(BootstrapRequest::decode_frame(&length.to_be_bytes(), true).is_err());
    }
    let denial = http::encode_denied();
    http::decode_denied(&denial, true).unwrap();
    assert!(http::decode_denied(&denial, false).is_err());
    let mut extra = denial.clone();
    extra.push(0);
    assert!(http::decode_denied(&extra, true).is_err());
    for end in 0..denial.len() {
        assert!(http::decode_denied(&denial[..end], true).is_err());
    }
    let metadata = ReaderResultV1::decode(HTTP).unwrap();
    let good = http::encode_result(&metadata, b"abc", 3).unwrap();
    assert!(http::decode_result(&good, true, &metadata, &raw_sha256(b"abd"), 3).is_err());
    let mut corrupt = good.clone();
    *corrupt.last_mut().unwrap() ^= 1;
    assert!(http::decode_result(&corrupt, true, &metadata, &raw_sha256(b"abc"), 3).is_err());
    let header_end = good.windows(4).position(|p| p == b"\r\n\r\n").unwrap();
    for header in [
        "Content-Length: 1",
        "Transfer-Encoding: chunked",
        "Content-Encoding: gzip",
        "Connection: close",
        "Upgrade: websocket",
    ] {
        let mut bad = good[..header_end].to_vec();
        bad.extend_from_slice(format!("\r\n{header}").as_bytes());
        bad.extend_from_slice(&good[header_end..]);
        assert!(http::decode_result(&bad, true, &metadata, &raw_sha256(b"abc"), 3).is_err());
    }
    let mut private = encode_denied().unwrap();
    private.push(0);
    assert!(decode_denied(&private, true).is_err());
    assert!(decode_denied(&encode_denied().unwrap(), false).is_err());
    let mut selected = BootstrapReadV1::decode(READ).unwrap();
    let result = BootstrapReadResultV1::decode(RESULT).unwrap();
    let response = encode_readback(&result, b"abc", 3).unwrap();
    selected.object_key = format!("custody/sha256/{}", "f".repeat(64));
    assert!(decode_readback(&response, true, &selected, 3).is_err());
    selected.bootstrap_transaction_id = "00000000-0000-4000-8000-00000000000A".into();
    assert!(selected.encode().is_err());
    let mut result = result;
    result.content_sha256 = "f".repeat(64);
    assert!(result.encode().is_err());
}
