const BUILD: &str = include_str!("../../../packaging/debian/build-plugin-process-deb.sh");
const ATTEMPT: &str =
    include_str!("../../../packaging/debian/run-plugin-process-package-attempt.sh");
const VALIDATE: &str = include_str!("../../../packaging/debian/validate-plugin-process-package.sh");
const PROVENANCE: &str = include_str!("../../../packaging/plugin-process-package-provenance.sh");
const PREPARE_WEB_DIST: &str = include_str!("../../../packaging/web/prepare-web-dist.sh");
const PROMOTE: &str = include_str!("../../../packaging/debian/promote-plugin-process-package.sh");

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

#[test]
fn provenance_stage_is_source_owned_and_fails_closed_before_package_work() {
    for required in [
        "--stage-closure-provenance-inputs ABSOLUTE_IMMUTABLE_INPUT_ROOT",
        "produce_closure_provenance_inputs",
        "requires immutable non-symlink inputs",
        "rejects a dirty or source-identity-mismatched archive",
        "rejects an unpinned Kanon validator",
        "binary_sha256",
        "validator_sha=$(canonical_sha256_file \"$validator_receipt\" binary_sha256)",
        "canonical_sha256_digest",
        "requires the sha256 algorithm",
        "requires exactly 64 lower-case SHA-256 hex characters",
        "toolchain_mode_from_receipt",
        "native-tool-bundle@sha256:$inventory_sha",
        "rejects mixed native and container modes",
        "requires immutable sealed identity inputs while leaving fresh output roots writable",
        "component-candidate-input validate",
        "require_valid_component_candidate_report",
        "requires jq for Kanon validator report validation",
        ".stage == \"component-candidate-input\"",
        "(.issues | type == \"array\")",
        "Kanon validator rejected emitted inputs",
        "compiled-dependency-witness.json",
        "component-candidate-input.validation.json",
    ] {
        assert!(
            ATTEMPT.contains(required),
            "missing provenance-stage contract: {required}"
        );
    }
    let producer = &ATTEMPT[ATTEMPT
        .find("produce_closure_provenance_inputs")
        .expect("producer")
        ..ATTEMPT
            .find("reject_symlink_ancestry \"$sealed_root\"")
            .expect("producer dispatch")];
    for forbidden in ["cargo build", "build-plugin-process-deb.sh"] {
        assert!(
            !producer.contains(forbidden),
            "provenance stage must not perform {forbidden}"
        );
    }
}

#[test]
fn provenance_stage_emits_validator_accepted_inputs_and_rejects_expected_tuple_mismatch() {
    use std::os::unix::fs::PermissionsExt;

    let temp = std::env::temp_dir().join(format!(
        "dasobjectstore-provenance-stage-{}",
        std::process::id()
    ));
    let sealed = staged_fixture(&temp);
    // Start the complete handoff with no generated provenance or native-stage
    // outputs.  The reviewed tool bundle is an independently immutable input;
    // it is deliberately not copied into the closure until the source-owned
    // witness and normal provenance stages have emitted their own documents.
    let tool_input = immutable_tool_input_fixture(&temp, &sealed, true);
    for path in ["toolchain", "network-denied-bin", "staging-home"] {
        fs::remove_dir_all(sealed.join(path)).expect("clear pre-stage tool output fixture");
    }
    for name in [
        "component-candidate-input.toml",
        "source-tree",
        "compiled-dependency-witness.json",
        "dependency-witness-receipt.toml",
        "component-candidate-input.validation.json",
        "package-recipe.json",
        "provenance-tuple.toml",
    ] {
        let path = sealed.join("inputs").join(name);
        if path.exists() {
            fs::remove_file(path).expect("clear producer output fixture");
        }
    }
    fs::remove_file(sealed.join("f05-inputs.sha256")).expect("clear producer manifest fixture");
    Command::new("chmod")
        .args(["-R", "a-w"])
        .arg(sealed.join("source"))
        .status()
        .expect("seal source inputs before provenance staging");
    write_f05_manifest(&sealed);
    let input = temp.join("provenance-inputs");
    fs::create_dir(&input).expect("create provenance inputs");
    let archive = input.join("source-archive.tar");
    write(&archive, "immutable source archive fixture\n");
    let revision = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    write(
        input.join("source-identity.toml"),
        &format!(
            "source_revision = \"{revision}\"\nworkspace_version = \"0.186.17\"\ngit_tree = \"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\"\nsource_archive_sha256 = \"sha256:{}\"\nsource_content_sha256 = \"sha256:{}\"\n",
            sha256(&archive),
            tree_sha256(&sealed.join("source")),
        ),
    );
    write(
        input.join("registry.toml"),
        "[products.dasobjectstore]\nbinaries = [\"dasobjectstore\"]\n",
    );
    write(
        input.join("package-recipe.json"),
        "{\"package\":\"fixture\"}\n",
    );
    let validator = input.join("kanon-component-candidate-input");
    executable(&validator, "{\"valid\":true}");
    write(
        &validator,
        "#!/bin/sh\nprintf '%s\\n' '{\n  \"valid\": true,\n  \"stage\": \"component-candidate-input\",\n  \"issues\": []\n}'\n",
    );
    fs::set_permissions(&validator, fs::Permissions::from_mode(0o755))
        .expect("make pinned validator executable");
    write(
        input.join("kanon-component-candidate-input.receipt"),
        &format!(
            "revision = \"4a7b1a16c9864c3eb0b66b60b4bffbe752052cc7\"\nbinary_sha256 = \"{}\"\n",
            sha256(&validator)
        ),
    );
    write(
        input.join("expected-tuple.toml"),
        &format!(
            "source_revision = \"{revision}\"\nworkspace_version = \"0.186.17\"\nsource_git_tree = \"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\"\nsource_archive_sha256 = \"sha256:{}\"\nsource_content_sha256 = \"sha256:{}\"\nrecipe_sha256 = \"sha256:{}\"\nvalidator_revision = \"4a7b1a16c9864c3eb0b66b60b4bffbe752052cc7\"\nvalidator_binary_sha256 = \"sha256:{}\"\ntoolchain_kind = \"native-tool-bundle\"\ntool_inventory_sha256 = \"{}\"\n",
            sha256(&archive),
            tree_sha256(&sealed.join("source")),
            sha256(&input.join("package-recipe.json")),
            sha256(&validator),
            sha256(&tool_input.join("tool-input-inventory.txt")),
        ),
    );
    Command::new("chmod")
        .args(["-R", "a-w"])
        .arg(&input)
        .status()
        .expect("seal provenance inputs");
    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../packaging/debian/run-plugin-process-package-attempt.sh");
    let run = |stage: &Path, inputs: &Path| {
        Command::new("bash")
            .arg(&script)
            .args(["--sealed-root"])
            .arg(stage)
            .args(["--stage-closure-provenance-inputs"])
            .arg(inputs)
            .output()
            .expect("run provenance stage")
    };
    let unseeded_stage = |name: &str| {
        let stage = temp.join(name);
        copy_tree(&sealed, &stage);
        Command::new("chmod")
            .args(["-R", "u+w"])
            .arg(&stage)
            .status()
            .expect("make denial stage writable");
        for name in [
            "component-candidate-input.toml",
            "component-candidate-input.validation.json",
            "package-recipe.json",
            "provenance-tuple.toml",
        ] {
            let path = stage.join("inputs").join(name);
            if path.exists() {
                fs::remove_file(path).expect("clear unseeded producer output");
            }
        }
        fs::remove_file(stage.join("f05-inputs.sha256")).expect("clear unseeded producer manifest");
        Command::new("chmod")
            .args(["-R", "a-w"])
            .arg(stage.join("source"))
            .status()
            .expect("seal unseeded source inputs before provenance staging");
        Command::new("chmod")
            .args(["a-w"])
            .args([
                stage
                    .join("inputs/source-tree")
                    .to_str()
                    .expect("source-tree path"),
                stage
                    .join("inputs/compiled-dependency-witness.json")
                    .to_str()
                    .expect("witness path"),
                stage
                    .join("inputs/dependency-witness-receipt.toml")
                    .to_str()
                    .expect("witness receipt path"),
            ])
            .status()
            .expect("seal copied dependency-witness inputs before tuple denial");
        write_f05_manifest(&stage);
        stage
    };
    let witness = Command::new("bash")
        .arg(&script)
        .args(["--sealed-root"])
        .arg(&sealed)
        .args(["--stage-closure-provenance-inputs"])
        .arg(&input)
        .arg("--dependency-witness-only")
        .output()
        .expect("run source-owned dependency-witness stage");
    assert!(
        witness.status.success(),
        "dependency-witness stage stderr: {}",
        String::from_utf8_lossy(&witness.stderr)
    );

    // Jenkins #348 emits canonical tuple digests while the admitted Kanon
    // receipt uses its schema's bare `binary_sha256` field.  The consumer must
    // normalize that one trusted legacy spelling, then reject every malformed,
    // unsupported, or byte-mismatched tuple digest before it emits a candidate.
    for (name, replacement, expected_error) in [
        (
            "unsupported-recipe-algorithm",
            ("recipe_sha256 = \"sha256:", "recipe_sha256 = \"sha512:"),
            "recipe_sha256 requires the sha256 algorithm",
        ),
        (
            "duplicate-validator-prefix",
            (
                "validator_binary_sha256 = \"sha256:",
                "validator_binary_sha256 = \"sha256:sha256:",
            ),
            "validator_binary_sha256 requires exactly 64 lower-case SHA-256 hex characters",
        ),
        (
            "tampered-recipe-digest",
            (
                "recipe_sha256 = \"sha256:",
                "recipe_sha256 = \"sha256:ffffffff",
            ),
            "rejects an expected tuple not bound to the generated dependency witness",
        ),
    ] {
        let variant = temp.join(format!("{name}-inputs"));
        copy_tree(&input, &variant);
        Command::new("chmod")
            .args(["-R", "u+w"])
            .arg(&variant)
            .status()
            .expect("make tuple variant writable");
        let tuple = variant.join("expected-tuple.toml");
        let original = fs::read_to_string(&tuple).expect("read canonical expected tuple");
        let mutated = if name == "tampered-recipe-digest" {
            original
                .lines()
                .map(|line| {
                    if line.starts_with("recipe_sha256 = ") {
                        "recipe_sha256 = \"sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff\""
                    } else {
                        line
                    }
                })
                .collect::<Vec<_>>()
                .join("\n")
                + "\n"
        } else {
            original.replacen(replacement.0, replacement.1, 1)
        };
        write(&tuple, &mutated);
        Command::new("chmod")
            .args(["-R", "a-w"])
            .arg(&variant)
            .status()
            .expect("reseal tuple variant");
        let stage = unseeded_stage(name);
        let manifest_before = sha256(&stage.join("f05-inputs.sha256"));
        let denied = run(&stage, &variant);
        assert!(
            !denied.status.success()
                && String::from_utf8_lossy(&denied.stderr).contains(expected_error),
            "{name} must deny through the canonical digest boundary: {}",
            String::from_utf8_lossy(&denied.stderr)
        );
        assert!(
            !stage.join("inputs/component-candidate-input.toml").exists()
                && !stage
                    .join("inputs/component-candidate-input.validation.json")
                    .exists()
                && !stage.join("inputs/package-recipe.json").exists()
                && !stage.join("inputs/provenance-tuple.toml").exists()
                && sha256(&stage.join("f05-inputs.sha256")) == manifest_before,
            "{name} must not emit candidate outputs or rewrite its sealed manifest"
        );
    }

    // The pinned validator's JSON is intentionally pretty-printed in the
    // primary positive path.  Exercise the same runner boundary with compact
    // JSON and with semantically invalid or misleading reports; a substring
    // scan must never promote those reports to an accepted candidate.
    for (name, report, accepted) in [
        (
            "compact-validator-report",
            "{\"valid\":true,\"stage\":\"component-candidate-input\",\"issues\":[]}",
            true,
        ),
        (
            "false-validator-report",
            "{\"valid\":false,\"stage\":\"component-candidate-input\",\"issues\":[]}",
            false,
        ),
        (
            "string-validator-report",
            "{\"valid\":\"true\",\"stage\":\"component-candidate-input\",\"issues\":[]}",
            false,
        ),
        (
            "nested-validator-report",
            "{\"valid\":false,\"stage\":\"component-candidate-input\",\"issues\":[\"{\\\"valid\\\":true}\"]}",
            false,
        ),
        ("malformed-validator-report", "{\"valid\":true", false),
    ] {
        let variant = temp.join(format!("{name}-inputs"));
        copy_tree(&input, &variant);
        Command::new("chmod")
            .args(["-R", "u+w"])
            .arg(&variant)
            .status()
            .expect("make validator-report variant writable");
        let variant_validator = variant.join("kanon-component-candidate-input");
        write(
            &variant_validator,
            &format!("#!/bin/sh\nprintf '%s\\n' '{report}'\n"),
        );
        fs::set_permissions(&variant_validator, fs::Permissions::from_mode(0o755))
            .expect("make report validator executable");
        let receipt = variant.join("kanon-component-candidate-input.receipt");
        write(
            &receipt,
            &format!(
                "revision = \"4a7b1a16c9864c3eb0b66b60b4bffbe752052cc7\"\nbinary_sha256 = \"{}\"\n",
                sha256(&variant_validator)
            ),
        );
        let tuple = variant.join("expected-tuple.toml");
        let original = fs::read_to_string(&tuple).expect("read validator-report tuple");
        write(
            &tuple,
            &original.replacen(
                &format!(
                    "validator_binary_sha256 = \"sha256:{}\"",
                    sha256(&validator)
                ),
                &format!(
                    "validator_binary_sha256 = \"sha256:{}\"",
                    sha256(&variant_validator)
                ),
                1,
            ),
        );
        Command::new("chmod")
            .args(["-R", "a-w"])
            .arg(&variant)
            .status()
            .expect("reseal validator-report variant");
        let stage = unseeded_stage(name);
        let manifest_before = sha256(&stage.join("f05-inputs.sha256"));
        let result = run(&stage, &variant);
        assert_eq!(
            result.status.success(),
            accepted,
            "{name} produced unexpected stderr: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        if accepted {
            assert!(
                stage
                    .join("inputs/component-candidate-input.toml")
                    .is_file()
            );
        } else {
            assert!(
                String::from_utf8_lossy(&result.stderr)
                    .contains("Kanon validator rejected emitted inputs")
                    && !stage.join("inputs/component-candidate-input.toml").exists()
                    && !stage.join("inputs/package-recipe.json").exists()
                    && !stage.join("inputs/provenance-tuple.toml").exists()
                    && !stage
                        .join("inputs/component-candidate-input.validation.json")
                        .exists()
                    && sha256(&stage.join("f05-inputs.sha256")) == manifest_before,
                "{name} must fail before candidate output or manifest mutation"
            );
        }
    }

    // Each checked move is a normal-process failure boundary.  The producer
    // must roll back only its new outputs, retaining the source-owned witness
    // and the pre-producer manifest so a consumer cannot accept a partial set.
    for publication in ["candidate", "recipe", "tuple", "report"] {
        let stage = unseeded_stage(&format!("publish-{publication}-failure"));
        let manifest_before = sha256(&stage.join("f05-inputs.sha256"));
        let failed = Command::new("bash")
            .arg(&script)
            .args(["--sealed-root"])
            .arg(&stage)
            .args(["--stage-closure-provenance-inputs"])
            .arg(&input)
            .env(
                "DASOBJECTSTORE_F05_FAIL_PROVENANCE_PUBLISH_MOVE",
                publication,
            )
            .output()
            .expect("inject provenance publication failure");
        assert!(
            !failed.status.success()
                && String::from_utf8_lossy(&failed.stderr).contains("could not publish validated"),
            "{publication} publication fault must fail closed: {}",
            String::from_utf8_lossy(&failed.stderr)
        );
        assert!(
            !stage.join("inputs/component-candidate-input.toml").exists()
                && !stage.join("inputs/package-recipe.json").exists()
                && !stage.join("inputs/provenance-tuple.toml").exists()
                && !stage
                    .join("inputs/component-candidate-input.validation.json")
                    .exists()
                && sha256(&stage.join("f05-inputs.sha256")) == manifest_before,
            "{publication} publication fault must leave no generated partial state"
        );
    }

    // The manifest is the completion boundary consumed by later stages.  A
    // failure or ordinary interrupt after its replacement must restore the
    // exact pre-producer bytes and remove every generated document.
    for fault in ["fault", "signal"] {
        let stage = unseeded_stage(&format!("post-manifest-{fault}-failure"));
        let manifest_before =
            fs::read(stage.join("f05-inputs.sha256")).expect("read pre-producer manifest bytes");
        let failed = Command::new("bash")
            .arg(&script)
            .args(["--sealed-root"])
            .arg(&stage)
            .args(["--stage-closure-provenance-inputs"])
            .arg(&input)
            .env("DASOBJECTSTORE_F05_FAIL_PROVENANCE_POST_MANIFEST", fault)
            .output()
            .expect("inject post-manifest publication failure");
        assert!(
            !failed.status.success(),
            "{fault} post-manifest fault must fail closed"
        );
        assert!(
            !stage.join("inputs/component-candidate-input.toml").exists()
                && !stage.join("inputs/package-recipe.json").exists()
                && !stage.join("inputs/provenance-tuple.toml").exists()
                && !stage
                    .join("inputs/component-candidate-input.validation.json")
                    .exists()
                && fs::read(stage.join("f05-inputs.sha256"))
                    .expect("read restored pre-producer manifest")
                    == manifest_before,
            "{fault} post-manifest fault must restore the original manifest and leave no generated state"
        );
    }
    let accepted = run(&sealed, &input);
    assert!(
        accepted.status.success(),
        "producer stderr: {}",
        String::from_utf8_lossy(&accepted.stderr)
    );
    assert!(sealed
        .join("inputs/component-candidate-input.toml")
        .is_file());
    assert!(sealed
        .join("inputs/compiled-dependency-witness.json")
        .is_file());
    assert!(sealed
        .join("inputs/component-candidate-input.validation.json")
        .is_file());
    assert_eq!(
        fs::read(sealed.join("inputs/package-recipe.json")).expect("read staged package recipe"),
        fs::read(input.join("package-recipe.json")).expect("read validated package recipe"),
        "normal provenance must publish the exact validated package recipe into the sealed closure"
    );
    assert!(
        verify_f05_manifest(&sealed),
        "the closure manifest must bind the staged package recipe"
    );
    let candidate = fs::read_to_string(sealed.join("inputs/component-candidate-input.toml"))
        .expect("read emitted candidate");
    assert!(
        candidate.contains("toolchain_image = \"native-tool-bundle@sha256:")
            && candidate.contains("toolchain_image_sha256 = \"sha256:"),
        "native expected tuples must map only their explicit inventory digest into Kanon's immutable tool identity slots"
    );
    assert!(
        sealed.join("inputs/provenance-tuple.toml").is_file(),
        "the emitted candidate must retain the independently supplied selected-mode tuple"
    );
    let witness = sealed.join("inputs/compiled-dependency-witness.json");
    assert!(
        witness.is_file()
            && sealed
                .join("inputs/dependency-witness-receipt.toml")
                .is_file()
            && fs::metadata(&witness)
                .expect("read generated witness mode")
                .permissions()
                .readonly(),
        "the provenance stage must generate and freeze an archive/lock-bound witness before candidate emission"
    );

    // This is intentionally one unseeded, real script sequence rather than
    // independent producer and native-tool fixtures: witness-only -> normal
    // provenance -> native toolchain stage -> normal preflight-only.
    seal_tool_stage_identity_inputs(&sealed);
    let native_stage = Command::new("bash")
        .arg(&script)
        .args(["--sealed-root"])
        .arg(&sealed)
        .args(["--stage-closure-toolchain-inputs"])
        .arg(&tool_input)
        .output()
        .expect("run native toolchain stage after provenance");
    assert!(
        native_stage.status.success(),
        "native stage after provenance failed: {}",
        String::from_utf8_lossy(&native_stage.stderr)
    );
    assert!(
        String::from_utf8_lossy(&native_stage.stdout)
            .contains("closure_stage_toolchain_inputs=PASS"),
        "native stage must retain its receipt after normal provenance"
    );
    let cache = temp.join("sequence-cache");
    let attempt = temp.join("sequence-attempt");
    let diagnostic = temp.join("sequence-diagnostic");
    fs::create_dir(&cache).expect("create sequence stage cache");
    fs::create_dir(&attempt).expect("create sequence attempt root");
    fs::create_dir(&diagnostic).expect("create sequence diagnostic root");
    let preflight = Command::new("bash")
        .arg(&script)
        .args(["--sealed-root"])
        .arg(&sealed)
        .args(["--attempt-root"])
        .arg(&attempt)
        .args(["--diagnostic-root"])
        .arg(&diagnostic)
        .args(["--stage-cache-root"])
        .arg(&cache)
        .arg("--preflight-only")
        .output()
        .expect("run normal preflight after native stage");
    assert!(
        preflight.status.success(),
        "unseeded sequence preflight failed: {}",
        String::from_utf8_lossy(&preflight.stderr)
    );
    assert!(
        fs::read_to_string(diagnostic.join("preflight.log"))
            .expect("read unseeded preflight log")
            .contains("preflight_only=PASS"),
        "unseeded sequence must retain the final normal preflight marker"
    );
    assert!(
        !attempt.join("target").exists() && !attempt.join("output").exists(),
        "the source-only sequence may not compile or emit a package"
    );
    let mismatch = temp.join("mismatch-inputs");
    copy_tree(&input, &mismatch);
    Command::new("chmod")
        .args(["-R", "u+w"])
        .arg(&mismatch)
        .status()
        .expect("unseal mismatch");
    let identity =
        fs::read_to_string(mismatch.join("source-identity.toml")).expect("read identity");
    write(
        mismatch.join("source-identity.toml"),
        &identity.replace(revision, "cccccccccccccccccccccccccccccccccccccccc"),
    );
    Command::new("chmod")
        .args(["-R", "a-w"])
        .arg(&mismatch)
        .status()
        .expect("reseal mismatch");
    let fresh = staged_fixture(&temp.join("mismatch-stage"));
    for name in [
        "component-candidate-input.toml",
        "source-tree",
        "compiled-dependency-witness.json",
        "package-recipe.json",
    ] {
        fs::remove_file(fresh.join("inputs").join(name)).expect("clear mismatch output");
    }
    fs::remove_file(fresh.join("f05-inputs.sha256")).expect("clear mismatch manifest");
    Command::new("chmod")
        .args(["-R", "a-w"])
        .arg(fresh.join("source"))
        .status()
        .expect("seal mismatch source inputs before provenance staging");
    write_f05_manifest(&fresh);
    let pre_producer_manifest = sha256(&fresh.join("f05-inputs.sha256"));
    let denied = Command::new("bash")
        .arg(&script)
        .args(["--sealed-root"])
        .arg(&fresh)
        .args(["--stage-closure-provenance-inputs"])
        .arg(&mismatch)
        .arg("--dependency-witness-only")
        .output()
        .expect("run denied dependency-witness stage");
    assert!(
        !denied.status.success()
            && String::from_utf8_lossy(&denied.stderr)
                .contains("rejects a dirty or source-identity-mismatched archive")
    );
    assert!(
        !fresh.join("inputs/component-candidate-input.toml").exists()
            && !fresh
                .join("inputs/compiled-dependency-witness.json")
                .exists()
            && !fresh
                .join("inputs/component-candidate-input.validation.json")
                .exists()
            && sha256(&fresh.join("f05-inputs.sha256")) == pre_producer_manifest,
        "a rejected tuple must not emit outputs or rewrite its pre-producer manifest"
    );

    // The real archive-only handoff must stop at the same causal boundary: a
    // source archive without its independently staged physical vendor closure
    // cannot manufacture the witness, candidate, or a later preflight input.
    let missing_vendor = staged_fixture(&temp.join("missing-vendor-stage"));
    for name in [
        "component-candidate-input.toml",
        "source-tree",
        "compiled-dependency-witness.json",
        "dependency-witness-receipt.toml",
        "component-candidate-input.validation.json",
        "package-recipe.json",
        "provenance-tuple.toml",
    ] {
        let path = missing_vendor.join("inputs").join(name);
        if path.exists() {
            fs::remove_file(path).expect("clear missing-vendor generated fixture");
        }
    }
    fs::remove_file(missing_vendor.join("f05-inputs.sha256"))
        .expect("clear missing-vendor manifest");
    fs::remove_dir_all(missing_vendor.join("source/vendor"))
        .expect("remove physical dependency closure");
    let missing_input = temp.join("missing-vendor-inputs");
    copy_tree(&input, &missing_input);
    Command::new("chmod")
        .args(["-R", "u+w"])
        .arg(&missing_input)
        .status()
        .expect("make missing-vendor identity input writable");
    let missing_identity = fs::read_to_string(missing_input.join("source-identity.toml"))
        .expect("read missing-vendor identity");
    write(
        missing_input.join("source-identity.toml"),
        &missing_identity.replace(
            &format!(
                "source_content_sha256 = \"sha256:{}\"",
                tree_sha256(&sealed.join("source"))
            ),
            &format!(
                "source_content_sha256 = \"sha256:{}\"",
                tree_sha256(&missing_vendor.join("source"))
            ),
        ),
    );
    Command::new("chmod")
        .args(["-R", "a-w"])
        .arg(&missing_input)
        .status()
        .expect("reseal missing-vendor identity inputs");
    Command::new("chmod")
        .args(["-R", "a-w"])
        .arg(missing_vendor.join("source"))
        .status()
        .expect("seal missing-vendor source");
    write_f05_manifest(&missing_vendor);
    let missing_manifest = sha256(&missing_vendor.join("f05-inputs.sha256"));
    let missing_result = Command::new("bash")
        .arg(&script)
        .args(["--sealed-root"])
        .arg(&missing_vendor)
        .args(["--stage-closure-provenance-inputs"])
        .arg(&missing_input)
        .arg("--dependency-witness-only")
        .output()
        .expect("run missing physical dependency closure denial");
    assert!(
        !missing_result.status.success()
            && String::from_utf8_lossy(&missing_result.stderr)
                .contains("requires a physical dependency closure"),
        "witness-only must stop before provenance when the physical vendor closure is absent"
    );
    assert!(
        !missing_vendor
            .join("inputs/compiled-dependency-witness.json")
            .exists()
            && !missing_vendor
                .join("inputs/component-candidate-input.toml")
                .exists()
            && sha256(&missing_vendor.join("f05-inputs.sha256")) == missing_manifest,
        "the missing-vendor boundary must not emit generated inputs or rewrite its manifest"
    );
    Command::new("chmod")
        .args(["-R", "u+w"])
        .arg(&temp)
        .status()
        .expect("unseal fixture");
    fs::remove_dir_all(temp).expect("remove fixture");
}

