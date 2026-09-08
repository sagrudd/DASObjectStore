// Actual catalogue/ledger -> new snapshot verifier -> concrete FIFO child.
// Separate synthetic reader credential is fixture-only, not reopening authority.
#[test]
fn bounded_joined_real_ledger_and_concrete_reader() {
    use dasobjectstore_object_service::custody::{
        verify_custody_readback_existing, CustodyReadLimits,
    };
    use std::os::unix::fs::PermissionsExt;
    let parent = PathBuf::from(std::env::var_os("HOME").unwrap())
        .canonicalize()
        .unwrap();
    let f = BatchFixture::new_at(
        parent.join(format!(
            ".custody-joined-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )),
    );
    let receipts = f
        .controller()
        .retain_custody_inventory(&batch_inventory(), batch_inputs())
        .unwrap();
    let executable = f.root.join("synthetic-read");
    fs::write(&executable, "#!/usr/bin/python3\nimport sys\nwith open(sys.argv[-1], 'wb') as out: out.write(b'first')\n").unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    let reader = crate::runtime::GarageCustodyS3Reader::new(
        &crate::runtime::SystemServiceCommandRunner,
        "http://127.0.0.1:1",
        &receipts[0].bucket_name,
        &receipts[0].reader_identity,
        vec![
            ("AWS_ACCESS_KEY_ID".into(), "fixture".into()),
            ("AWS_SECRET_ACCESS_KEY".into(), "fixture".into()),
        ],
        &f.root,
    );
    let mut reader = reader.into_bounded(executable).unwrap();
    let before = fs::read(f.ledger()).unwrap();
    let verified = verify_custody_readback_existing(
        &f.ledger(),
        &receipts[0],
        &mut reader,
        CustodyReadLimits {
            maximum_bytes: 5,
            timeout: std::time::Duration::from_secs(3),
        },
    )
    .unwrap();
    assert_eq!(verified.bytes, b"first");
    assert_eq!(verified.receipt, receipts[0]);
    assert_eq!(fs::read(f.ledger()).unwrap(), before);
}
