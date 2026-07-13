use clap::{Args, Subcommand};
use dasobjectstore_object_service::ObjectServiceProviderId;
use std::path::{Path, PathBuf};

#[derive(Debug, Eq, PartialEq, Args)]
pub(crate) struct ServiceArgs {
    #[command(subcommand)]
    command: ServiceCommand,
}
impl ServiceArgs {
    pub(crate) fn command(&self) -> &ServiceCommand {
        &self.command
    }
}

#[derive(Debug, Eq, PartialEq, Subcommand)]
pub(crate) enum ServiceCommand {
    /// Render Docker Compose YAML for store-aware object service access.
    RenderCompose(ServiceRenderComposeArgs),
    /// Provision S3 buckets and credentials from the live ObjectStore registry without restarting the service.
    Provision(ServiceProvisionArgs),
    /// Start the rendered object service with Docker Compose.
    Up(ServiceComposeArgs),
    /// Stop the rendered object service with Docker Compose.
    Down(ServiceComposeArgs),
    /// Inspect the rendered object service with Docker Compose.
    Status(ServiceStatusArgs),
}

#[derive(Debug, Eq, PartialEq, Args)]
pub(crate) struct ServiceProvisionArgs {
    /// Object service provider to provision.
    #[arg(long, default_value = "garage")]
    provider: ObjectServiceProviderId,
    /// Show the provisioning plan counts without applying Garage bucket/key changes.
    #[arg(long)]
    dry_run: bool,
    /// Rotate persisted Garage credentials before applying bucket/key grants.
    #[arg(long)]
    rotate_credentials: bool,
}
impl ServiceProvisionArgs {
    pub(crate) fn provider(&self) -> ObjectServiceProviderId {
        self.provider
    }
    pub(crate) fn dry_run(&self) -> bool {
        self.dry_run
    }
    pub(crate) fn rotate_credentials(&self) -> bool {
        self.rotate_credentials
    }
}

#[derive(Debug, Eq, PartialEq, Args)]
pub(crate) struct ServiceComposeArgs {
    /// Path to the rendered Docker Compose YAML file.
    #[arg(long)]
    compose_file: PathBuf,
    /// Optional Docker Compose project directory.
    #[arg(long)]
    project_directory: Option<PathBuf>,
    /// Print the Docker Compose command without executing it.
    #[arg(long)]
    dry_run: bool,
}
impl ServiceComposeArgs {
    pub(crate) fn compose_file(&self) -> &Path {
        &self.compose_file
    }
    pub(crate) fn project_directory(&self) -> Option<&Path> {
        self.project_directory.as_deref()
    }
    pub(crate) fn dry_run(&self) -> bool {
        self.dry_run
    }
}

#[derive(Debug, Eq, PartialEq, Args)]
pub(crate) struct ServiceStatusArgs {
    /// Path to the rendered Docker Compose YAML file.
    #[arg(long)]
    compose_file: PathBuf,
    /// Optional Docker Compose project directory.
    #[arg(long)]
    project_directory: Option<PathBuf>,
    /// Emit Docker Compose service status as JSON.
    #[arg(long)]
    json: bool,
    /// Print the Docker Compose status command as JSON without executing it.
    #[arg(long)]
    dry_run: bool,
}
impl ServiceStatusArgs {
    pub(crate) fn compose_file(&self) -> &Path {
        &self.compose_file
    }
    pub(crate) fn project_directory(&self) -> Option<&Path> {
        self.project_directory.as_deref()
    }
    pub(crate) fn json(&self) -> bool {
        self.json
    }
    pub(crate) fn dry_run(&self) -> bool {
        self.dry_run
    }
}

#[derive(Debug, Eq, PartialEq, Args)]
pub(crate) struct ServiceRenderComposeArgs {
    /// Advanced test override for the system-managed store registry path.
    #[arg(long = "stores-file", hide = true)]
    stores_file: Option<PathBuf>,
    /// Docker Compose project name.
    #[arg(long)]
    project_name: String,
    /// SSD metadata path to mount into the service container.
    #[arg(long)]
    ssd_metadata_path: PathBuf,
    /// HDD data path to mount into the service container.
    #[arg(long)]
    hdd_data_path: PathBuf,
    /// Object service provider to render for.
    #[arg(long)]
    provider: ObjectServiceProviderId,
    /// Compose service name.
    #[arg(long, default_value = "object-service")]
    service_name: String,
    /// Container image for the selected object service.
    #[arg(long)]
    image: String,
    /// Host address to bind the object-service port on.
    #[arg(long, default_value = "0.0.0.0")]
    bind_address: String,
    /// API port to expose.
    #[arg(long)]
    api_port: u16,
    /// Host path for the generated Garage configuration file.
    #[arg(long, default_value = "/etc/dasobjectstore/garage.toml")]
    config_path: PathBuf,
}
impl ServiceRenderComposeArgs {
    pub(crate) fn stores_file(&self) -> Option<&Path> {
        self.stores_file.as_deref()
    }
    pub(crate) fn project_name(&self) -> &str {
        &self.project_name
    }
    pub(crate) fn ssd_metadata_path(&self) -> &Path {
        &self.ssd_metadata_path
    }
    pub(crate) fn hdd_data_path(&self) -> &Path {
        &self.hdd_data_path
    }
    pub(crate) fn provider(&self) -> ObjectServiceProviderId {
        self.provider
    }
    pub(crate) fn service_name(&self) -> &str {
        &self.service_name
    }
    pub(crate) fn image(&self) -> &str {
        &self.image
    }
    pub(crate) fn bind_address(&self) -> &str {
        &self.bind_address
    }
    pub(crate) fn api_port(&self) -> u16 {
        self.api_port
    }
    pub(crate) fn config_path(&self) -> &Path {
        &self.config_path
    }
}