#[test]
fn plugin_process_recipe_is_a_linux_amd64_component_only_fixture() {
    for required in [
        "Package: $package_name",
        "Architecture: amd64",
        "umask 022",
        "install -m 0755 \"$server\" \"$root/usr/bin/dasobjectstore-server\"",
        "find \"$root/opt/dasobjectstore/web\" -type d -exec chmod 0755 {} +",
        "find \"$root/opt/dasobjectstore/web\" -type f -exec chmod 0644 {} +",
        "plugin-process-descriptor.json",
        "\"$root/opt/dasobjectstore/web/\"",
        "das_plugin_process_write_provenance",
        "validate-plugin-process-package.sh",
    ] {
        assert!(
            BUILD.contains(required),
            "missing fixture contract: {required}"
        );
    }
    for forbidden in [
        "dasobjectstored",
        "lib/systemd",
        "DEBIAN/postinst",
        "DEBIAN/prerm",
        "DEBIAN/postrm",
    ] {
        assert!(
            !BUILD.contains(forbidden),
            "forbidden appliance surface: {forbidden}"
        );
    }
}

#[test]
fn plugin_package_version_is_derived_from_copied_cargo_metadata() {
    for required in [
        "metadata --locked --no-deps --format-version 1 --manifest-path \"$repo_root/Cargo.toml\"",
        "select(.name == \"dasobjectstore-cli\") | .version",
        "das_plugin_process_strict_semver \"$version\"",
        "strict DASObjectStore SemVer from copied sealed Cargo metadata",
    ] {
        assert!(
            BUILD.contains(required),
            "missing copied-Cargo version contract: {required}"
        );
    }
    assert!(
        !BUILD.contains("DASObjectStore 0.186.3"),
        "package validation must not retain a fixed stale version gate"
    );
}

#[test]
fn copied_metadata_semver_accepts_the_patch_and_rejects_malformed_values() {
    let source_script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../packaging/plugin-process-package-provenance.sh");
    for version in ["0.186.6", "1.0.0"] {
        let status = Command::new("bash")
            .args([
                "-c",
                "source \"$1\"; das_plugin_process_strict_semver \"$2\"",
                "fixture",
            ])
            .arg(&source_script)
            .arg(version)
            .status()
            .expect("run accepted SemVer regression");
        assert!(status.success(), "strict SemVer {version} must be accepted");
    }
    for version in ["", "01.186.6", "0.186", "0.186.6-beta"] {
        let status = Command::new("bash")
            .args([
                "-c",
                "source \"$1\"; das_plugin_process_strict_semver \"$2\"",
                "fixture",
            ])
            .arg(&source_script)
            .arg(version)
            .status()
            .expect("run malformed SemVer regression");
        assert!(
            !status.success(),
            "malformed SemVer {version:?} must be rejected"
        );
    }
}

#[test]
fn plugin_package_candidate_source_binding_rejects_genuine_mismatch() {
    for required in [
        "candidate source revision to match the source-tree witness",
        "candidate_revision=\"$(sed -n 's/^source_revision",
        "source_tree_revision=\"$(sed -n 's/^revision=",
    ] {
        assert!(
            BUILD.contains(required) || PROVENANCE.contains(required),
            "missing candidate source rejection: {required}"
        );
    }
}

#[test]
fn candidate_source_binding_accepts_the_patch_and_rejects_genuine_mismatch() {
    let temp = std::env::temp_dir().join(format!(
        "dasobjectstore-plugin-process-candidate-source-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time")
            .as_nanos()
    ));
    fs::create_dir(&temp).expect("temporary candidate-source root");
    let stage = staged_fixture(&temp);
    let source_script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../packaging/plugin-process-package-provenance.sh");
    let accepted = Command::new("bash")
        .args([
            "-c",
            "source \"$1\"; das_plugin_process_f05_require_staged_closure \"$2/source\"",
            "fixture",
        ])
        .arg(&source_script)
        .arg(&stage)
        .env("DASOBJECTSTORE_F05_STAGED_CLOSURE_ROOT", &stage)
        .status()
        .expect("read staged candidate source binding");
    assert!(
        accepted.success(),
        "accept matching candidate source binding"
    );
    write(
        stage.join("inputs/source-tree"),
        "revision=bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\n",
    );
    write_f05_manifest(&stage);
    let mismatched = Command::new("bash")
        .args([
            "-c",
            "source \"$1\"; das_plugin_process_f05_require_staged_closure \"$2/source\"",
            "fixture",
        ])
        .arg(&source_script)
        .arg(&stage)
        .env("DASOBJECTSTORE_F05_STAGED_CLOSURE_ROOT", &stage)
        .status()
        .expect("run candidate source mismatch rejection");
    assert!(
        !mismatched.success(),
        "candidate source revision mismatch must be rejected"
    );
    fs::remove_dir_all(temp).expect("remove temporary candidate-source root");
}

#[test]
fn package_validation_rejects_appliance_content_and_requires_descriptor_contract() {
    for required in [
        "dasobjectstore-plugin-process",
        "plugin server must be a Linux amd64 ELF executable",
        "mnemosyne.plugin-process-descriptor/v1",
        "plugin package is missing web index",
        "plugin package is missing WebAssembly assets",
        "plugin package is missing JavaScript assets",
        "plugin package contains forbidden appliance path",
        "plugin package must not contain symlinks",
    ] {
        assert!(
            VALIDATE.contains(required),
            "missing validation contract: {required}"
        );
    }
}

#[test]
fn provenance_binds_every_plugin_only_build_input_and_output() {
    for required in [
        "plugin-process-package-provenance.v1",
        "source_revision",
        "cargo_toml_sha256",
        "cargo_lock_sha256",
        "descriptor_source_sha256",
        "web_assets_sha256",
        "server_sha256",
        "package_sha256",
        "f05_staged_inputs_manifest_sha256",
        "f05_component_candidate_input_sha256",
        "f05_source_tree_sha256",
        "f05_dependency_witness_sha256",
        "f05_package_recipe_sha256",
        "f05_vendor_tree_sha256",
        "f05_vendor_config_sha256",
        "f05_cargo_sha256",
        "f05_rustc_sha256",
        "f05_trunk_sha256",
        "f05_wasm_target_sha256",
        "f05_toolchain_image_sha256",
        "f05_cargo_version",
        "f05_rustc_version",
        "f05_trunk_version",
    ] {
        assert!(
            PROVENANCE.contains(required),
            "missing provenance witness: {required}"
        );
    }
}

#[test]
fn staged_web_closure_is_exact_vendored_and_rejects_ambient_siblings() {
    for required in [
        "DASOBJECTSTORE_F05_STAGED_CLOSURE_ROOT",
        "requires an absolute DASOBJECTSTORE_F05_STAGED_CLOSURE_ROOT",
        "requires stage/source to be the repository root",
        "f05-inputs.sha256",
        "requires a staged vendor tree and vendor config",
        "requires absolute, non-symlink staged cargo, rustc, and trunk tools",
        "requires a staged wasm32-unknown-unknown target",
        "wasm-bindgen-0.2.128/wasm-bindgen",
        "wasm-opt-version_123/wasm-opt",
        "XDG_CACHE_HOME=\"$isolated_xdg_cache\"",
        "wasm-bindgen-0.2.128/wasm-bindgen",
        "wasm-opt-version_123/bin/wasm-opt",
        "requires hash-verified staged wasm-bindgen cache input",
        "requires hash-verified staged wasm-opt cache input",
        "requires f05-inputs.sha256 to bind",
        "requires a non-empty staged vendor tree",
        "requires a non-empty staged wasm32-unknown-unknown target",
        "CARGO_NET_OFFLINE=true",
        "RUSTC=\"$staged_rustc\"",
        "\"$staged_trunk\" build --release",
        "F05_PROSOPIKON_REVISION=f09749273ef382c1b42bf04a77d96189dd7361b3",
        "requires the exact Prosopikon lock revision",
        "rejects an ambient Prosopikon sibling",
        "does not accept a prebuilt web distribution",
        "does not permit the developer fallback",
        "DASOBJECTSTORE_F05_ATTEMPT_ROOT",
        "requires an absolute, non-symlink F05 attempt root",
    ] {
        assert!(
            PREPARE_WEB_DIST.contains(required),
            "missing staged closure contract: {required}"
        );
    }
}

#[test]
fn package_attempt_requires_real_network_namespace_isolation() {
    for required in [
        "bwrap_args=(--unshare-net)",
        "bwrap_args+=(--ro-bind / / --ro-bind \"$sealed_root\" /opt --bind \"$attempt_root\" /var/tmp --bind \"$diagnostic_root\" /var/cache --proc /proc --dev /dev)",
        "exec /usr/bin/bwrap \"${bwrap_args[@]}\"",
        "--sealed-root /opt --attempt-root /var/tmp --diagnostic-root /var/cache",
        "requires /usr/bin/bwrap network isolation",
        "requires loopback-only network interfaces",
        "requires empty IPv4 routes",
    ] {
        assert!(
            ATTEMPT.contains(required),
            "missing network-isolation contract: {required}"
        );
    }
    assert!(
        !ATTEMPT.contains("--bind /tmp"),
        "the Bubblewrap runner must not make host /tmp writable"
    );
}

