//! Source-only, read-only custody input consistency checker. No runtime authority.
mod custody_plan_input;

use clap::{Parser, Subcommand};
use dasobjectstore_object_service::bootstrap_plan::plan_bootstrap;
use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Parser)]
#[command(
    version,
    about = "Check untrusted custody inputs; never authorizes execution"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    /// Check supplied regular files and emit only a redacted consistency result.
    Plan {
        #[arg(long)]
        manifest: PathBuf,
        #[arg(long)]
        observation: PathBuf,
    },
}

fn main() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error) => {
            if matches!(
                error.kind(),
                clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion
            ) {
                let _ = error.print();
                return ExitCode::SUCCESS;
            }
            // Clap's normal error can echo hostile argument/credential values.
            eprintln!("bootstrap_plan_arguments_denied");
            return ExitCode::FAILURE;
        }
    };
    match run(cli) {
        Ok(output) => {
            if std::io::stdout().write_all(&output).is_ok() {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Err(code) => {
            eprintln!("{code}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<Vec<u8>, String> {
    let Command::Plan {
        manifest,
        observation,
    } = cli.command;
    let manifest = custody_plan_input::read(&manifest).map_err(str::to_owned)?;
    let observation = custody_plan_input::read(&observation).map_err(str::to_owned)?;
    let plan = plan_bootstrap(&manifest, &observation).map_err(|e| e.to_string())?;
    let mut output =
        serde_json::to_vec(&plan).map_err(|_| "bootstrap_plan_output_denied".to_owned())?;
    output.push(b'\n');
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use dasobjectstore_core::ids::StoreId;
    use dasobjectstore_object_service::bootstrap_plan::{MANIFEST_SCHEMA, OBSERVATION_SCHEMA};
    use dasobjectstore_object_service::{
        CustodyAssuranceClass, CustodyRetentionPolicyV1, CustodyStoreDefinitionV1,
        CustodyStoreProfileV1, CUSTODY_OVERLAY_SCHEMA_V1, CUSTODY_PROFILE_V1,
    };
    use serde_json::{json, Value};
    use sha2::{Digest, Sha256};
    fn digest(value: &[u8]) -> String {
        format!("sha256:{:x}", Sha256::digest(value))
    }
    mod fixture {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../dasobjectstore-object-service/src/bootstrap_plan/fixture.rs"
        ));
    }

    #[test]
    fn real_file_adapter_roundtrip_preserves_inputs_and_emits_no_authority() {
        let root = std::env::temp_dir()
            .canonicalize()
            .unwrap()
            .join(format!("custody-plan-cli-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let (m, o) = fixture::fixture();
        let (m, o) = fixture::bytes(&m, &o);
        let manifest = root.join("manifest");
        let observation = root.join("observation");
        std::fs::write(&manifest, &m).unwrap();
        std::fs::write(&observation, &o).unwrap();
        let output = run(Cli {
            command: Command::Plan {
                manifest: manifest.clone(),
                observation: observation.clone(),
            },
        })
        .unwrap();
        let output: Value = serde_json::from_slice(&output).unwrap();
        assert_eq!(output["execution_authorized"], false);
        assert_eq!(output["live_target_verified"], false);
        assert_eq!(output["manifest_sha256"], digest(&m));
        assert_eq!(std::fs::read(&manifest).unwrap(), m);
        assert_eq!(std::fs::read(&observation).unwrap(), o);
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 2);
        std::fs::write(&manifest, b"{\"secret\":\"never-echo\"}").unwrap();
        let denial = run(Cli {
            command: Command::Plan {
                manifest: manifest.clone(),
                observation: observation.clone(),
            },
        })
        .unwrap_err();
        assert_eq!(denial, "bootstrap_plan_encoding_denied");
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 2);
        std::fs::remove_file(manifest).unwrap();
        std::fs::remove_file(observation).unwrap();
        std::fs::remove_dir(root).unwrap();
    }
    #[test]
    fn only_plan_and_explicit_inputs_parse() {
        assert!(
            Cli::try_parse_from(["tool", "plan", "--manifest", "/a", "--observation", "/b"])
                .is_ok()
        );
        for extra in [
            "--apply",
            "--output",
            "--credential",
            "--target",
            "--config",
        ] {
            assert!(Cli::try_parse_from([
                "tool",
                "plan",
                "--manifest",
                "/a",
                "--observation",
                "/b",
                extra
            ])
            .is_err());
        }
        assert!(Cli::try_parse_from(["tool", "apply"]).is_err());
    }
}
