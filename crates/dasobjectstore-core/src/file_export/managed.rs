use super::FileExportRecipeError;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ManagedExportHost {
    Linux,
    Macos,
    Other,
}

impl ManagedExportHost {
    fn as_str(self) -> &'static str {
        match self {
            Self::Linux => "linux",
            Self::Macos => "macos",
            Self::Other => "other",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ManagedExportProtocol {
    Smb,
    Nfs,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ManagedExportExecutionMode {
    ManualReviewRequired,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ManagedReadOnlyExportTaskRequest {
    pub host: ManagedExportHost,
    pub protocol: ManagedExportProtocol,
    pub recipe_path: PathBuf,
    pub service_name: String,
}

impl ManagedReadOnlyExportTaskRequest {
    pub fn new(
        host: ManagedExportHost,
        protocol: ManagedExportProtocol,
        recipe_path: impl Into<PathBuf>,
        service_name: impl Into<String>,
    ) -> Self {
        Self {
            host,
            protocol,
            recipe_path: recipe_path.into(),
            service_name: service_name.into(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ManagedReadOnlyExportTaskPlan {
    pub host: ManagedExportHost,
    pub protocol: ManagedExportProtocol,
    pub execution_mode: ManagedExportExecutionMode,
    pub requires_root: bool,
    pub mutates_host: bool,
    pub commands: Vec<ManagedExportCommand>,
    pub safety_notes: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ManagedExportCommand {
    pub program: String,
    pub args: Vec<String>,
    pub description: String,
}

pub fn plan_managed_read_only_export_task(
    request: &ManagedReadOnlyExportTaskRequest,
) -> Result<ManagedReadOnlyExportTaskPlan, FileExportRecipeError> {
    validate_linux_host(request.host)?;
    validate_recipe_path(&request.recipe_path)?;
    validate_service_name(&request.service_name)?;

    Ok(ManagedReadOnlyExportTaskPlan {
        host: request.host,
        protocol: request.protocol,
        execution_mode: ManagedExportExecutionMode::ManualReviewRequired,
        requires_root: true,
        mutates_host: true,
        commands: commands_for(request),
        safety_notes: vec![
            "DASObjectStore only plans these commands; it does not execute them.".to_string(),
            "Review the generated recipe before installing it into system export configuration."
                .to_string(),
            "Run commands only on Linux and only after confirming the export is read-only."
                .to_string(),
        ],
    })
}

fn commands_for(request: &ManagedReadOnlyExportTaskRequest) -> Vec<ManagedExportCommand> {
    let destination = destination_path(request.protocol);
    let mut commands = vec![
        ManagedExportCommand {
            program: "install".to_string(),
            args: vec![
                "-m".to_string(),
                "0444".to_string(),
                request.recipe_path.display().to_string(),
                destination.to_string(),
            ],
            description: "Install the reviewed read-only export recipe.".to_string(),
        },
        validation_command(request.protocol),
    ];

    commands.push(ManagedExportCommand {
        program: "systemctl".to_string(),
        args: vec!["reload".to_string(), request.service_name.clone()],
        description: "Reload the export service after configuration validation.".to_string(),
    });

    commands
}

fn destination_path(protocol: ManagedExportProtocol) -> &'static str {
    match protocol {
        ManagedExportProtocol::Smb => "/etc/samba/conf.d/dasobjectstore-readonly.conf",
        ManagedExportProtocol::Nfs => "/etc/exports.d/dasobjectstore-readonly.exports",
    }
}

fn validation_command(protocol: ManagedExportProtocol) -> ManagedExportCommand {
    match protocol {
        ManagedExportProtocol::Smb => ManagedExportCommand {
            program: "testparm".to_string(),
            args: vec!["-s".to_string()],
            description: "Validate Samba configuration before service reload.".to_string(),
        },
        ManagedExportProtocol::Nfs => ManagedExportCommand {
            program: "exportfs".to_string(),
            args: vec!["-ra".to_string()],
            description: "Validate and reload kernel NFS export table.".to_string(),
        },
    }
}

fn validate_linux_host(host: ManagedExportHost) -> Result<(), FileExportRecipeError> {
    if host == ManagedExportHost::Linux {
        Ok(())
    } else {
        Err(FileExportRecipeError::UnsupportedManagedExportHost {
            host: host.as_str().to_string(),
        })
    }
}

fn validate_recipe_path(path: &Path) -> Result<(), FileExportRecipeError> {
    if path.is_absolute() {
        Ok(())
    } else {
        Err(FileExportRecipeError::RelativeRecipePath {
            path: path.to_path_buf(),
        })
    }
}

fn validate_service_name(value: &str) -> Result<(), FileExportRecipeError> {
    if value.trim().is_empty() {
        Err(FileExportRecipeError::BlankServiceName)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        plan_managed_read_only_export_task, ManagedExportExecutionMode, ManagedExportHost,
        ManagedExportProtocol, ManagedReadOnlyExportTaskRequest,
    };
    use crate::file_export::FileExportRecipeError;
    use std::path::PathBuf;

    #[test]
    fn plans_linux_smb_export_task_without_executing_it() {
        let request = ManagedReadOnlyExportTaskRequest::new(
            ManagedExportHost::Linux,
            ManagedExportProtocol::Smb,
            "/tmp/dasobjectstore/generated.conf",
            "smb",
        );

        let plan = plan_managed_read_only_export_task(&request).expect("task plan");

        assert_eq!(
            plan.execution_mode,
            ManagedExportExecutionMode::ManualReviewRequired
        );
        assert!(plan.requires_root);
        assert!(plan.mutates_host);
        assert_eq!(plan.commands.len(), 3);
        assert_eq!(plan.commands[0].program, "install");
        assert!(plan.commands[0]
            .args
            .contains(&"/etc/samba/conf.d/dasobjectstore-readonly.conf".to_string()));
        assert_eq!(plan.commands[1].program, "testparm");
        assert_eq!(
            plan.commands[2].args,
            vec!["reload".to_string(), "smb".to_string()]
        );
        assert!(plan
            .safety_notes
            .iter()
            .any(|note| note.contains("does not execute")));
    }

    #[test]
    fn plans_linux_nfs_export_task_without_executing_it() {
        let request = ManagedReadOnlyExportTaskRequest::new(
            ManagedExportHost::Linux,
            ManagedExportProtocol::Nfs,
            "/tmp/dasobjectstore/generated.exports",
            "nfs-server",
        );

        let plan = plan_managed_read_only_export_task(&request).expect("task plan");

        assert_eq!(plan.commands[1].program, "exportfs");
        assert_eq!(
            plan.commands[0].args,
            vec![
                "-m".to_string(),
                "0444".to_string(),
                "/tmp/dasobjectstore/generated.exports".to_string(),
                "/etc/exports.d/dasobjectstore-readonly.exports".to_string(),
            ]
        );
    }

    #[test]
    fn rejects_macos_managed_export_tasks() {
        let request = ManagedReadOnlyExportTaskRequest::new(
            ManagedExportHost::Macos,
            ManagedExportProtocol::Smb,
            "/tmp/dasobjectstore/generated.conf",
            "smb",
        );

        let err = plan_managed_read_only_export_task(&request).expect_err("macos rejected");

        assert_eq!(
            err,
            FileExportRecipeError::UnsupportedManagedExportHost {
                host: "macos".to_string()
            }
        );
    }

    #[test]
    fn rejects_relative_recipe_paths() {
        let request = ManagedReadOnlyExportTaskRequest::new(
            ManagedExportHost::Linux,
            ManagedExportProtocol::Smb,
            "generated.conf",
            "smb",
        );

        let err = plan_managed_read_only_export_task(&request).expect_err("path rejected");

        assert_eq!(
            err,
            FileExportRecipeError::RelativeRecipePath {
                path: PathBuf::from("generated.conf")
            }
        );
    }

    #[test]
    fn rejects_blank_service_names() {
        let request = ManagedReadOnlyExportTaskRequest::new(
            ManagedExportHost::Linux,
            ManagedExportProtocol::Smb,
            "/tmp/dasobjectstore/generated.conf",
            " ",
        );

        let err = plan_managed_read_only_export_task(&request).expect_err("service rejected");

        assert_eq!(err, FileExportRecipeError::BlankServiceName);
    }
}