#[test]
fn staged_web_build_binds_writable_attempt_tmp_to_rust_and_trunk() {
    for required in [
        "requires a writable per-attempt temporary directory",
        "XDG_CACHE_HOME=\"$isolated_xdg_cache\" TMPDIR=\"$attempt_root/tmp\" TMP=\"$attempt_root/tmp\" TEMP=\"$attempt_root/tmp\"",
    ] {
        assert!(
            PREPARE_WEB_DIST.contains(required),
            "missing staged Trunk temporary-storage contract: {required}"
        );
    }
}

#[test]
fn external_attempt_harness_confines_writes_to_a_copied_closure() {
    for required in [
        "--sealed-root",
        "--attempt-root",
        "--diagnostic-root",
        "--stage-cache-root",
        "reject_symlink_ancestry \"$sealed_root\" 'sealed closure root'",
        "reject_symlink_ancestry \"$attempt_root\" 'external attempt root'",
        "reject_symlink_ancestry \"$diagnostic_root\" 'external diagnostic root'",
        "must not pass through a symlink",
        "sealed closure must not contain symlinks",
        "external attempt root must be empty",
        "external diagnostic root must be empty",
        "external diagnostic root and external attempt root must not overlap",
        "preflight.log",
        "attempt_root_empty=PASS",
        "preflight_failure=$*",
        "cp -a --reflink=auto \"$sealed_root\" \"$copied_closure\"",
        "write_batched_manifest",
        "xargs -0 -r -n 128 -P \"$hash_jobs\"",
        "stage-reuse-receipt",
        "stage_cache_cold=PASS",
        "stage_cache_reuse=PASS",
        "leased stage cache root must be immutable before reuse",
        "chmod -R u+w \"$copied_closure\"",
        "attempt_cargo_home=\"$attempt_root/cargo-home\"",
        "copied closure Cargo cache must remain immutable",
        "release build requires a physical per-attempt Cargo cache copy",
        "release build altered the copied closure instead of its per-attempt Cargo cache",
        "copied_manifest=\"$copied_source/Cargo.toml\"",
        "copied_lock=\"$copied_source/Cargo.lock\"",
        "copied closure requires a physical non-symlink",
        "build --manifest-path \"$copied_manifest\"",
        "DASOBJECTSTORE_F05_ATTEMPT_ROOT=\"$attempt_root\"",
        "export TMPDIR=\"$attempt_root/tmp\"",
        "diagnostic_status=\"$diagnostic_root/terminal-status\"",
        "printf 'exit_code=%s\\n' \"$status\" > \"$status_file\"",
        "umask 022",
    ] {
        assert!(
            ATTEMPT.contains(required),
            "missing external attempt harness contract: {required}"
        );
    }
}

#[cfg(target_os = "linux")]
#[test]
fn external_attempt_harness_copies_sealed_inputs_and_retains_real_failure_status() {
    let temp = fs::canonicalize(std::env::temp_dir())
        .expect("canonical temporary directory")
        .join(format!(
            "dasobjectstore-plugin-process-attempt-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time")
                .as_nanos()
        ));
    fs::create_dir(&temp).expect("temporary harness root");
    let sealed = staged_fixture(&temp);
    let staged_cargo = sealed.join("toolchain/bin/cargo");
    write(
        &staged_cargo,
        "#!/bin/sh\nprintf 'cwd=%s\\n' \"$PWD\" > \"$DASOBJECTSTORE_F05_ATTEMPT_ROOT/cargo-invocation.log\"\nprintf 'argv=%s\\n' \"$*\" >> \"$DASOBJECTSTORE_F05_ATTEMPT_ROOT/cargo-invocation.log\"\nprintf 'cargo_home=%s\\n' \"$CARGO_HOME\" >> \"$DASOBJECTSTORE_F05_ATTEMPT_ROOT/cargo-invocation.log\"\nprintf 'tmpdir=%s\\n' \"$TMPDIR\" >> \"$DASOBJECTSTORE_F05_ATTEMPT_ROOT/cargo-invocation.log\"\nprintf 'umask=%s\\n' \"$(umask)\" >> \"$DASOBJECTSTORE_F05_ATTEMPT_ROOT/cargo-invocation.log\"\ntest \"$TMPDIR\" = \"$DASOBJECTSTORE_F05_ATTEMPT_ROOT/tmp\" && test -d \"$TMPDIR\" && test -w \"$TMPDIR\" || exit 72\nmkdir -p \"$CARGO_HOME/.global-cache\"\nprintf 'mutable Cargo cache\\n' > \"$CARGO_HOME/.global-cache/fixture\"\nprintf 'tmp_writable=PASS\\n' >> \"$DASOBJECTSTORE_F05_ATTEMPT_ROOT/cargo-invocation.log\"\nexit 71\n",
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&staged_cargo, fs::Permissions::from_mode(0o755))
            .expect("make capturing staged Cargo executable");
    }
    write_f05_manifest(&sealed);
    let manifest = fs::read(sealed.join("f05-inputs.sha256")).expect("read sealed manifest");
    let attempt = temp.join("attempt");
    fs::create_dir(&attempt).expect("create external attempt root");
    let diagnostic = temp.join("diagnostic");
    fs::create_dir(&diagnostic).expect("create external diagnostic root");
    let non_workspace_cwd = temp.join("outside-workspace");
    fs::create_dir(&non_workspace_cwd).expect("create non-workspace cwd");
    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../packaging/debian/run-plugin-process-package-attempt.sh");

    let status = Command::new("bash")
        .args(["-c", "umask 077; exec \"$@\"", "fixture"])
        .arg(&script)
        .args(["--sealed-root"])
        .arg(&sealed)
        .args(["--attempt-root"])
        .arg(&attempt)
        .args(["--diagnostic-root"])
        .arg(&diagnostic)
        .current_dir(&non_workspace_cwd)
        .status()
        .expect("run forced-failure package attempt");
    assert!(
        !status.success(),
        "stub staged Cargo must force a real failure"
    );
    let cargo_invocation = fs::read_to_string(attempt.join("cargo-invocation.log"))
        .expect("captured staged Cargo invocation");
    assert!(
        cargo_invocation.contains("cwd=/var/tmp/closure/source"),
        "Cargo must run from the stable copied-source path rather than the caller cwd"
    );
    assert!(
        cargo_invocation.contains("--manifest-path /var/tmp/closure/source/Cargo.toml"),
        "Cargo must receive the explicit copied manifest path"
    );
    assert!(
        cargo_invocation
            .find("argv=build --manifest-path")
            .is_some(),
        "Cargo must receive the build subcommand before its manifest argument"
    );
    assert!(
        cargo_invocation.contains("tmpdir=/var/tmp/tmp")
            && cargo_invocation.contains("umask=0022")
            && cargo_invocation.contains("tmp_writable=PASS"),
        "the Bubblewrap-isolated staged Rust invocation must override a restrictive caller umask while using stable paths and writable temporary storage"
    );
    assert_eq!(
        fs::read_to_string(diagnostic.join("terminal-status")).expect("read terminal status"),
        format!("exit_code={}\n", status.code().expect("exit code")),
        "attempt must retain the actual nonzero terminal status"
    );
    assert_eq!(
        fs::read(sealed.join("f05-inputs.sha256")).expect("read sealed manifest after attempt"),
        manifest,
        "attempt must not modify the sealed original"
    );
    assert!(
        attempt.join("closure/source").is_dir(),
        "attempt has a work copy"
    );
    assert!(
        fs::read_to_string(attempt.join("closure/source/.cargo/f05-vendor-config.toml"))
            .expect("read copied vendor config")
            .contains("directory = \"/var/tmp/closure/source/vendor\""),
        "the copied Cargo vendor path must use the stable Bubblewrap mount"
    );
    assert!(
        fs::read_to_string(diagnostic.join("preflight.log"))
            .expect("read external preflight log")
            .contains("attempt_root_empty=PASS"),
        "diagnostics must prove the attempt root was empty at runner entry"
    );
    assert!(
        !attempt.join("preflight.log").exists() && !attempt.join("terminal-status").exists(),
        "diagnostics must remain outside the supplied attempt root"
    );
    assert!(
        verify_f05_manifest(&attempt.join("closure")),
        "a mutable Cargo global cache must not invalidate the copied closure manifest"
    );
    assert!(
        cargo_invocation.contains("cargo_home=/var/tmp/cargo-home")
            && attempt.join("cargo-home/.global-cache/fixture").is_file()
            && !attempt.join("closure/cargo-home/.global-cache/fixture").exists(),
        "the release build must receive a separate mutable Cargo cache outside the manifest-bound copied closure"
    );

    let escape = temp.join("escape");
    fs::create_dir(&escape).expect("create symlink escape target");
    let symlink_attempt = temp.join("attempt-link");
    let symlink_diagnostic = temp.join("symlink-diagnostic");
    fs::create_dir(&symlink_diagnostic).expect("create symlink diagnostic root");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&escape, &symlink_attempt).expect("create attempt symlink");
    let escaped = Command::new("bash")
        .arg(&script)
        .args(["--sealed-root"])
        .arg(&sealed)
        .args(["--attempt-root"])
        .arg(&symlink_attempt)
        .args(["--diagnostic-root"])
        .arg(&symlink_diagnostic)
        .status()
        .expect("run symlink escape attempt");
    assert!(
        !escaped.success(),
        "harness must reject an attempt-root symlink"
    );
    assert!(
        !escape.join("terminal-status").exists(),
        "harness must not write through the symlink escape"
    );

    let symlink_parent_target = temp.join("symlink-parent-target");
    fs::create_dir(&symlink_parent_target).expect("create symlink parent target");
    let symlink_parent = temp.join("symlink-parent");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&symlink_parent_target, &symlink_parent)
        .expect("create symlinked attempt parent");
    let attempt_below_symlink = symlink_parent.join("attempt");
    let parent_diagnostic = temp.join("parent-diagnostic");
    fs::create_dir(&parent_diagnostic).expect("create parent diagnostic root");
    let ancestry_rejected = Command::new("bash")
        .arg(&script)
        .args(["--sealed-root"])
        .arg(&sealed)
        .args(["--attempt-root"])
        .arg(&attempt_below_symlink)
        .args(["--diagnostic-root"])
        .arg(&parent_diagnostic)
        .status()
        .expect("run symlink-parent attempt");
    assert!(
        !ancestry_rejected.success(),
        "harness must reject an attempt root below a symlinked parent"
    );
    assert!(
        !symlink_parent_target.join("attempt").exists(),
        "harness must not create a closure or terminal status through a symlinked parent"
    );

    let nonempty_attempt = temp.join("nonempty-attempt");
    fs::create_dir(&nonempty_attempt).expect("create nonempty attempt root");
    write(
        nonempty_attempt.join("launcher.log"),
        "external diagnostic\n",
    );
    let preflight_diagnostic = temp.join("preflight-diagnostic");
    fs::create_dir(&preflight_diagnostic).expect("create preflight diagnostic root");
    let preflight_rejected = Command::new("bash")
        .arg(&script)
        .args(["--sealed-root"])
        .arg(&sealed)
        .args(["--attempt-root"])
        .arg(&nonempty_attempt)
        .args(["--diagnostic-root"])
        .arg(&preflight_diagnostic)
        .status()
        .expect("run nonempty attempt-root rejection");
    assert!(
        !preflight_rejected.success(),
        "harness must reject a nonempty attempt root before Bubblewrap"
    );
    assert!(
        fs::read_to_string(preflight_diagnostic.join("preflight.log"))
            .expect("read external preflight failure")
            .contains("preflight_failure=external attempt root must be empty"),
        "preflight failure must be retained outside the rejected attempt root"
    );
    assert!(
        !nonempty_attempt.join("terminal-status").exists(),
        "runner must not write a status into the rejected attempt root"
    );
    Command::new("chmod")
        .args(["-R", "u+w"])
        .arg(&temp)
        .status()
        .expect("unseal temporary harness root before cleanup");
    fs::remove_dir_all(temp).expect("remove temporary harness root");
}

#[cfg(target_os = "linux")]
#[test]
fn closure_stage_producer_binds_current_inputs_before_real_preflight() {
    use std::os::unix::fs::PermissionsExt;

    let temp = fs::canonicalize(std::env::temp_dir())
        .expect("canonical temporary directory")
        .join(format!(
            "dasobjectstore-plugin-process-closure-stage-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time")
                .as_nanos()
        ));
    fs::create_dir(&temp).expect("create closure-stage fixture root");
    let sealed = staged_fixture(&temp);
    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../packaging/debian/run-plugin-process-package-attempt.sh");
    let config = sealed.join("source/.cargo/f05-vendor-config.toml");
    fs::remove_file(&config).expect("remove externally supplied vendor config");

    let staged = Command::new("bash")
        .arg(&script)
        .args(["--sealed-root"])
        .arg(&sealed)
        .arg("--stage-closure-config")
        .output()
        .expect("run source-owned closure-stage producer");
    assert!(
        staged.status.success(),
        "closure-stage producer failed: {}",
        String::from_utf8_lossy(&staged.stderr)
    );
    let staged_stdout = String::from_utf8(staged.stdout).expect("closure-stage output");
    assert!(
        staged_stdout.contains("closure_stage_vendor_config=PASS"),
        "producer must retain its generator/config receipt"
    );
    assert_eq!(
        fs::read_to_string(&config).expect("read generated vendor config"),
        format!(
            "[source.vendored-sources]\ndirectory = \"{}\"\n",
            sealed.join("source/vendor").display()
        ),
        "producer must bind the physical staged vendor tree, not a fixture path"
    );
    let manifest = fs::read_to_string(sealed.join("f05-inputs.sha256"))
        .expect("read generated staged manifest");
    for input in [
        "source/.cargo/f05-vendor-config.toml",
        "source/packaging/debian/run-plugin-process-package-attempt.sh",
        "inputs/component-candidate-input.toml",
        "inputs/source-tree",
        "inputs/compiled-dependency-witness.json",
    ] {
        assert!(manifest.contains(input), "manifest must bind {input}");
    }

    let cache = temp.join("leased-cache");
    let attempt = temp.join("attempt");
    let diagnostic = temp.join("diagnostic");
    fs::create_dir(&cache).expect("create leased cache root");
    fs::create_dir(&attempt).expect("create external attempt root");
    fs::create_dir(&diagnostic).expect("create external diagnostic root");
    let preflight = Command::new("bash")
        .arg(&script)
        .args(["--sealed-root"])
        .arg(&sealed)
        .args(["--attempt-root"])
        .arg(&attempt)
        .args(["--diagnostic-root"])
        .arg(&diagnostic)
        .args(["--stage-cache-root"])
        .arg(&cache)
        .arg("--preflight-only")
        .status()
        .expect("run real preflight handoff");
    assert!(
        preflight.success(),
        "generated source stage must hand off to the real preflight"
    );
    assert!(
        fs::read_to_string(diagnostic.join("preflight.log"))
            .expect("read preflight receipt")
            .contains("preflight_only=PASS"),
        "preflight must retain the source-stage handoff"
    );
    assert!(
        !attempt.join("cargo.log").exists()
            && !attempt.join("target").exists()
            && !attempt.join("output").exists(),
        "stage-to-preflight handoff must not build, prepare web assets, or package"
    );

    let missing_root = temp.join("missing-vendor");
    copy_tree(&sealed, &missing_root);
    let missing = missing_root;
    fs::set_permissions(
        missing.join("source/.cargo"),
        fs::Permissions::from_mode(0o755),
    )
    .expect("make copied config parent writable");
    fs::remove_file(missing.join("source/.cargo/f05-vendor-config.toml"))
        .expect("remove copied generated config");
    fs::remove_dir_all(missing.join("source/vendor")).expect("remove copied vendor tree");
    assert!(
        !Command::new("bash")
            .arg(&script)
            .args(["--sealed-root"])
            .arg(&missing)
            .arg("--stage-closure-config")
            .status()
            .expect("run missing vendor denial")
            .success(),
        "producer must reject a missing physical vendor tree"
    );

    let mismatched_root = temp.join("mismatched-revision");
    copy_tree(&sealed, &mismatched_root);
    let mismatched = mismatched_root;
    fs::set_permissions(mismatched.join("inputs"), fs::Permissions::from_mode(0o755))
        .expect("make copied inputs writable");
    write(
        mismatched.join("inputs/component-candidate-input.toml"),
        "source_revision = \"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\"\n",
    );
    assert!(
        !Command::new("bash")
            .arg(&script)
            .args(["--sealed-root"])
            .arg(&mismatched)
            .arg("--stage-closure-config")
            .status()
            .expect("run mismatched revision denial")
            .success(),
        "producer must reject a candidate/source-tree/witness revision mismatch"
    );

    let escaped_root = temp.join("escaped-vendor");
    copy_tree(&sealed, &escaped_root);
    let escaped = escaped_root;
    fs::set_permissions(escaped.join("source"), fs::Permissions::from_mode(0o755))
        .expect("make copied source writable");
    fs::remove_dir_all(escaped.join("source/vendor"))
        .expect("remove copied vendor for symlink denial");
    let escape_target = temp.join("vendor-escape-target");
    fs::create_dir(&escape_target).expect("create vendor escape target");
    std::os::unix::fs::symlink(&escape_target, escaped.join("source/vendor"))
        .expect("create vendor symlink escape");
    assert!(
        !Command::new("bash")
            .arg(&script)
            .args(["--sealed-root"])
            .arg(&escaped)
            .arg("--stage-closure-config")
            .status()
            .expect("run vendor symlink denial")
            .success(),
        "producer must reject a symlinked or escaped vendor path"
    );
    assert!(
        !escape_target.join("f05-vendor-config.toml").exists(),
        "producer must not write through a vendor symlink escape"
    );

    assert!(
        Command::new("chmod")
            .args(["-R", "u+w"])
            .arg(&temp)
            .status()
            .expect("restore disposable fixture permissions")
            .success(),
        "test cleanup may only restore permissions on its disposable fixture"
    );
    fs::remove_dir_all(temp).expect("remove closure-stage fixture root");
}