#[cfg(test)]
mod tests {
    use super::ServiceCommand;
    use crate::cli::{Cli, Command};
    use clap::Parser;
    use std::path::Path;

    #[test]
    fn parses_service_command_families() {
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "service",
            "render-compose",
            "--project-name",
            "dasobjectstore-dev",
            "--ssd-metadata-path",
            "/ssd/meta",
            "--hdd-data-path",
            "/hdd/data",
            "--provider",
            "garage",
            "--service-name",
            "garage",
            "--image",
            "garage:latest",
            "--api-port",
            "3900",
        ])
        .expect("service render-compose parses");
        let Some(Command::Service(args)) = cli.command() else {
            panic!("expected service command")
        };
        let ServiceCommand::RenderCompose(render) = args.command() else {
            panic!("expected render")
        };
        assert_eq!(render.stores_file(), None);
        assert_eq!(render.project_name(), "dasobjectstore-dev");
        assert_eq!(render.ssd_metadata_path(), Path::new("/ssd/meta"));
        assert_eq!(render.hdd_data_path(), Path::new("/hdd/data"));
        assert_eq!(render.provider().name(), "garage");
        assert_eq!(render.service_name(), "garage");
        assert_eq!(render.image(), "garage:latest");
        assert_eq!(render.bind_address(), "0.0.0.0");
        assert_eq!(render.api_port(), 3900);
        assert_eq!(
            render.config_path(),
            Path::new("/etc/dasobjectstore/garage.toml")
        );

        for (args, expected) in [
            (
                vec!["provision", "--provider", "garage", "--dry-run"],
                "provision",
            ),
            (vec!["provision", "--rotate-credentials"], "rotate"),
            (
                vec![
                    "up",
                    "--compose-file",
                    "/tmp/compose.yaml",
                    "--project-directory",
                    "/tmp/project",
                    "--dry-run",
                ],
                "up",
            ),
            (
                vec!["down", "--compose-file", "/tmp/compose.yaml", "--dry-run"],
                "down",
            ),
            (
                vec![
                    "status",
                    "--compose-file",
                    "/tmp/compose.yaml",
                    "--project-directory",
                    "/tmp/project",
                    "--json",
                    "--dry-run",
                ],
                "status",
            ),
        ] {
            let mut argv = vec!["dasobjectstore", "service"];
            argv.extend(args);
            let cli = Cli::try_parse_from(argv).expect("service command parses");
            let Some(Command::Service(service)) = cli.command() else {
                panic!("expected service command")
            };
            match (service.command(), expected) {
                (ServiceCommand::Provision(provision), "provision") => {
                    assert_eq!(provision.provider().name(), "garage");
                    assert!(provision.dry_run());
                    assert!(!provision.rotate_credentials());
                }
                (ServiceCommand::Provision(provision), "rotate") => {
                    assert!(provision.rotate_credentials());
                    assert!(!provision.dry_run());
                }
                (ServiceCommand::Up(up), "up") => {
                    assert_eq!(up.compose_file(), Path::new("/tmp/compose.yaml"));
                    assert_eq!(up.project_directory(), Some(Path::new("/tmp/project")));
                    assert!(up.dry_run());
                }
                (ServiceCommand::Down(down), "down") => {
                    assert_eq!(down.compose_file(), Path::new("/tmp/compose.yaml"));
                    assert_eq!(down.project_directory(), None);
                    assert!(down.dry_run());
                }
                (ServiceCommand::Status(status), "status") => {
                    assert_eq!(status.compose_file(), Path::new("/tmp/compose.yaml"));
                    assert_eq!(status.project_directory(), Some(Path::new("/tmp/project")));
                    assert!(status.json());
                    assert!(status.dry_run());
                }
                _ => panic!("unexpected service command"),
            }
        }
    }
}