#[cfg(target_os = "linux")]
#[test]
fn external_attempt_harness_reuses_only_a_fully_revalidated_immutable_leased_stage() {
    use std::os::unix::fs::PermissionsExt;

    let temp = fs::canonicalize(std::env::temp_dir())
        .expect("canonical temporary directory")
        .join(format!(
            "dasobjectstore-plugin-process-stage-cache-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time")
                .as_nanos()
        ));
    fs::create_dir(&temp).expect("create temporary stage-cache root");
    let sealed = staged_fixture(&temp);
    let staged_cargo = sealed.join("toolchain/bin/cargo");
    write(
        &staged_cargo,
        "#!/bin/sh\nif [ \"${1-}\" = tree ]; then\n  test -d \"$CARGO_HOME/git/checkouts/prosopikon-739f7520363f0e4d/f097492\" || exit 72\n  printf 'offline_resolution=called\\n' >> \"$DASOBJECTSTORE_F05_ATTEMPT_ROOT/cargo-resolution.log\"\n  exit 0\nfi\nprintf 'cargo=called\\n' >> \"$DASOBJECTSTORE_F05_ATTEMPT_ROOT/cargo.log\"\nexit 71\n",
    );
    fs::set_permissions(&staged_cargo, fs::Permissions::from_mode(0o755))
        .expect("make staged Cargo executable");
    write_f05_manifest(&sealed);
    let sealed_manifest = fs::read(sealed.join("f05-inputs.sha256")).expect("read sealed manifest");
    let cache = temp.join("leased-cache");
    fs::create_dir(&cache).expect("create leased stage cache root");
    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../packaging/debian/run-plugin-process-package-attempt.sh");

    let run = |name: &str, cache_root: &Path| {
        let attempt = temp.join(format!("{name}-attempt"));
        let diagnostic = temp.join(format!("{name}-diagnostic"));
        fs::create_dir(&attempt).expect("create fresh attempt root");
        fs::create_dir(&diagnostic).expect("create fresh diagnostic root");
        let status = Command::new("bash")
            .arg(&script)
            .args(["--sealed-root"])
            .arg(&sealed)
            .args(["--attempt-root"])
            .arg(&attempt)
            .args(["--diagnostic-root"])
            .arg(&diagnostic)
            .args(["--stage-cache-root"])
            .arg(cache_root)
            .status()
            .expect("run cached package-attempt fixture");
        (attempt, diagnostic, status)
    };

    let run_preflight = |name: &str, cache_root: &Path| {
        let attempt = temp.join(format!("{name}-attempt"));
        let diagnostic = temp.join(format!("{name}-diagnostic"));
        fs::create_dir(&attempt).expect("create fresh preflight attempt root");
        fs::create_dir(&diagnostic).expect("create fresh preflight diagnostic root");
        let status = Command::new("bash")
            .arg(&script)
            .args(["--sealed-root"])
            .arg(&sealed)
            .args(["--attempt-root"])
            .arg(&attempt)
            .args(["--diagnostic-root"])
            .arg(&diagnostic)
            .args(["--stage-cache-root"])
            .arg(cache_root)
            .arg("--preflight-only")
            .status()
            .expect("run cached preflight-only fixture");
        (attempt, diagnostic, status)
    };

    let preflight_cache = temp.join("preflight-leased-cache");
    fs::create_dir(&preflight_cache).expect("create preflight leased stage cache root");
    let (preflight_cold_attempt, preflight_cold_diagnostic, preflight_cold_status) =
        run_preflight("preflight-cold", &preflight_cache);
    assert!(
        preflight_cold_status.success(),
        "preflight-only must accept a freshly admitted cold leased stage"
    );
    let preflight_cold_log = fs::read_to_string(preflight_cold_diagnostic.join("preflight.log"))
        .expect("read cold preflight diagnostic");
    assert!(
        preflight_cold_log.contains("stage_cache_cold=PASS")
            && preflight_cold_log.contains("preflight_only=PASS"),
        "cold preflight must retain cache admission and copied-vendor binding"
    );
    assert!(
        fs::read_to_string(
            preflight_cold_attempt.join("closure/source/.cargo/f05-vendor-config.toml"),
        )
        .expect("read cold preflight copied config")
        .contains("directory = \"/var/tmp/closure/source/vendor\""),
        "cold preflight must use the web preparer's copied vendor path"
    );
    assert!(
        preflight_cold_attempt
            .join("cargo-resolution.log")
            .is_file()
            && !preflight_cold_attempt.join("cargo.log").exists()
            && !preflight_cold_attempt.join("target").exists()
            && !preflight_cold_attempt.join("output").exists(),
        "preflight-only must resolve the exact locked Cargo build without compiling, preparing web assets, or creating package output"
    );
    assert!(
        !preflight_cold_diagnostic
            .join("package-success-receipt")
            .exists(),
        "preflight-only must not emit a package-success receipt"
    );

    let (preflight_warm_attempt, preflight_warm_diagnostic, preflight_warm_status) =
        run_preflight("preflight-warm", &preflight_cache);
    assert!(
        preflight_warm_status.success(),
        "preflight-only must accept the fully revalidated warm leased stage"
    );
    let preflight_warm_log = fs::read_to_string(preflight_warm_diagnostic.join("preflight.log"))
        .expect("read warm preflight diagnostic");
    assert!(
        preflight_warm_log.contains("stage_cache_reuse=PASS")
            && preflight_warm_log.contains("preflight_only=PASS"),
        "warm preflight must retain cache reuse and copied-vendor binding"
    );
    assert!(
        fs::read_to_string(
            preflight_warm_attempt.join("closure/source/.cargo/f05-vendor-config.toml"),
        )
        .expect("read warm preflight copied config")
        .contains("directory = \"/var/tmp/closure/source/vendor\""),
        "warm preflight must use its copied vendor path without mutating the cache"
    );
    assert!(
        preflight_warm_attempt
            .join("cargo-resolution.log")
            .is_file()
            && !preflight_warm_attempt.join("cargo.log").exists()
            && !preflight_warm_attempt.join("target").exists()
            && !preflight_warm_attempt.join("output").exists(),
        "warm preflight-only must not invoke Cargo, web preparation, or package output"
    );

    let (cold_attempt, cold_diagnostic, cold_status) = run("cold", &cache);
    assert!(
        !cold_status.success(),
        "fixture Cargo must stop the cold attempt"
    );
    assert!(
        fs::read_to_string(cold_diagnostic.join("preflight.log"))
            .expect("read cold stage diagnostic")
            .contains("stage_cache_cold=PASS"),
        "cold attempt must record creation of the leased immutable stage"
    );
    let receipt =
        fs::read_to_string(cache.join("stage-reuse-receipt")).expect("read leased stage receipt");
    for field in [
        "lease_key=",
        "sealed_manifest_sha256=",
        "runner_sha256=",
        "copied_config_sha256=",
    ] {
        assert!(receipt.contains(field), "receipt must bind {field}");
    }
    assert!(
        cache
            .join("current/source/.cargo/f05-vendor-config.toml")
            .is_file()
            && fs::read_to_string(cache.join("current/source/.cargo/f05-vendor-config.toml"))
                .expect("read immutable cache config")
                .contains("/mnt/current/source/vendor"),
        "the immutable cache receipt must retain its fixed Bubblewrap cache path"
    );
    assert!(
        fs::read_to_string(cold_attempt.join("closure/source/.cargo/f05-vendor-config.toml"))
            .expect("read cold attempt config")
            .contains("directory = \"/var/tmp/closure/source/vendor\""),
        "cold execution must derive an attempt-local config bound to its copied vendor tree"
    );
    assert_eq!(
        fs::metadata(&cache)
            .expect("read immutable cache mode")
            .permissions()
            .mode()
            & 0o222,
        0,
        "leased stage cache root must become immutable before reuse"
    );
    assert_eq!(
        fs::read(sealed.join("f05-inputs.sha256")).expect("re-read sealed manifest"),
        sealed_manifest,
        "cold cache creation must not change the sealed source inputs"
    );

    let (warm_attempt, warm_diagnostic, warm_status) = run("warm", &cache);
    assert!(
        !warm_status.success(),
        "fixture Cargo must stop the warm attempt"
    );
    assert!(
        fs::read_to_string(warm_diagnostic.join("preflight.log"))
            .expect("read warm stage diagnostic")
            .contains("stage_cache_reuse=PASS"),
        "warm attempt must reuse the fully revalidated leased stage"
    );
    assert!(
        warm_attempt.join("cargo.log").is_file(),
        "warm attempt must reach staged Cargo"
    );
    assert!(
        fs::read_to_string(warm_attempt.join("closure/source/.cargo/f05-vendor-config.toml"))
            .expect("read warm attempt config")
            .contains("directory = \"/var/tmp/closure/source/vendor\""),
        "warm execution must derive an attempt-local config without modifying the cache"
    );

    let writable_mode = temp.join("writable-mode-cache");
    copy_tree(&cache, &writable_mode);
    let writable_cached_input = writable_mode.join("current/source/vendor/fixture.crate");
    fs::set_permissions(&writable_cached_input, fs::Permissions::from_mode(0o644))
        .expect("make cached input unexpectedly writable");
    let (_, writable_mode_diagnostic, writable_mode_status) = run("writable-mode", &writable_mode);
    assert!(
        !writable_mode_status.success(),
        "a writable cached stage input must be rejected"
    );
    assert!(
        fs::read_to_string(writable_mode_diagnostic.join("preflight.log"))
            .expect("read writable-mode diagnostic")
            .contains("leased stage cache must be immutable before reuse"),
        "warm reuse must reject a cached stage with writable contents"
    );

    let tampered = temp.join("tampered-cache");
    copy_tree(&cache, &tampered);
    fs::set_permissions(&tampered, fs::Permissions::from_mode(0o755))
        .expect("make tampered cache root writable");
    let tampered_vendor = tampered.join("current/source/vendor/fixture.crate");
    fs::set_permissions(
        tampered_vendor.parent().expect("vendor parent"),
        fs::Permissions::from_mode(0o755),
    )
    .expect("make tampered vendor parent writable");
    fs::set_permissions(&tampered_vendor, fs::Permissions::from_mode(0o644))
        .expect("make tampered vendor input writable");
    write(&tampered_vendor, "tampered cached input\n");
    fs::set_permissions(&tampered_vendor, fs::Permissions::from_mode(0o444))
        .expect("restore immutable tampered vendor input mode");
    fs::set_permissions(
        tampered_vendor.parent().expect("vendor parent"),
        fs::Permissions::from_mode(0o555),
    )
    .expect("restore immutable tampered vendor parent mode");
    fs::set_permissions(&tampered, fs::Permissions::from_mode(0o555))
        .expect("restore immutable tampered cache root");
    let (_, tampered_diagnostic, tampered_status) = run_preflight("tampered", &tampered);
    assert!(
        !tampered_status.success(),
        "tampered cache must be rejected"
    );
    assert!(
        fs::read_to_string(tampered_diagnostic.join("preflight.log"))
            .expect("read tampered diagnostic")
            .contains("leased stage cache has a missing or altered input"),
        "warm reuse must fully revalidate cached content"
    );

    let missing_receipt = temp.join("missing-receipt-cache");
    copy_tree(&cache, &missing_receipt);
    fs::set_permissions(&missing_receipt, fs::Permissions::from_mode(0o755))
        .expect("make missing-receipt cache root writable");
    fs::remove_file(missing_receipt.join("stage-reuse-receipt")).expect("remove cache receipt");
    let (_, missing_diagnostic, missing_status) =
        run_preflight("missing-receipt", &missing_receipt);
    assert!(
        !missing_status.success(),
        "cache missing its receipt must be rejected"
    );
    assert!(
        fs::read_to_string(missing_diagnostic.join("preflight.log"))
            .expect("read missing receipt diagnostic")
            .contains("leased stage cache requires both a physical receipt and current stage"),
        "cache reuse must fail closed when its receipt is absent"
    );

    let mismatched_receipt = temp.join("mismatched-receipt-cache");
    copy_tree(&cache, &mismatched_receipt);
    fs::set_permissions(&mismatched_receipt, fs::Permissions::from_mode(0o755))
        .expect("make mismatched-receipt cache root writable");
    fs::set_permissions(
        mismatched_receipt.join("stage-reuse-receipt"),
        fs::Permissions::from_mode(0o644),
    )
    .expect("make mismatched cache receipt writable");
    write(
        mismatched_receipt.join("stage-reuse-receipt"),
        "lease_key=wrong-runner-or-cache-key\n",
    );
    fs::set_permissions(
        mismatched_receipt.join("stage-reuse-receipt"),
        fs::Permissions::from_mode(0o444),
    )
    .expect("restore immutable mismatched cache receipt mode");
    fs::set_permissions(&mismatched_receipt, fs::Permissions::from_mode(0o555))
        .expect("restore immutable mismatched cache root");
    let (_, mismatch_diagnostic, mismatch_status) = run("mismatched-receipt", &mismatched_receipt);
    assert!(
        !mismatch_status.success(),
        "runner or cache-key mismatch must be rejected"
    );
    assert!(
        fs::read_to_string(mismatch_diagnostic.join("preflight.log"))
            .expect("read mismatch diagnostic")
            .contains(
                "leased stage cache receipt does not bind this manifest, runner, and copied config"
            ),
        "cache receipt must bind runner bytes and copied configuration"
    );

    let missing_config = temp.join("missing-config-cache");
    copy_tree(&cache, &missing_config);
    fs::set_permissions(&missing_config, fs::Permissions::from_mode(0o755))
        .expect("make missing-config cache root writable");
    let config = missing_config.join("current/source/.cargo/f05-vendor-config.toml");
    fs::set_permissions(
        config.parent().expect("config parent"),
        fs::Permissions::from_mode(0o755),
    )
    .expect("make config parent writable");
    fs::remove_file(config).expect("remove cached config");
    fs::set_permissions(
        missing_config.join("current/source/.cargo"),
        fs::Permissions::from_mode(0o555),
    )
    .expect("restore immutable missing-config parent");
    fs::set_permissions(&missing_config, fs::Permissions::from_mode(0o555))
        .expect("restore immutable missing-config cache root");
    let (_, config_diagnostic, config_status) = run_preflight("missing-config", &missing_config);
    assert!(
        !config_status.success(),
        "cache missing copied config must be rejected"
    );
    assert!(
        fs::read_to_string(config_diagnostic.join("preflight.log"))
            .expect("read missing config diagnostic")
            .contains("leased stage cache requires a physical copied vendor config"),
        "cache reuse must reject a missing copied config"
    );

    let missing_manifest = temp.join("missing-manifest-cache");
    copy_tree(&cache, &missing_manifest);
    fs::set_permissions(&missing_manifest, fs::Permissions::from_mode(0o755))
        .expect("make missing-manifest cache root writable");
    fs::set_permissions(
        missing_manifest.join("current"),
        fs::Permissions::from_mode(0o755),
    )
    .expect("make cached manifest parent writable");
    let manifest = missing_manifest.join("current/f05-inputs.sha256");
    fs::set_permissions(&manifest, fs::Permissions::from_mode(0o644))
        .expect("make cached manifest writable");
    fs::remove_file(manifest).expect("remove cached manifest");
    fs::set_permissions(
        missing_manifest.join("current"),
        fs::Permissions::from_mode(0o555),
    )
    .expect("restore immutable cached manifest parent");
    fs::set_permissions(&missing_manifest, fs::Permissions::from_mode(0o555))
        .expect("restore immutable missing-manifest cache root");
    let (_, manifest_diagnostic, manifest_status) =
        run_preflight("missing-manifest", &missing_manifest);
    assert!(
        !manifest_status.success(),
        "cache missing its manifest must be rejected"
    );
    assert!(
        fs::read_to_string(manifest_diagnostic.join("preflight.log"))
            .expect("read missing manifest diagnostic")
            .contains("leased stage cache has a missing or altered input"),
        "cache reuse must reject a missing cached manifest"
    );

    let cache_escape = temp.join("cache-escape");
    let cache_link = temp.join("cache-link");
    fs::create_dir(&cache_escape).expect("create cache escape target");
    std::os::unix::fs::symlink(&cache_escape, &cache_link).expect("create cache symlink");
    let (_, _escape_diagnostic, escape_status) = run("cache-symlink", &cache_link);
    assert!(
        !escape_status.success(),
        "cache-root symlink must be rejected"
    );
    assert!(
        fs::read_dir(&cache_escape)
            .expect("read cache escape target")
            .next()
            .is_none(),
        "cache-root escape must fail before a cache write"
    );
    assert!(
        Command::new("chmod")
            .args(["-R", "u+w"])
            .arg(&temp)
            .status()
            .expect("make temporary stage-cache fixture writable")
            .success(),
        "temporary stage-cache fixture must be writable before cleanup"
    );
    fs::remove_dir_all(temp).expect("remove temporary stage-cache fixture root");
}

#[cfg(target_os = "linux")]
#[test]
fn preflight_cargo_tree_denies_a_separately_copied_missing_git_input_before_compilation() {
    use std::os::unix::fs::PermissionsExt;

    let temp = fs::canonicalize(std::env::temp_dir())
        .expect("canonical temporary directory")
        .join(format!(
            "dasobjectstore-plugin-process-missing-git-input-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time")
                .as_nanos()
        ));
    fs::create_dir(&temp).expect("create missing Git-input fixture root");

    let sealed = staged_fixture(&temp);
    let staged_cargo = sealed.join("toolchain/bin/cargo");
    write(
        &staged_cargo,
        "#!/bin/sh\nif [ \"${1-}\" = tree ]; then\n  checkout=\"$CARGO_HOME/git/checkouts/prosopikon-739f7520363f0e4d/f097492\"\n  if [ ! -d \"$checkout\" ]; then\n    printf 'missing_git_input=prosopikon\\n' >> \"$DASOBJECTSTORE_F05_ATTEMPT_ROOT/cargo-tree.log\"\n    exit 72\n  fi\n  printf 'offline_resolution=called\\n' >> \"$DASOBJECTSTORE_F05_ATTEMPT_ROOT/cargo-tree.log\"\n  exit 0\nfi\nprintf 'unexpected_cargo_subcommand=%s\\n' \"${1-}\" >> \"$DASOBJECTSTORE_F05_ATTEMPT_ROOT/cargo-tree.log\"\nexit 71\n",
    );
    fs::set_permissions(&staged_cargo, fs::Permissions::from_mode(0o755))
        .expect("make staged Cargo executable");
    write_f05_manifest(&sealed);

    let missing = temp.join("missing-git-input-closure");
    copy_tree(&sealed, &missing);
    fs::set_permissions(
        missing.join("cargo-home/git/checkouts/prosopikon-739f7520363f0e4d"),
        fs::Permissions::from_mode(0o755),
    )
    .expect("make copied checkout parent writable");
    fs::remove_dir_all(
        missing.join("cargo-home/git/checkouts/prosopikon-739f7520363f0e4d/f097492"),
    )
    .expect("remove separately copied Prosopikon checkout");
    write_f05_manifest(&missing);

    let attempt = temp.join("attempt");
    let diagnostic = temp.join("diagnostic");
    let cache = temp.join("leased-cache");
    fs::create_dir(&attempt).expect("create fresh external attempt root");
    fs::create_dir(&diagnostic).expect("create fresh external diagnostic root");
    fs::create_dir(&cache).expect("create fresh leased cache root");
    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../packaging/debian/run-plugin-process-package-attempt.sh");
    let status = Command::new("bash")
        .arg(&script)
        .args(["--sealed-root"])
        .arg(&missing)
        .args(["--attempt-root"])
        .arg(&attempt)
        .args(["--diagnostic-root"])
        .arg(&diagnostic)
        .args(["--stage-cache-root"])
        .arg(&cache)
        .arg("--preflight-only")
        .status()
        .expect("run missing Git-input preflight");

    assert_eq!(
        status.code(),
        Some(72),
        "the same locked offline cargo-tree preflight must expose the missing staged Git input"
    );
    assert!(
        fs::read_to_string(attempt.join("cargo-tree.log"))
            .expect("read Cargo-tree denial receipt")
            .contains("missing_git_input=prosopikon"),
        "the denial must occur in cargo tree, after the copied cache is selected and before compilation"
    );
    assert!(
        !attempt.join("target").exists()
            && !attempt.join("output").exists()
            && !diagnostic.join("package-success-receipt").exists(),
        "a missing staged Git input must deny before compiler or package output exists"
    );
    assert!(
        fs::read_to_string(diagnostic.join("preflight.log"))
            .expect("read missing Git-input terminal receipt")
            .contains("terminal_exit_code=72"),
        "the preflight terminal receipt must retain Cargo-tree's real denial status"
    );

    assert!(
        Command::new("chmod")
            .args(["-R", "u+w"])
            .arg(&temp)
            .status()
            .expect("restore disposable missing Git-input fixture permissions")
            .success(),
        "test cleanup may only restore permissions on its disposable fixture"
    );
    fs::remove_dir_all(temp).expect("remove missing Git-input fixture root");
}

#[cfg(target_os = "linux")]
#[test]
fn external_attempt_harness_binds_writable_tmp_for_staged_trunk() {
    use std::os::unix::fs::PermissionsExt;

    let temp = fs::canonicalize(std::env::temp_dir())
        .expect("canonical temporary directory")
        .join(format!(
            "dasobjectstore-plugin-process-trunk-tmp-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time")
                .as_nanos()
        ));
    fs::create_dir(&temp).expect("temporary harness root");
    let sealed = staged_fixture(&temp);
    let attempt = temp.join("attempt");
    let diagnostic = temp.join("diagnostic");
    fs::create_dir(&attempt).expect("create external attempt root");
    fs::create_dir(&diagnostic).expect("create external diagnostic root");
    let staged_cargo = sealed.join("toolchain/bin/cargo");
    write(
        &staged_cargo,
        "#!/bin/sh\nset -eu\nmkdir -p \"$CARGO_HOME/.global-cache\"\nprintf 'attempt-only-cargo-cache\\n' > \"$CARGO_HOME/.global-cache/cargo-sentinel\"\nexit 0\n",
    );
    fs::set_permissions(&staged_cargo, fs::Permissions::from_mode(0o755))
        .expect("make staged Cargo executable");

    let network_download = attempt.join("network-download");
    let staged_curl = sealed.join("network-denied-bin/curl");
    write(
        &staged_curl,
        &format!(
            "#!/bin/sh\nprintf 'network_request=UNEXPECTED\\n' > \"{}\"\nexit 75\n",
            network_download.display()
        ),
    );
    fs::set_permissions(&staged_curl, fs::Permissions::from_mode(0o755))
        .expect("make staged downloader denial executable");

    let staged_trunk = sealed.join("toolchain/bin/trunk");
    write(
        &staged_trunk,
        &format!(
            "#!/bin/sh\nset -eu\ntest \"$TMPDIR\" = \"/var/tmp/tmp\"\ntest \"$TMP\" = \"$TMPDIR\"\ntest \"$TEMP\" = \"$TMPDIR\"\ntest \"$XDG_CACHE_HOME\" = \"/var/tmp/xdg-cache\"\ntest -d \"$TMPDIR\" && test -w \"$TMPDIR\"\nbindgen=\"$XDG_CACHE_HOME/trunk/wasm-bindgen-0.2.128/wasm-bindgen\"\nwasm_opt=\"$XDG_CACHE_HOME/trunk/wasm-opt-version_123/bin/wasm-opt\"\nif ! test -x \"$bindgen\" || ! test -x \"$wasm_opt\"; then\n  curl https://example.invalid/trunk-tool\nfi\nprintf 'tmpdir=%s\\ntmp=%s\\ntemp=%s\\nxdg_cache=%s\\ncached_wasm_bindgen=%s\\ncached_wasm_opt=%s\\ndownloader=NOT_INVOKED\\n' \"$TMPDIR\" \"$TMP\" \"$TEMP\" \"$XDG_CACHE_HOME\" \"$bindgen\" \"$wasm_opt\" > \"$TMPDIR/trunk-env.log\"\n: > \"$TMPDIR/trunk-temp-proof\"\nif test -w /tmp; then\n  printf 'host_tmp_writable=UNEXPECTED\\n' >> \"$TMPDIR/trunk-env.log\"\n  exit 74\nfi\nprintf 'host_tmp_writable=DENIED\\n' >> \"$TMPDIR/trunk-env.log\"\nexit 73\n",
        ),
    );
    fs::set_permissions(&staged_trunk, fs::Permissions::from_mode(0o755))
        .expect("make staged Trunk executable");
    let staged_prepare = sealed.join("source/packaging/web/prepare-web-dist.sh");
    write(&staged_prepare, PREPARE_WEB_DIST);
    fs::set_permissions(&staged_prepare, fs::Permissions::from_mode(0o755))
        .expect("make staged web preparer executable");
    fs::create_dir_all(sealed.join("source/crates/dasobjectstore-gui-web"))
        .expect("create staged web root");
    write_f05_manifest(&sealed);
    let manifest = fs::read(sealed.join("f05-inputs.sha256")).expect("read sealed manifest");

    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../packaging/debian/run-plugin-process-package-attempt.sh");
    let result = Command::new("bash")
        .arg(&script)
        .args(["--sealed-root"])
        .arg(&sealed)
        .args(["--attempt-root"])
        .arg(&attempt)
        .args(["--diagnostic-root"])
        .arg(&diagnostic)
        .output()
        .expect("run staged Trunk temporary-storage fixture");
    assert!(
        !result.status.success(),
        "fake Trunk must stop the fixture after its proof: {}",
        String::from_utf8_lossy(&result.stderr)
    );

    let trunk_environment = fs::read_to_string(attempt.join("tmp/trunk-env.log"))
        .expect("read staged Trunk temporary-storage evidence");
    let expected_tmp = "/var/tmp/tmp";
    let expected_xdg_cache = "/var/tmp/xdg-cache";
    assert!(
        trunk_environment.contains(&format!("tmpdir={expected_tmp}"))
            && trunk_environment.contains(&format!("tmp={expected_tmp}"))
            && trunk_environment.contains(&format!("temp={expected_tmp}"))
            && trunk_environment.contains(&format!("xdg_cache={expected_xdg_cache}"))
            && trunk_environment.contains(&format!(
                "cached_wasm_bindgen={expected_xdg_cache}/trunk/wasm-bindgen-0.2.128/wasm-bindgen"
            ))
            && trunk_environment.contains(&format!(
                "cached_wasm_opt={expected_xdg_cache}/trunk/wasm-opt-version_123/bin/wasm-opt"
            ))
            && trunk_environment.contains("downloader=NOT_INVOKED")
            && trunk_environment.contains("host_tmp_writable=DENIED"),
        "staged Trunk must receive only the external temporary and hash-verified tool cache directories"
    );
    assert!(
        attempt.join("tmp/trunk-temp-proof").is_file()
            && attempt
                .join("xdg-cache/trunk/wasm-bindgen-0.2.128/wasm-bindgen")
                .is_file()
            && attempt
                .join("xdg-cache/trunk/wasm-opt-version_123/bin/wasm-opt")
                .is_file()
            && !network_download.exists()
            && !sealed.join("tmp").exists(),
        "temporary and Trunk-cache writes must remain under the external attempt root without downloader fallback"
    );
    assert!(
        attempt
            .join("cargo-home/.global-cache/cargo-sentinel")
            .is_file()
            && !attempt
                .join("closure/cargo-home/.global-cache/cargo-sentinel")
                .exists(),
        "fake Cargo must write only to the mutable attempt-local Cargo home"
    );
    assert!(
        verify_f05_manifest(&attempt.join("closure")),
        "the copied closure manifest must remain valid after fake Cargo and Trunk run"
    );
    assert_eq!(
        fs::read(sealed.join("f05-inputs.sha256")).expect("read sealed manifest after attempt"),
        manifest,
        "fake Trunk fixture must not alter the sealed inputs"
    );
    Command::new("chmod")
        .args(["-R", "u+w"])
        .arg(&temp)
        .status()
        .expect("unseal temporary harness root before cleanup");
    fs::remove_dir_all(temp).expect("remove temporary harness root");
}

#[cfg(target_os = "linux")]
#[test]
#[ignore = "requires DASOBJECTSTORE_F05_REAL_STAGED_CLOSURE and performs a real staged Trunk build"]
fn real_staged_trunk_uses_hydrated_xdg_cache_without_a_downloader() {
    use std::os::unix::fs::PermissionsExt;

    let source_closure = PathBuf::from(
        std::env::var("DASOBJECTSTORE_F05_REAL_STAGED_CLOSURE")
            .expect("real Trunk regression requires DASOBJECTSTORE_F05_REAL_STAGED_CLOSURE"),
    );
    assert!(
        source_closure.is_absolute()
            && source_closure.is_dir()
            && !fs::symlink_metadata(&source_closure)
                .expect("inspect real staged closure")
                .file_type()
                .is_symlink(),
        "real Trunk regression requires an absolute physical staged closure"
    );
    assert!(
        Command::new("shasum")
            .args(["-a", "256", "-c", "f05-inputs.sha256"])
            .current_dir(&source_closure)
            .status()
            .expect("verify real staged closure before fixture")
            .success(),
        "real staged closure must be intact before the Bubblewrap fixture"
    );

    let real_trunk = source_closure.join("toolchain/bin/trunk");
    let real_trunk_version = Command::new(&real_trunk)
        .arg("--version")
        .output()
        .expect("run staged Trunk version check");
    assert!(
        real_trunk_version.status.success()
            && String::from_utf8_lossy(&real_trunk_version.stdout).contains("trunk 0.21.14"),
        "fixture requires the declared real staged Trunk 0.21.14"
    );

    let temp = fs::canonicalize(std::env::temp_dir())
        .expect("canonical temporary directory")
        .join(format!(
            "dasobjectstore-real-staged-trunk-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time")
                .as_nanos()
        ));
    fs::create_dir(&temp).expect("create real Trunk fixture root");
    let attempt = temp.join("attempt");
    fs::create_dir(&attempt).expect("create real Trunk external attempt root");
    let copied_closure = attempt.join("closure");
    let copy_status = Command::new("cp")
        .args(["-a"])
        .arg(&source_closure)
        .arg(&copied_closure)
        .status()
        .expect("copy real staged closure into the external attempt root");
    assert!(copy_status.success(), "copy real staged closure");
    let writable_copy = Command::new("chmod")
        .args(["-R", "u+w"])
        .arg(&copied_closure)
        .status()
        .expect("make only the copied closure writable");
    assert!(writable_copy.success(), "make copied closure writable");

    let copied_prepare = copied_closure.join("source/packaging/web/prepare-web-dist.sh");
    fs::write(&copied_prepare, PREPARE_WEB_DIST)
        .expect("install tested web preparer in copied closure");
    fs::set_permissions(&copied_prepare, fs::Permissions::from_mode(0o755))
        .expect("make copied web preparer executable");
    let copied_vendor_config = copied_closure.join("source/.cargo/f05-vendor-config.toml");
    let copied_vendor_path = copied_closure.join("source/vendor");
    let vendor_config =
        fs::read_to_string(&copied_vendor_config).expect("read copied vendor config");
    let rewritten_vendor_config = vendor_config
        .lines()
        .map(|line| {
            if line.starts_with("directory = ") {
                format!("directory = \"{}\"", copied_vendor_path.display())
            } else {
                line.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(
        &copied_vendor_config,
        format!("{rewritten_vendor_config}\n"),
    )
    .expect("bind copied vendor config to copied source");
    let copied_manifest = Command::new("bash")
        .arg("-ceu")
        .arg(
            "cd \"$1\"\nfind . -type f ! -name f05-inputs.sha256 -print | LC_ALL=C sort | while IFS= read -r input; do\n  printf '%s  %s\\n' \"$(shasum -a 256 \"$input\" | awk '{print $1}')\" \"${input#./}\"\ndone > f05-inputs.sha256",
        )
        .arg("fixture")
        .arg(&copied_closure)
        .status()
        .expect("rebind copied closure manifest to the tested preparer");
    assert!(copied_manifest.success(), "rebind copied closure manifest");
    fs::create_dir_all(attempt.join("tmp")).expect("create external temporary directory");

    let bwrap_script = "test ! -w /tmp\ntest -d \"$DASOBJECTSTORE_F05_ATTEMPT_ROOT/tmp\" && test -w \"$DASOBJECTSTORE_F05_ATTEMPT_ROOT/tmp\"\nexec bash \"$DASOBJECTSTORE_F05_STAGED_CLOSURE_ROOT/source/packaging/web/prepare-web-dist.sh\"";
    let bwrap_output = Command::new("/usr/bin/bwrap")
        .args(["--unshare-net", "--ro-bind", "/", "/", "--bind"])
        .arg(&attempt)
        .arg(&attempt)
        .args(["--proc", "/proc", "--dev", "/dev"])
        .args(["--setenv", "DASOBJECTSTORE_F05_STAGED_CLOSURE_ROOT"])
        .arg(&copied_closure)
        .args(["--setenv", "DASOBJECTSTORE_F05_ATTEMPT_ROOT"])
        .arg(&attempt)
        .args(["--", "/usr/bin/bash", "-ceu", bwrap_script])
        .output()
        .expect("run real staged Trunk under Bubblewrap");
    let bwrap_log = temp.join("real-staged-trunk-bwrap.log");
    let mut combined_log = bwrap_output.stdout;
    combined_log.extend_from_slice(&bwrap_output.stderr);
    fs::write(&bwrap_log, &combined_log).expect("retain Bubblewrap fixture log");
    assert!(
        bwrap_output.status.success(),
        "real staged Trunk must build from the hydrated cache under network-denied Bubblewrap: {}",
        String::from_utf8_lossy(&combined_log)
    );
    let bwrap_log_text = String::from_utf8_lossy(&combined_log);
    assert!(
        !bwrap_log_text
            .to_ascii_lowercase()
            .contains("downloading wasm-bindgen"),
        "hydrated staged wasm-bindgen cache must avoid a downloader request"
    );
    assert!(
        attempt
            .join("xdg-cache/trunk/wasm-bindgen-0.2.128/wasm-bindgen")
            .is_file()
            && attempt
                .join("xdg-cache/trunk/wasm-opt-version_123/bin/wasm-opt")
                .is_file()
            && attempt
                .join("closure/source/crates/dasobjectstore-gui-web/dist/index.html")
                .is_file(),
        "real staged Trunk must use hydrated external cache and write web output only in the copied closure"
    );
    assert!(
        Command::new("shasum")
            .args(["-a", "256", "-c", "f05-inputs.sha256"])
            .current_dir(&source_closure)
            .status()
            .expect("reverify real staged closure after fixture")
            .success(),
        "real staged Trunk fixture must leave the sealed source closure immutable"
    );
    fs::remove_dir_all(temp).expect("remove external real Trunk fixture root");
}

#[test]
fn plugin_process_promotion_is_same_artifact_fail_closed_and_never_builds() {
    for required in [
        "mnemosyne.dasobjectstore.plugin-process-package-provenance.v1",
        "promotion-receipt.json",
        "source DEB changed during promotion",
        "mv \"$stage_dir\" \"$final_dir\"",
    ] {
        assert!(
            PROMOTE.contains(required),
            "promotion helper is missing required contract: {required}"
        );
    }
    let temp = fs::canonicalize(std::env::temp_dir())
        .expect("canonical temporary directory")
        .join(format!(
            "dasobjectstore-plugin-process-promotion-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time")
                .as_nanos()
        ));
    fs::create_dir(&temp).expect("temporary promotion root");
    let inputs = temp.join("inputs");
    let destination = temp.join("destination");
    let markers = temp.join("markers");
    fs::create_dir_all(&inputs).expect("promotion input root");
    fs::create_dir(&destination).expect("caller-owned destination");
    fs::create_dir(&markers).expect("forbidden executable markers");

    let source = inputs.join("dasobjectstore-plugin-process_0.186.12_amd64.deb");
    write(&source, "fixture plugin-process DEB bytes\n");
    let digest = sha256(&source);
    let provenance = inputs.join("plugin-process.provenance.json");
    let revision = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let provenance_json = |package_digest: &str, version: &str, source_revision: &str| {
        format!(
            "{{\"schema\":\"mnemosyne.dasobjectstore.plugin-process-package-provenance.v1\",\"package_name\":\"dasobjectstore-plugin-process\",\"package_version\":\"{version}\",\"architecture\":\"amd64\",\"source_revision\":\"{source_revision}\",\"package_sha256\":\"{package_digest}\"}}\n"
        )
    };
    write(&provenance, &provenance_json(&digest, "0.186.12", revision));

    let marker_log = temp.join("forbidden-invocation.log");
    for name in [
        "cargo",
        "rustc",
        "trunk",
        "build-plugin-process-deb.sh",
        "dpkg",
        "systemctl",
        "apt",
        "apt-get",
    ] {
        write(
            markers.join(name),
            &format!(
                "#!/bin/sh\nprintf '%s\\n' \"{name}\" >> \"{}\"\nexit 97\n",
                marker_log.display()
            ),
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(markers.join(name), fs::Permissions::from_mode(0o755))
                .expect("make forbidden invocation marker executable");
        }
    }

    // Alma Linux supplies sha256sum but not shasum.  A successful promotion
    // must therefore use the portable primary without falling through to this
    // deliberately failing compatibility marker.
    #[cfg(target_os = "linux")]
    write(
        markers.join("shasum"),
        &format!(
            "#!/bin/sh\nprintf '%s\\n' shasum >> \"{}\"\nexit 97\n",
            marker_log.display()
        ),
    );
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(markers.join("shasum"), fs::Permissions::from_mode(0o755))
            .expect("make forbidden shasum marker executable");
    }

    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../packaging/debian/promote-plugin-process-package.sh");
    let run = |deb: &Path, sidecar: &Path, expected: &str, destination: &Path| {
        Command::new("bash")
            .arg(&script)
            .args(["--source-deb"])
            .arg(deb)
            .args(["--provenance"])
            .arg(sidecar)
            .args(["--expected-sha256", expected, "--destination-dir"])
            .arg(destination)
            .args([
                "--source-revision",
                revision,
                "--package-version",
                "0.186.12",
                "--architecture",
                "amd64",
            ])
            .env(
                "PATH",
                format!(
                    "{}:{}",
                    markers.display(),
                    std::env::var("PATH").expect("PATH")
                ),
            )
            .output()
            .expect("run source-only promotion helper")
    };

    let accepted = run(&source, &provenance, &digest, &destination);
    assert!(
        accepted.status.success(),
        "matching fixture promotion must succeed: {}",
        String::from_utf8_lossy(&accepted.stderr)
    );
    let promotion = destination.join(format!(
        "dasobjectstore-plugin-process-0.186.12-amd64-{digest}"
    ));
    let promoted_deb = promotion.join("dasobjectstore-plugin-process_0.186.12_amd64.deb");
    assert_eq!(
        sha256(&promoted_deb),
        digest,
        "promotion retains exact DEB bytes"
    );
    let receipt = promotion.join("promotion-receipt.json");
    let receipt_check = Command::new("jq")
        .args([
            "-e",
            &format!(
                ".source_deb_sha256 == \"{digest}\" and .destination_deb_sha256 == \"{digest}\" and .source_revision == \"{revision}\" and .package_version == \"0.186.12\" and .architecture == \"amd64\""
            ),
        ])
        .arg(&receipt)
        .status()
        .expect("validate promotion receipt");
    assert!(
        receipt_check.success(),
        "receipt must bind the promoted bytes and tuple"
    );
    assert!(
        !marker_log.exists(),
        "success path must not invoke Cargo, Rustc, Trunk, packaging, installer, or service markers"
    );

    let reject = |name: &str, deb: &Path, sidecar: &Path, expected: &str| {
        let rejected_destination = temp.join(format!("rejected-{name}"));
        fs::create_dir(&rejected_destination).expect("rejected caller destination");
        let result = run(deb, sidecar, expected, &rejected_destination);
        assert!(
            !result.status.success(),
            "promotion must reject {name}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(
            fs::read_dir(&rejected_destination)
                .expect("read rejected destination")
                .next()
                .is_none(),
            "failed {name} promotion must leave no accepted destination or receipt"
        );
    };
    let wrong_digest = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    reject(
        "expected-digest-mismatch",
        &source,
        &provenance,
        wrong_digest,
    );
    let bad_provenance = inputs.join("bad-provenance.json");
    write(
        &bad_provenance,
        &provenance_json(wrong_digest, "0.186.12", revision),
    );
    reject(
        "provenance-digest-mismatch",
        &source,
        &bad_provenance,
        &digest,
    );
    write(&bad_provenance, "not json\n");
    reject("malformed-provenance", &source, &bad_provenance, &digest);
    write(
        &bad_provenance,
        &provenance_json(&digest, "0.186.13", revision),
    );
    reject("wrong-provenance-tuple", &source, &bad_provenance, &digest);
    reject(
        "missing-source",
        &inputs.join("missing.deb"),
        &provenance,
        &digest,
    );
    reject(
        "missing-sidecar",
        &source,
        &inputs.join("missing.json"),
        &digest,
    );

    let tampered = inputs.join("tampered.deb");
    fs::copy(&source, &tampered).expect("copy tamper fixture");
    write(&tampered, "tampered after provenance verification\n");
    reject("tampered-source", &tampered, &provenance, &digest);
    #[cfg(unix)]
    {
        let symlink = inputs.join("source-link.deb");
        std::os::unix::fs::symlink(&source, &symlink).expect("source symlink fixture");
        reject("symlink-source", &symlink, &provenance, &digest);

        let escape = temp.join("destination-escape");
        let symlink_parent = temp.join("destination-link");
        fs::create_dir(&escape).expect("destination symlink escape target");
        std::os::unix::fs::symlink(&escape, &symlink_parent).expect("destination symlink fixture");
        let escaped = run(
            &source,
            &provenance,
            &digest,
            &symlink_parent.join("destination"),
        );
        assert!(
            !escaped.status.success(),
            "promotion must reject a destination under a symlinked ancestor"
        );
        assert!(
            !escape.join("destination").exists(),
            "symlinked destination ancestry must not receive a promotion output"
        );
    }
    assert!(
        !marker_log.exists(),
        "rejection paths must not invoke Cargo, Rustc, Trunk, builders, installers, or services"
    );
    fs::remove_dir_all(temp).expect("remove promotion fixture root");
}

fn write(path: impl AsRef<Path>, contents: &str) {
    let path = path.as_ref();
    fs::create_dir_all(path.parent().expect("fixture parent")).expect("create fixture parent");
    fs::write(path, contents).expect("write fixture input");
}

fn executable(path: impl AsRef<Path>, version: &str) {
    let path = path.as_ref();
    write(
        path,
        &format!(
            "#!/bin/sh\n[ \"${{1-}}\" = --version ] && {{ echo {version}; exit 0; }}\nexit 0\n"
        ),
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755))
            .expect("make staged tool executable");
    }
}

fn sha256(path: &Path) -> String {
    let output = Command::new("sha256sum")
        .arg(path)
        .output()
        .or_else(|_| {
            Command::new("shasum")
                .args(["-a", "256"])
                .arg(path)
                .output()
        })
        .expect("hash fixture input");
    assert!(output.status.success(), "hash fixture input");
    String::from_utf8(output.stdout)
        .expect("hash output")
        .split_whitespace()
        .next()
        .expect("hash value")
        .to_owned()
}

fn verify_f05_manifest(root: &Path) -> bool {
    Command::new("bash")
        .args([
            "-ceu",
            "if command -v sha256sum >/dev/null 2>&1; then sha256sum -c f05-inputs.sha256; elif command -v shasum >/dev/null 2>&1; then shasum -a 256 -c f05-inputs.sha256; else exit 127; fi",
        ])
        .current_dir(root)
        .status()
        .expect("verify fixture manifest")
        .success()
}

fn copy_tree(from: &Path, to: &Path) {
    let status = Command::new("cp")
        .args(["-a"])
        .arg(from)
        .arg(to)
        .status()
        .expect("copy staged fixture");
    assert!(status.success(), "copy staged fixture");
}

fn write_f05_manifest(stage: &Path) {
    let mut files = Vec::new();
    fn collect(root: &Path, current: &Path, files: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(current).expect("read staged fixture directory") {
            let path = entry.expect("staged fixture entry").path();
            if path.is_dir() {
                collect(root, &path, files);
            } else if path
                .file_name()
                .is_some_and(|name| name != "f05-inputs.sha256")
            {
                files.push(
                    path.strip_prefix(root)
                        .expect("relative staged input")
                        .to_owned(),
                );
            }
        }
    }
    collect(stage, stage, &mut files);
    files.sort();
    let manifest = files
        .iter()
        .map(|relative| format!("{}  {}", sha256(&stage.join(relative)), relative.display()))
        .collect::<Vec<_>>()
        .join("\n");
    write(stage.join("f05-inputs.sha256"), &(manifest + "\n"));
}

fn staged_fixture(root: &Path) -> PathBuf {
    let stage = root.join("closure");
    let source = stage.join("source");
    let staged_runner = source.join("packaging/debian/run-plugin-process-package-attempt.sh");
    write(&staged_runner, ATTEMPT);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&staged_runner, fs::Permissions::from_mode(0o755))
            .expect("make staged package-attempt runner executable");
    }
    write(
        source.join("Cargo.toml"),
        "[workspace]\nresolver = \"2\"\n\n[workspace.package]\nversion = \"0.186.17\"\n\n[workspace.dependencies]\nprosopikon-core = { git = \"https://github.com/sagrudd/prosopikon.git\", rev = \"f09749273ef382c1b42bf04a77d96189dd7361b3\" }\nprosopikon-yew = { git = \"https://github.com/sagrudd/prosopikon.git\", rev = \"f09749273ef382c1b42bf04a77d96189dd7361b3\" }\n",
    );
    write(
        source.join("Cargo.lock"),
        "version = 4\nsource = \"git+https://github.com/sagrudd/pistis.git?rev=14e481497d3838d3310df3b0a21232f5d01d6f9f#14e481497d3838d3310df3b0a21232f5d01d6f9f\"\nsource = \"git+https://github.com/sagrudd/prosopikon.git?rev=f09749273ef382c1b42bf04a77d96189dd7361b3#f09749273ef382c1b42bf04a77d96189dd7361b3\"\nsource = \"git+https://github.com/sagrudd/proxenos.git?rev=d4c3054fb7d88c9f718d2987ec19bf7bc444d391#d4c3054fb7d88c9f718d2987ec19bf7bc444d391\"\nsource = \"git+https://github.com/sagrudd/thesaurophylax.git?rev=0bfb16857d135d2830de2cf53d245b68ed2d051f#0bfb16857d135d2830de2cf53d245b68ed2d051f\"\n",
    );
    write(
        source.join(".cargo/f05-vendor-config.toml"),
        &format!(
            "[source.vendored-sources]\ndirectory = \"{}\"\n",
            source.join("vendor").display()
        ),
    );
    write(source.join("vendor/fixture.crate"), "vendored dependency\n");
    write(
        stage.join("inputs/component-candidate-input.toml"),
        "source_revision = \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"\ntoolchain_image = \"docker.io/library/rust@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"\ntoolchain_image_sha256 = \"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"\n",
    );
    write(
        stage.join("inputs/source-tree"),
        "revision=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n",
    );
    write(
        stage.join("inputs/compiled-dependency-witness.json"),
        "{\"source_revision\": \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\", \"dependencies\":[]}\n",
    );
    write(
        stage.join("inputs/toolchain-input-admission-receipt.toml"),
        "source_revision = \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"\nworkspace_version = \"0.186.17\"\ntoolchain_image = \"docker.io/library/rust@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"\ntoolchain_image_sha256 = \"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"\ninventory_sha256 = \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"\nreusable_tool_provenance_inventory_sha256 = \"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\"\n",
    );
    write(
        stage.join("inputs/package-recipe.json"),
        "{\"package\":\"fixture\"}\n",
    );
    executable(stage.join("toolchain/bin/cargo"), "cargo fixture 1.0.0");
    executable(stage.join("toolchain/bin/rustc"), "rustc fixture 1.0.0");
    executable(stage.join("toolchain/bin/trunk"), "trunk fixture 1.0.0");
    executable(
        stage.join("toolchain/trunk-tools/wasm-bindgen-0.2.128/wasm-bindgen"),
        "wasm-bindgen 0.2.128",
    );
    executable(
        stage.join("toolchain/trunk-tools/wasm-opt-version_123/wasm-opt"),
        "wasm-opt version_123",
    );
    write(
        stage.join("toolchain/lib/rustlib/wasm32-unknown-unknown/libfixture.rlib"),
        "wasm target\n",
    );
    fs::create_dir_all(stage.join("staging-home")).expect("create staged home");
    write(
        stage.join("cargo-home/config.toml"),
        "[net]\noffline = true\ngit-fetch-with-cli = false\n",
    );
    for checkout in [
        "pistis-13d5c72a63ff6278/14e4814",
        "prosopikon-739f7520363f0e4d/f097492",
        "proxenos-10a0a1d74c5551fd/d4c3054",
        "thesaurophylax-08d7bd2129966817/0bfb168",
    ] {
        write(
            stage
                .join("cargo-home/git/checkouts")
                .join(checkout)
                .join("fixture"),
            "immutable Git checkout fixture\n",
        );
    }
    fs::create_dir_all(stage.join("network-denied-bin")).expect("create network denial path");
    write(stage.join("web/index.html"), "<html></html>\n");
    write(stage.join("server"), "server fixture\n");
    write(stage.join("artifact.deb"), "package fixture\n");
    write(
        source.join("packaging/linux/opt/dasobjectstore/plugin-process-descriptor.json"),
        "{\"schema\":\"fixture\"}\n",
    );
    write_f05_manifest(&stage);
    stage
}

#[test]
fn offline_git_cache_contract_binds_every_locked_source_before_resolution() {
    for required in [
        "--stage-closure-git-inputs",
        "git-inputs.sha256",
        "git-input-admission-receipt.toml",
        "git input stage rejects altered input bytes",
        "git input stage requires immutable physical non-symlink inputs",
        "git input stage rejects a mismatched $name revision",
        "git input stage rejects a substituted $name checkout",
        "git input stage rejects an inventory not bound by the admission receipt",
        "command -v sha256sum",
        "verify_sha256_manifest",
        "network-denied-bin/shasum",
        "pistis|https://github.com/sagrudd/pistis.git|14e481497d3838d3310df3b0a21232f5d01d6f9f",
        "prosopikon|https://github.com/sagrudd/prosopikon.git|f09749273ef382c1b42bf04a77d96189dd7361b3",
        "proxenos|https://github.com/sagrudd/proxenos.git|d4c3054fb7d88c9f718d2987ec19bf7bc444d391",
        "thesaurophylax|https://github.com/sagrudd/thesaurophylax.git|0bfb16857d135d2830de2cf53d245b68ed2d051f",
        "copied_preflight_config",
        "[source.crates-io]",
        "cargo\" tree --manifest-path \"$copied_manifest\" --offline --config \"$copied_preflight_config\" --locked --target x86_64-unknown-linux-gnu -p dasobjectstore-cli --edges normal,build",
        "offline_locked_resolution=PASS",
    ] {
        assert!(
            ATTEMPT.contains(required),
            "missing offline Git-cache contract: {required}"
        );
    }
}

#[cfg(target_os = "linux")]
fn tree_sha256(path: &Path) -> String {
    let output = Command::new("bash")
        .args([
            "-c",
            "cd \"$1\" && find . -type f -print0 | LC_ALL=C sort -z | xargs -0 -r sh -c 'for file; do if command -v sha256sum >/dev/null 2>&1; then sha256sum \"$file\"; else shasum -a 256 \"$file\"; fi; done' sh | { if command -v sha256sum >/dev/null 2>&1; then sha256sum; else shasum -a 256; fi; } | awk '{print $1}'",
            "fixture",
        ])
        .arg(path)
        .output()
        .expect("hash fixture tree");
    assert!(output.status.success(), "hash fixture tree");
    String::from_utf8(output.stdout)
        .expect("fixture tree hash output")
        .trim()
        .to_owned()
}

#[cfg(target_os = "linux")]
fn write_git_input_manifest(input: &Path) {
    write_f05_manifest(input);
    fs::rename(
        input.join("f05-inputs.sha256"),
        input.join("git-inputs.sha256"),
    )
    .expect("name disposable Git-input manifest");
}

#[cfg(target_os = "linux")]
fn immutable_git_input_fixture(root: &Path, sealed: &Path) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let input = root.join("git-inputs");
    fs::create_dir(&input).expect("create disposable Git-input root");
    write(
        input.join("cargo-home/config.toml"),
        "[net]\noffline = true\ngit-fetch-with-cli = false\n",
    );
    let sources = [
        (
            "pistis",
            "14e481497d3838d3310df3b0a21232f5d01d6f9f",
            "pistis-13d5c72a63ff6278",
            "14e4814",
        ),
        (
            "prosopikon",
            "f09749273ef382c1b42bf04a77d96189dd7361b3",
            "prosopikon-739f7520363f0e4d",
            "f097492",
        ),
        (
            "proxenos",
            "d4c3054fb7d88c9f718d2987ec19bf7bc444d391",
            "proxenos-10a0a1d74c5551fd",
            "d4c3054",
        ),
        (
            "thesaurophylax",
            "0bfb16857d135d2830de2cf53d245b68ed2d051f",
            "thesaurophylax-08d7bd2129966817",
            "0bfb168",
        ),
    ];
    let mut inventory = String::new();
    for (name, revision, cache_name, short) in sources {
        let checkout = input.join(format!("cargo-home/git/checkouts/{cache_name}/{short}"));
        let database = input.join(format!("cargo-home/git/db/{cache_name}"));
        write(checkout.join("fixture-content"), name);
        write(checkout.join(".fixture-head"), revision);
        write(database.join("fixture-cache"), name);
        inventory.push_str(&format!(
            "{name}_url = \"https://github.com/sagrudd/{name}.git\"\n{name}_revision = \"{revision}\"\n{name}_checkout_tree_sha256 = \"{}\"\n{name}_db_tree_sha256 = \"{}\"\n",
            tree_sha256(&checkout),
            tree_sha256(&database),
        ));
    }
    write(input.join("git-inputs.toml"), &inventory);
    write_git_input_manifest(&input);
    write(
        sealed.join("inputs/git-input-admission-receipt.toml"),
        &format!(
            "git_input_inventory_sha256 = \"{}\"\n",
            sha256(&input.join("git-inputs.toml"))
        ),
    );
    assert!(
        Command::new("chmod")
            .args(["-R", "a-w"])
            .arg(&input)
            .status()
            .expect("make disposable Git inputs immutable")
            .success(),
        "Git-input fixture must become immutable before staging"
    );
    fs::set_permissions(&input, fs::Permissions::from_mode(0o555))
        .expect("make disposable Git-input root immutable");
    input
}

#[cfg(target_os = "linux")]
#[test]
fn git_input_stage_admits_complete_bound_cache_and_rejects_missing_or_substituted_inputs() {
    use std::os::unix::fs::PermissionsExt;

    let temp = fs::canonicalize(std::env::temp_dir())
        .expect("canonical temporary directory")
        .join(format!(
            "dasobjectstore-git-input-stage-{}",
            std::process::id()
        ));
    let _ = fs::remove_dir_all(&temp);
    fs::create_dir(&temp).expect("create Git-input fixture root");
    let sealed = staged_fixture(&temp);
    let input = immutable_git_input_fixture(&temp, &sealed);
    fs::remove_dir_all(sealed.join("cargo-home"))
        .expect("remove fixture-only cache before the Git-input stage");
    let fake_bin = temp.join("fake-bin");
    let fake_git = fake_bin.join("git");
    write(
        &fake_git,
        "#!/bin/sh\nif [ \"${1-}\" = -c ]; then shift 2; fi\nif [ \"${1-}\" = -C ]; then head=$2/.fixture-head; shift 2; fi\n[ \"${1-}\" = rev-parse ] && cat \"$head\"\n",
    );
    fs::set_permissions(&fake_git, fs::Permissions::from_mode(0o755))
        .expect("make fake Git executable");
    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../packaging/debian/run-plugin-process-package-attempt.sh");
    let run_stage = |stage: &Path, inputs: &Path| {
        Command::new("bash")
            .arg(&script)
            .args(["--sealed-root"])
            .arg(stage)
            .args(["--stage-closure-git-inputs"])
            .arg(inputs)
            .env("PATH", format!("{}:/usr/bin:/bin", fake_bin.display()))
            .status()
            .expect("run Git-input stage")
    };
    assert!(
        run_stage(&sealed, &input).success(),
        "complete reviewed Git cache must stage"
    );
    assert!(sealed
        .join("cargo-home/git/checkouts/prosopikon-739f7520363f0e4d/f097492")
        .is_dir());

    let missing = temp.join("missing-input");
    copy_tree(&input, &missing);
    Command::new("chmod")
        .args(["-R", "u+w"])
        .arg(&missing)
        .status()
        .expect("make missing fixture writable");
    fs::remove_dir_all(missing.join("cargo-home/git/db/proxenos-10a0a1d74c5551fd"))
        .expect("remove required cache");
    write_git_input_manifest(&missing);
    Command::new("chmod")
        .args(["-R", "a-w"])
        .arg(&missing)
        .status()
        .expect("make missing fixture immutable");
    let missing_stage = staged_fixture(&temp.join("missing-stage"));
    write(
        missing_stage.join("inputs/git-input-admission-receipt.toml"),
        &format!(
            "git_input_inventory_sha256 = \"{}\"\n",
            sha256(&missing.join("git-inputs.toml"))
        ),
    );
    assert!(
        !run_stage(&missing_stage, &missing).success(),
        "missing Git cache must deny before Cargo"
    );

    let substituted = temp.join("substituted-input");
    copy_tree(&input, &substituted);
    Command::new("chmod")
        .args(["-R", "u+w"])
        .arg(&substituted)
        .status()
        .expect("make substituted fixture writable");
    write(
        substituted
            .join("cargo-home/git/checkouts/pistis-13d5c72a63ff6278/14e4814/fixture-content"),
        "substituted",
    );
    let inventory =
        fs::read_to_string(substituted.join("git-inputs.toml")).expect("read inventory");
    let old = tree_sha256(&input.join("cargo-home/git/checkouts/pistis-13d5c72a63ff6278/14e4814"));
    let new =
        tree_sha256(&substituted.join("cargo-home/git/checkouts/pistis-13d5c72a63ff6278/14e4814"));
    write(
        substituted.join("git-inputs.toml"),
        &inventory.replace(&old, &new),
    );
    write_git_input_manifest(&substituted);
    Command::new("chmod")
        .args(["-R", "a-w"])
        .arg(&substituted)
        .status()
        .expect("make substituted fixture immutable");
    let substituted_stage = staged_fixture(&temp.join("substituted-stage"));
    write(
        substituted_stage.join("inputs/git-input-admission-receipt.toml"),
        &format!(
            "git_input_inventory_sha256 = \"{}\"\n",
            sha256(&input.join("git-inputs.toml"))
        ),
    );
    assert!(
        !run_stage(&substituted_stage, &substituted).success(),
        "self-consistent substituted cache must fail the independently bound inventory digest"
    );

    let symlinked = temp.join("symlinked-input");
    copy_tree(&input, &symlinked);
    Command::new("chmod")
        .args(["-R", "u+w"])
        .arg(&symlinked)
        .status()
        .expect("make symlink fixture writable");
    fs::remove_file(symlinked.join("cargo-home/config.toml"))
        .expect("remove disposable cache config");
    std::os::unix::fs::symlink("/etc/passwd", symlinked.join("cargo-home/config.toml"))
        .expect("create disposable input symlink");
    let symlinked_stage = staged_fixture(&temp.join("symlinked-stage"));
    write(
        symlinked_stage.join("inputs/git-input-admission-receipt.toml"),
        &format!(
            "git_input_inventory_sha256 = \"{}\"\n",
            sha256(&input.join("git-inputs.toml"))
        ),
    );
    assert!(
        !run_stage(&symlinked_stage, &symlinked).success(),
        "symlinked Git input must deny before any Cargo operation"
    );

    Command::new("chmod")
        .args(["-R", "u+w"])
        .arg(&temp)
        .status()
        .expect("make disposable Git fixtures removable");
    fs::remove_dir_all(temp).expect("remove Git-input fixture root");
}

fn write_tool_input_manifest(input: &Path) {
    let manifest = input.join("tool-inputs.sha256");
    if manifest.exists() {
        fs::remove_file(&manifest).expect("replace disposable tool-input manifest");
    }
    write_f05_manifest(input);
    fs::rename(input.join("f05-inputs.sha256"), manifest)
        .expect("name disposable tool-input manifest");
}

fn immutable_tool_input_fixture(root: &Path, stage: &Path, native: bool) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let input = root.join("tool-inputs");
    fs::create_dir(&input).expect("create disposable tool-input root");
    copy_tree(&stage.join("toolchain"), &input.join("toolchain"));
    copy_tree(
        &stage.join("network-denied-bin"),
        &input.join("network-denied-bin"),
    );
    copy_tree(&stage.join("staging-home"), &input.join("staging-home"));
    executable(
        input.join("network-denied-bin/git"),
        "printf 'network denied\\n' >&2\nexit 1",
    );
    write(input.join("staging-home/README"), "isolated staging home\n");
    let inventory = input.join("tool-input-inventory.txt");
    write(
        &inventory,
        &format!(
            "source_contract_target=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n\
source_contract_version=0.186.17\n\
[candidate_binding]\n\
source_revision=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n\
workspace_version=0.186.17\n\
[reusable_tool_provenance]\n\
inventory_sha256=bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\n\
[cargo]\n\
sha256={}\n\
version=cargo fixture 1.0.0\n\
[rustc]\n\
sha256={}\n\
version=rustc fixture 1.0.0\n\
[trunk]\n\
sha256={}\n\
version=trunk fixture 1.0.0\n\
[wasm_bindgen]\n\
sha256={}\n\
version=wasm-bindgen 0.2.128\n\
[wasm_opt]\n\
sha256={}\n\
version=wasm-opt version_123\n\
[wasm_sysroot]\n\
tree_sha256={}\n\
[prior_staged_toolchain]\n\
tree_sha256={}\n\
[network_denial]\n\
tree_sha256={}\n\
sha256={}\n\
[staging_home]\n\
tree_sha256={}\n",
            sha256(&input.join("toolchain/bin/cargo")),
            sha256(&input.join("toolchain/bin/rustc")),
            sha256(&input.join("toolchain/bin/trunk")),
            sha256(&input.join("toolchain/trunk-tools/wasm-bindgen-0.2.128/wasm-bindgen"),),
            sha256(&input.join("toolchain/trunk-tools/wasm-opt-version_123/wasm-opt"),),
            tree_sha256(&input.join("toolchain/lib/rustlib/wasm32-unknown-unknown")),
            tree_sha256(&input.join("toolchain")),
            tree_sha256(&input.join("network-denied-bin")),
            sha256(&input.join("network-denied-bin/git")),
            tree_sha256(&input.join("staging-home")),
        ),
    );
    let inventory_sha = sha256(&inventory);
    let mode = if native {
        "native-tool-bundle"
    } else {
        "container-image"
    };
    let tool_identity = if native {
        format!("tool_inventory_sha256 = \"{inventory_sha}\"\n")
    } else {
        "toolchain_image = \"docker.io/library/rust@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"\ntoolchain_image_sha256 = \"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"\n".to_owned()
    };
    write(
        input.join("tool-inputs.toml"),
        &format!(
            "source_revision = \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"\ntoolchain_kind = \"{mode}\"\ninventory_sha256 = \"{inventory_sha}\"\n{tool_identity}"
        ),
    );
    write(
        stage.join("inputs/provenance-tuple.toml"),
        &format!(
            "source_revision = \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"\nworkspace_version = \"0.186.17\"\ntoolchain_kind = \"{mode}\"\nsource_git_tree = \"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\"\n{tool_identity}"
        ),
    );
    if native {
        write(
            stage.join("inputs/component-candidate-input.toml"),
            &format!(
                "source_revision = \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"\ntoolchain_image = \"native-tool-bundle@sha256:{inventory_sha}\"\ntoolchain_image_sha256 = \"sha256:{inventory_sha}\"\n"
            ),
        );
    }
    write_tool_input_manifest(&input);
    assert!(
        Command::new("chmod")
            .args(["-R", "a-w"])
            .arg(&input)
            .status()
            .expect("make disposable tool input immutable")
            .success(),
        "tool-input fixture must become immutable before staging"
    );
    fs::set_permissions(&input, fs::Permissions::from_mode(0o555))
        .expect("make disposable tool-input root immutable");
    input
}

fn seal_tool_stage_identity_inputs(stage: &Path) {
    for relative in [
        "source/packaging/debian/run-plugin-process-package-attempt.sh",
        "inputs/component-candidate-input.toml",
        "inputs/source-tree",
        "inputs/compiled-dependency-witness.json",
        "inputs/provenance-tuple.toml",
    ] {
        assert!(
            Command::new("chmod")
                .args(["a-w"])
                .arg(stage.join(relative))
                .status()
                .expect("seal disposable staged identity input")
                .success(),
            "staged identity input {relative} must be immutable while the inputs directory stays available for generated receipts"
        );
    }
}

#[cfg(target_os = "linux")]
#[test]
fn native_tool_input_stage_emits_only_a_native_receipt_into_a_fresh_output_path() {
    use std::os::unix::fs::PermissionsExt;

    let temp = std::env::temp_dir().join(format!(
        "dasobjectstore-native-tool-input-stage-{}",
        std::process::id()
    ));
    let sealed = staged_fixture(&temp);
    let input = immutable_tool_input_fixture(&temp, &sealed, true);
    fs::set_permissions(&sealed, fs::Permissions::from_mode(0o755))
        .expect("make fresh closure output root writable");
    for path in ["toolchain", "network-denied-bin", "staging-home"] {
        fs::remove_dir_all(sealed.join(path)).expect("remove disposable pre-stage output");
    }
    fs::remove_file(sealed.join("f05-inputs.sha256"))
        .expect("remove disposable pre-stage manifest");
    seal_tool_stage_identity_inputs(&sealed);
    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../packaging/debian/run-plugin-process-package-attempt.sh");
    let result = Command::new("bash")
        .arg(&script)
        .args(["--sealed-root"])
        .arg(&sealed)
        .args(["--stage-closure-toolchain-inputs"])
        .arg(&input)
        .output()
        .expect("run native tool-input stage");
    assert!(
        result.status.success(),
        "native tool-input stage failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    let receipt = fs::read_to_string(sealed.join("inputs/toolchain-input-receipt.toml"))
        .expect("read generated native receipt");
    let inventory = sha256(&input.join("tool-input-inventory.txt"));
    assert!(
        receipt.contains("toolchain_kind = \"native-tool-bundle\"")
            && receipt.contains(&format!("tool_inventory_sha256 = \"{inventory}\""))
            && !receipt.contains("toolchain_image ="),
        "native stage must bind only its reviewed inventory digest"
    );
    assert!(
        sealed.join("toolchain/bin/cargo").is_file()
            && sealed.join("inputs/toolchain-input-receipt.toml").is_file()
            && !sealed.join("output").exists(),
        "fresh closure output paths may receive only staged inputs, never a package output"
    );
    Command::new("chmod")
        .args(["-R", "u+w"])
        .arg(&temp)
        .status()
        .expect("unseal disposable native fixture");
    fs::remove_dir_all(temp).expect("remove disposable native fixture");
}

#[cfg(target_os = "linux")]
#[test]
fn tool_input_stage_binds_immutable_inventory_before_real_preflight() {
    use std::os::unix::fs::PermissionsExt;

    let temp = fs::canonicalize(std::env::temp_dir())
        .expect("canonical temporary directory")
        .join(format!(
            "dasobjectstore-plugin-process-tool-input-stage-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time")
                .as_nanos()
        ));
    fs::create_dir(&temp).expect("create tool-input fixture root");
    let sealed = staged_fixture(&temp);
    let reviewed_input =
        std::env::var_os("DASOBJECTSTORE_REVIEWED_TOOL_INPUT_ROOT").map(PathBuf::from);
    let input = if let Some(root) = &reviewed_input {
        fs::canonicalize(root).expect("canonical reviewed tool-input root")
    } else {
        immutable_tool_input_fixture(&temp, &sealed, false)
    };
    if reviewed_input.is_some() {
        write(
            sealed.join("inputs/component-candidate-input.toml"),
            "source_revision = \"c7b38a244a8a515f865058d09f67e4abe61978cc\"\n\
toolchain_image = \"docker.io/library/rust@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"\n\
toolchain_image_sha256 = \"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"\n",
        );
        write(
            sealed.join("inputs/source-tree"),
            "revision=c7b38a244a8a515f865058d09f67e4abe61978cc\n",
        );
        write(
            sealed.join("inputs/compiled-dependency-witness.json"),
            "{\"source_revision\": \"c7b38a244a8a515f865058d09f67e4abe61978cc\", \"dependencies\":[]}\n",
        );
        write(
            sealed.join("inputs/toolchain-input-admission-receipt.toml"),
            "source_revision = \"c7b38a244a8a515f865058d09f67e4abe61978cc\"\nworkspace_version = \"0.186.17\"\ntoolchain_image = \"docker.io/library/rust@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"\ntoolchain_image_sha256 = \"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"\ninventory_sha256 = \"ec287428c379235f47fd42cdd7f208b0132ba6bbe132dd90f3040d34379b8eb9\"\nreusable_tool_provenance_inventory_sha256 = \"de2fa3ef73e6df2253295488c24833aa8fb4ceb1d9313a27b51dd3fdf309fe4f\"\n",
        );
    }
    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../packaging/debian/run-plugin-process-package-attempt.sh");

    fs::set_permissions(&sealed, fs::Permissions::from_mode(0o755))
        .expect("make disposable stage writable");
    fs::remove_dir_all(sealed.join("toolchain")).expect("remove pre-stage toolchain fixture");
    fs::remove_dir_all(sealed.join("network-denied-bin"))
        .expect("remove pre-stage network fixture");
    fs::remove_dir_all(sealed.join("staging-home")).expect("remove pre-stage home fixture");
    fs::remove_file(sealed.join("f05-inputs.sha256")).expect("remove pre-stage manifest");
    seal_tool_stage_identity_inputs(&sealed);

    let staged = Command::new("bash")
        .arg(&script)
        .args(["--sealed-root"])
        .arg(&sealed)
        .args(["--stage-closure-toolchain-inputs"])
        .arg(&input)
        .output()
        .expect("run source-owned tool-input stage");
    assert!(
        staged.status.success(),
        "tool-input stage failed: {}",
        String::from_utf8_lossy(&staged.stderr)
    );
    let staged_stdout = String::from_utf8(staged.stdout).expect("tool-input stage stdout");
    assert!(
        staged_stdout.contains("closure_stage_toolchain_inputs=PASS"),
        "source-owned stage must retain the immutable-input receipt"
    );
    for tool in ["cargo", "rustc", "trunk", "wasm_bindgen", "wasm_opt"] {
        assert!(
            staged_stdout.contains(&format!("toolchain_probe tool={tool}"))
                && staged_stdout.contains("actual_exit=0"),
            "the real isolated tool admission must accept declared {tool} before closure copy"
        );
    }
    let manifest = fs::read_to_string(sealed.join("f05-inputs.sha256"))
        .expect("read staged tool-input manifest");
    for input in [
        "toolchain/bin/cargo",
        "toolchain/bin/rustc",
        "toolchain/bin/trunk",
        "toolchain/trunk-tools/wasm-bindgen-0.2.128/wasm-bindgen",
        "toolchain/trunk-tools/wasm-opt-version_123/wasm-opt",
        "network-denied-bin/git",
        "staging-home/README",
        "inputs/toolchain-input-receipt.toml",
    ] {
        assert!(manifest.contains(input), "manifest must bind {input}");
    }
    assert!(
        manifest
            .lines()
            .any(|line| line.contains("  toolchain/lib/rustlib/wasm32-unknown-unknown/")),
        "manifest must bind every copied wasm sysroot file"
    );
    let copied_tool_receipt =
        fs::read_to_string(sealed.join("inputs/toolchain-input-receipt.toml"))
            .expect("read copied tool-input receipt");
    assert_eq!(
        copied_tool_receipt
            .lines()
            .find_map(|line| line
                .strip_prefix("inventory_sha256 = \"")
                .and_then(|value| value.strip_suffix('"')))
            .expect("read reviewed inventory binding"),
        sha256(&input.join("tool-input-inventory.txt")),
        "stage must bind the reviewed tool-input inventory"
    );
    assert!(
        fs::read_to_string(sealed.join("inputs/toolchain-input-receipt.toml"))
            .expect("read copied tool-input receipt")
            .contains("workspace_version = \"0.186.17\""),
        "stage must bind the candidate workspace version"
    );

    let attempt = temp.join("attempt");
    let diagnostic = temp.join("diagnostic");
    let cache = temp.join("cache");
    fs::create_dir(&attempt).expect("create tool-stage attempt root");
    fs::create_dir(&diagnostic).expect("create tool-stage diagnostic root");
    fs::create_dir(&cache).expect("create tool-stage cache root");
    assert!(
        Command::new("bash")
            .arg(&script)
            .args(["--sealed-root"])
            .arg(&sealed)
            .args(["--attempt-root"])
            .arg(&attempt)
            .args(["--diagnostic-root"])
            .arg(&diagnostic)
            .args(["--stage-cache-root"])
            .arg(&cache)
            .arg("--preflight-only")
            .status()
            .expect("run tool-input preflight handoff")
            .success(),
        "tool-input stage must hand off to real preflight without tool execution"
    );
    assert!(
        !attempt.join("target").exists() && !attempt.join("output").exists(),
        "tool-input handoff must not compile or package"
    );

    let version_mismatch_stage = temp.join("stage-version-mismatch");
    copy_tree(&sealed, &version_mismatch_stage);
    assert!(
        Command::new("chmod")
            .args(["-R", "u+w"])
            .arg(&version_mismatch_stage)
            .status()
            .expect("make disposable version-mismatch stage writable")
            .success(),
        "version-mismatch stage must be writable only in its disposable fixture"
    );
    fs::remove_dir_all(version_mismatch_stage.join("toolchain"))
        .expect("remove version-mismatch stage toolchain");
    fs::remove_dir_all(version_mismatch_stage.join("network-denied-bin"))
        .expect("remove version-mismatch stage network input");
    fs::remove_dir_all(version_mismatch_stage.join("staging-home"))
        .expect("remove version-mismatch stage home input");
    fs::remove_file(version_mismatch_stage.join("inputs/toolchain-input-receipt.toml"))
        .expect("remove version-mismatch stage receipt");
    fs::remove_file(version_mismatch_stage.join("f05-inputs.sha256"))
        .expect("remove version-mismatch stage manifest");
    write(
        version_mismatch_stage.join("source/Cargo.toml"),
        "[workspace]\nresolver = \"2\"\n\n[workspace.package]\nversion = \"0.186.18\"\n",
    );
    seal_tool_stage_identity_inputs(&version_mismatch_stage);
    let version_mismatch = Command::new("bash")
        .arg(&script)
        .args(["--sealed-root"])
        .arg(&version_mismatch_stage)
        .args(["--stage-closure-toolchain-inputs"])
        .arg(&input)
        .output()
        .expect("run candidate-version mismatch tool-input stage");
    assert!(
        !version_mismatch.status.success()
            && String::from_utf8_lossy(&version_mismatch.stderr)
                .contains("requires matching candidate and expected-tuple revision witnesses"),
        "an expected tuple must reject a mismatched candidate workspace version before a reviewed inventory can be copied"
    );

    for (name, mutate) in [
        ("missing-tool", "rm toolchain/bin/trunk"),
        ("altered-tool", "printf altered >> toolchain/bin/rustc"),
        (
            "symlink-escape",
            "rm toolchain/bin/cargo && ln -s ../../escape toolchain/bin/cargo",
        ),
        (
            "inventory-replacement",
            "printf replacement >> tool-input-inventory.txt",
        ),
        (
            "revision-mismatch",
            "sed -i 's/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb/' tool-inputs.toml",
        ),
        (
            "image-mismatch",
            "sed -i 's/sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb/' tool-inputs.toml",
        ),
    ] {
        let negative_root = temp.join(name);
        copy_tree(&input, &negative_root);
        assert!(
            Command::new("chmod")
                .args(["-R", "u+w"])
                .arg(&negative_root)
                .status()
                .expect("make disposable negative input writable")
                .success(),
            "negative input must be writable only in its disposable fixture"
        );
        if name == "symlink-escape" {
            write(temp.join("escape"), "decoy tool\n");
        }
        let mutation = Command::new("sh")
            .args(["-c", mutate])
            .current_dir(&negative_root)
            .status()
            .expect("mutate disposable negative input");
        assert!(mutation.success(), "prepare {name} negative input");
        if name == "altered-tool"
            || name == "inventory-replacement"
            || name == "revision-mismatch"
            || name == "image-mismatch"
        {
            write_tool_input_manifest(&negative_root);
        }
        assert!(
            Command::new("chmod")
                .args(["-R", "a-w"])
                .arg(&negative_root)
                .status()
                .expect("reseal disposable negative input")
                .success(),
            "negative input must be immutable at stage entry"
        );
        let negative_stage = temp.join(format!("stage-{name}"));
        copy_tree(&sealed, &negative_stage);
        assert!(
            Command::new("chmod")
                .args(["-R", "u+w"])
                .arg(&negative_stage)
                .status()
                .expect("make disposable negative stage writable")
                .success(),
            "negative stage must be writable"
        );
        fs::remove_dir_all(negative_stage.join("toolchain"))
            .expect("remove negative stage toolchain");
        fs::remove_dir_all(negative_stage.join("network-denied-bin"))
            .expect("remove negative stage network input");
        fs::remove_dir_all(negative_stage.join("staging-home"))
            .expect("remove negative stage home input");
        fs::remove_file(negative_stage.join("inputs/toolchain-input-receipt.toml"))
            .expect("remove negative stage receipt");
        fs::remove_file(negative_stage.join("f05-inputs.sha256"))
            .expect("remove negative stage manifest");
        seal_tool_stage_identity_inputs(&negative_stage);
        let denied = Command::new("bash")
            .arg(&script)
            .args(["--sealed-root"])
            .arg(&negative_stage)
            .args(["--stage-closure-toolchain-inputs"])
            .arg(&negative_root)
            .output()
            .expect("run denied tool-input stage");
        assert!(
            !denied.status.success(),
            "tool-input stage must reject {name}"
        );
        let expected = match name {
            "missing-tool" => "requires complete executable tool inputs",
            "altered-tool" => "rejects a substituted rustc input",
            "symlink-escape" => "rejects symlinked inputs",
            "inventory-replacement" => {
                "rejects an inventory receipt not bound to the reviewed document"
            }
            "revision-mismatch" => {
                "requires matching candidate and expected-tuple revision witnesses"
            }
            "image-mismatch" => "rejects a container image not bound to the expected tuple",
            _ => unreachable!("known tool-input negative fixture"),
        };
        assert!(
            String::from_utf8_lossy(&denied.stderr).contains(expected),
            "{name} must fail closed with its specific tool-input contract error"
        );
    }

    for (name, mutate, expected) in [
        (
            "duplicate-mode",
            "printf '\\ntoolchain_kind = \\\"container-image\\\"\\n' >> inputs/provenance-tuple.toml",
            "requires exactly one toolchain_kind",
        ),
        (
            "missing-mode",
            "sed -i '/^toolchain_kind = /d' inputs/provenance-tuple.toml",
            "requires exactly one toolchain_kind",
        ),
        (
            "mixed-native-container",
            "sed -i 's/toolchain_kind = \\\"container-image\\\"/toolchain_kind = \\\"native-tool-bundle\\\"/' inputs/provenance-tuple.toml",
            "native mode requires only one inventory digest and no image fields",
        ),
        (
            "writable-sealed-input",
            "true",
            "requires immutable sealed identity inputs while leaving fresh output roots writable",
        ),
    ] {
        let mode_stage = temp.join(format!("stage-{name}"));
        copy_tree(&sealed, &mode_stage);
        assert!(
            Command::new("chmod")
                .args(["-R", "u+w"])
                .arg(&mode_stage)
                .status()
                .expect("make disposable mode stage writable")
                .success(),
            "mode-stage fixture must be writable before its controlled mutation"
        );
        for path in [
            "toolchain",
            "network-denied-bin",
            "staging-home",
            "inputs/toolchain-input-receipt.toml",
            "f05-inputs.sha256",
        ] {
            let path = mode_stage.join(path);
            if path.is_dir() {
                fs::remove_dir_all(path).expect("remove disposable staged directory");
            } else {
                fs::remove_file(path).expect("remove disposable staged file");
            }
        }
        assert!(
            Command::new("sh")
                .args(["-c", mutate])
                .current_dir(&mode_stage)
                .status()
                .expect("mutate disposable mode fixture")
                .success(),
            "prepare {name} mode fixture"
        );
        if name != "writable-sealed-input" {
            seal_tool_stage_identity_inputs(&mode_stage);
        }
        let denied = Command::new("bash")
            .arg(&script)
            .args(["--sealed-root"])
            .arg(&mode_stage)
            .args(["--stage-closure-toolchain-inputs"])
            .arg(&input)
            .output()
            .expect("run selected-mode denial");
        assert!(
            !denied.status.success() && String::from_utf8_lossy(&denied.stderr).contains(expected),
            "toolchain stage must reject {name}: {}",
            String::from_utf8_lossy(&denied.stderr)
        );
        assert!(
            !mode_stage.join("toolchain").exists(),
            "{name} must deny before copying a toolchain into a closure"
        );
    }

    let unlaunchable_input = temp.join("unlaunchable-tool");
    copy_tree(&input, &unlaunchable_input);
    assert!(
        Command::new("chmod")
            .args(["-R", "u+w"])
            .arg(&unlaunchable_input)
            .status()
            .expect("make unlaunchable input writable")
            .success(),
        "unlaunchable input is writable only in its disposable fixture"
    );
    let old_trunk_sha = sha256(&input.join("toolchain/bin/trunk"));
    let old_toolchain_tree = tree_sha256(&input.join("toolchain"));
    let unlaunchable_trunk = unlaunchable_input.join("toolchain/bin/trunk");
    write(
        &unlaunchable_trunk,
        "#!/bin/sh\nprintf 'staged trunk loader rejected\\n' >&2\nexit 127\n",
    );
    fs::set_permissions(&unlaunchable_trunk, fs::Permissions::from_mode(0o755))
        .expect("make unlaunchable staged trunk executable");
    let new_trunk_sha = sha256(&unlaunchable_input.join("toolchain/bin/trunk"));
    let new_toolchain_tree = tree_sha256(&unlaunchable_input.join("toolchain"));
    let inventory = fs::read_to_string(unlaunchable_input.join("tool-input-inventory.txt"))
        .expect("read unlaunchable tool inventory");
    write(
        unlaunchable_input.join("tool-input-inventory.txt"),
        &inventory
            .replace(
                &format!("sha256={old_trunk_sha}"),
                &format!("sha256={new_trunk_sha}"),
            )
            .replace(
                &format!("tree_sha256={old_toolchain_tree}"),
                &format!("tree_sha256={new_toolchain_tree}"),
            ),
    );
    let original_inventory_sha = sha256(&input.join("tool-input-inventory.txt"));
    let unlaunchable_inventory_sha = sha256(&unlaunchable_input.join("tool-input-inventory.txt"));
    let input_receipt = fs::read_to_string(unlaunchable_input.join("tool-inputs.toml"))
        .expect("read unlaunchable input receipt");
    write(
        unlaunchable_input.join("tool-inputs.toml"),
        &input_receipt.replace(&original_inventory_sha, &unlaunchable_inventory_sha),
    );
    write_tool_input_manifest(&unlaunchable_input);
    assert!(
        Command::new("chmod")
            .args(["-R", "a-w"])
            .arg(&unlaunchable_input)
            .status()
            .expect("reseal unlaunchable input")
            .success(),
        "unlaunchable input must be immutable at stage entry"
    );
    let unlaunchable_stage = temp.join("stage-unlaunchable-tool");
    copy_tree(&sealed, &unlaunchable_stage);
    assert!(
        Command::new("chmod")
            .args(["-R", "u+w"])
            .arg(&unlaunchable_stage)
            .status()
            .expect("make unlaunchable stage writable")
            .success(),
        "unlaunchable stage is writable only in its disposable fixture"
    );
    for path in [
        "toolchain",
        "network-denied-bin",
        "staging-home",
        "inputs/toolchain-input-receipt.toml",
        "f05-inputs.sha256",
    ] {
        let path = unlaunchable_stage.join(path);
        if path.is_dir() {
            fs::remove_dir_all(path).expect("remove disposable staged directory");
        } else {
            fs::remove_file(path).expect("remove disposable staged file");
        }
    }
    write(
        unlaunchable_stage.join("inputs/toolchain-input-admission-receipt.toml"),
        &copied_tool_receipt.replace(&original_inventory_sha, &unlaunchable_inventory_sha),
    );
    seal_tool_stage_identity_inputs(&unlaunchable_stage);
    let unlaunchable = Command::new("bash")
        .arg(&script)
        .args(["--sealed-root"])
        .arg(&unlaunchable_stage)
        .args(["--stage-closure-toolchain-inputs"])
        .arg(&unlaunchable_input)
        .output()
        .expect("run unlaunchable staged-tool denial");
    assert!(
        !unlaunchable.status.success()
            && String::from_utf8_lossy(&unlaunchable.stderr)
                .contains("rejects an unlaunchable or version-mismatched trunk input"),
        "an ABI or loader-unlaunchable tool must fail closed before closure copy; stdout={} stderr={}",
        String::from_utf8_lossy(&unlaunchable.stdout),
        String::from_utf8_lossy(&unlaunchable.stderr)
    );
    let unlaunchable_stdout = String::from_utf8_lossy(&unlaunchable.stdout);
    assert!(
        unlaunchable_stdout.contains("toolchain_probe tool=trunk")
            && unlaunchable_stdout.contains("actual_exit=127")
            && unlaunchable_stdout.contains("staged\\ trunk\\ loader\\ rejected"),
        "the rejected tool probe must retain the staged path, exit, and loader diagnostic"
    );
    assert!(
        !unlaunchable_stage.join("toolchain").exists(),
        "an unlaunchable tool must deny before any toolchain is copied into a closure"
    );

    assert!(
        Command::new("chmod")
            .args(["-R", "u+w"])
            .arg(&temp)
            .status()
            .expect("restore disposable tool-stage permissions")
            .success(),
        "test cleanup may only restore permissions on its disposable fixtures"
    );
    fs::remove_dir_all(temp).expect("remove disposable tool-input fixture");
}

fn staged_provenance(stage: &Path) -> std::process::ExitStatus {
    let source_script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../packaging/plugin-process-package-provenance.sh");
    Command::new("bash")
        .args(["-c", "source \"$1\"; das_plugin_process_write_provenance \"$2/artifact.deb\" \"$2/source\" 0.186.6 amd64 \"$2/web\" \"$2/server\"", "fixture"])
        .arg(source_script)
        .arg(stage)
        .env("DASOBJECTSTORE_F05_STAGED_CLOSURE_ROOT", stage)
        .env(
            "DASOBJECTSTORE_SOURCE_REVISION",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        )
        .env("SOURCE_DATE_EPOCH", "1789751689")
        .status()
        .expect("run staged provenance fixture")
}

#[test]
fn staged_provenance_is_json_and_denies_tampered_closure_inputs() {
    let temp = std::env::temp_dir().join(format!(
        "dasobjectstore-plugin-process-contract-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time")
            .as_nanos()
    ));
    fs::create_dir(&temp).expect("temporary staged closure root");
    let stage = staged_fixture(&temp);
    assert!(
        staged_provenance(&stage).success(),
        "accept exact staged closure"
    );
    let provenance = stage.join("artifact.deb.provenance.json");
    assert!(
        Command::new("jq")
            .arg("empty")
            .arg(&provenance)
            .status()
            .expect("parse staged provenance JSON")
            .success(),
        "staged provenance sidecar must be JSON"
    );

    for (name, path, replacement) in [
        ("missing-cargo", "toolchain/bin/cargo", None),
        (
            "altered-rustc",
            "toolchain/bin/rustc",
            Some("altered rustc\n"),
        ),
        (
            "decoy-trunk",
            "toolchain/bin/trunk",
            Some("#!/bin/sh\necho decoy\n"),
        ),
        (
            "missing-wasm",
            "toolchain/lib/rustlib/wasm32-unknown-unknown/libfixture.rlib",
            None,
        ),
        (
            "altered-vendor",
            "source/vendor/fixture.crate",
            Some("altered vendor\n"),
        ),
        (
            "missing-config",
            "source/.cargo/f05-vendor-config.toml",
            None,
        ),
        (
            "altered-candidate",
            "inputs/component-candidate-input.toml",
            Some(
                "toolchain_image = \"docker.io/library/rust@sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\"\ntoolchain_image_sha256 = \"sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\"\n",
            ),
        ),
        (
            "altered-image",
            "inputs/component-candidate-input.toml",
            Some(
                "toolchain_image = \"docker.io/library/rust@sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc\"\ntoolchain_image_sha256 = \"sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc\"\n",
            ),
        ),
    ] {
        let variant_root = temp.join(name);
        fs::create_dir(&variant_root).expect("create staged closure variant root");
        copy_tree(&stage, &variant_root);
        let variant = variant_root.join("closure");
        let target = variant.join(path);
        if let Some(replacement) = replacement {
            write(&target, replacement);
            if path.starts_with("toolchain/bin/") {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    fs::set_permissions(&target, fs::Permissions::from_mode(0o755))
                        .expect("make decoy staged tool executable");
                }
            }
        } else {
            fs::remove_file(&target).expect("remove staged input");
        }
        assert!(
            !staged_provenance(&variant).success(),
            "staged provenance accepted {name} closure input"
        );
    }
    fs::remove_dir_all(temp).expect("remove temporary staged closure root");
}
