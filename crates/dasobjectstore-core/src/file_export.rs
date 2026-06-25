//! Read-only file export recipe generation.

mod managed;

use crate::ids::StoreId;
pub use managed::{
    plan_managed_read_only_export_task, ManagedExportCommand, ManagedExportExecutionMode,
    ManagedExportHost, ManagedExportProtocol, ManagedReadOnlyExportTaskPlan,
    ManagedReadOnlyExportTaskRequest,
};
use serde::{Deserialize, Serialize};
use std::fmt::{self, Display};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SmbExportRecipeRequest {
    pub share_name: String,
    pub store_id: StoreId,
    pub settled_path: PathBuf,
    pub comment: Option<String>,
}

impl SmbExportRecipeRequest {
    pub fn new(
        share_name: impl Into<String>,
        store_id: StoreId,
        settled_path: impl Into<PathBuf>,
    ) -> Self {
        Self {
            share_name: share_name.into(),
            store_id,
            settled_path: settled_path.into(),
            comment: None,
        }
    }

    pub fn with_comment(mut self, comment: impl Into<String>) -> Self {
        self.comment = Some(comment.into());
        self
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SmbExportRecipe {
    pub share_name: String,
    pub store_id: StoreId,
    pub settled_path: PathBuf,
    pub smb_conf_snippet: String,
    pub validation_notes: Vec<String>,
}

pub fn render_smb_export_recipe(
    request: &SmbExportRecipeRequest,
) -> Result<SmbExportRecipe, FileExportRecipeError> {
    validate_share_name(&request.share_name)?;
    validate_settled_path(&request.settled_path)?;

    let comment = request
        .comment
        .as_deref()
        .unwrap_or("DASObjectStore read-only settled export");
    let snippet = format!(
        "[{share_name}]\n  path = {path}\n  comment = {comment}\n  read only = yes\n  browseable = yes\n  guest ok = no\n  writable = no\n  create mask = 0444\n  directory mask = 0555\n  veto files = /.dasobjectstore/.DS_Store/\n",
        share_name = request.share_name,
        path = request.settled_path.display(),
        comment = escape_smb_value(comment),
    );

    Ok(SmbExportRecipe {
        share_name: request.share_name.clone(),
        store_id: request.store_id.clone(),
        settled_path: request.settled_path.clone(),
        smb_conf_snippet: snippet,
        validation_notes: vec![
            "Export only settled/protected object data, never SSD ingest staging.".to_string(),
            "Review the generated snippet before including it in smb.conf.".to_string(),
            "Restart or reload Samba outside DASObjectStore after applying the snippet."
                .to_string(),
        ],
    })
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct NfsExportRecipeRequest {
    pub export_name: String,
    pub store_id: StoreId,
    pub settled_path: PathBuf,
    pub client_spec: String,
}

impl NfsExportRecipeRequest {
    pub fn new(
        export_name: impl Into<String>,
        store_id: StoreId,
        settled_path: impl Into<PathBuf>,
        client_spec: impl Into<String>,
    ) -> Self {
        Self {
            export_name: export_name.into(),
            store_id,
            settled_path: settled_path.into(),
            client_spec: client_spec.into(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct NfsExportRecipe {
    pub export_name: String,
    pub store_id: StoreId,
    pub settled_path: PathBuf,
    pub exports_line: String,
    pub validation_notes: Vec<String>,
}

pub fn render_nfs_export_recipe(
    request: &NfsExportRecipeRequest,
) -> Result<NfsExportRecipe, FileExportRecipeError> {
    validate_export_name(&request.export_name)?;
    validate_settled_path(&request.settled_path)?;
    validate_nfs_client_spec(&request.client_spec)?;

    let exports_line = format!(
        "{} {}(ro,sync,no_subtree_check,root_squash)\n",
        request.settled_path.display(),
        request.client_spec
    );

    Ok(NfsExportRecipe {
        export_name: request.export_name.clone(),
        store_id: request.store_id.clone(),
        settled_path: request.settled_path.clone(),
        exports_line,
        validation_notes: vec![
            "Export only settled/protected object data, never SSD ingest staging.".to_string(),
            "Review the generated line before adding it to /etc/exports.".to_string(),
            "Reload NFS exports outside DASObjectStore after applying the recipe.".to_string(),
        ],
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FileExportRecipeError {
    BlankShareName,
    BlankExportName,
    BlankClientSpec,
    BlankServiceName,
    InvalidShareName { value: String },
    InvalidExportName { value: String },
    RelativeSettledPath { path: PathBuf },
    RelativeRecipePath { path: PathBuf },
    UnsupportedManagedExportHost { host: String },
}

impl Display for FileExportRecipeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BlankShareName => formatter.write_str("SMB share name must not be blank"),
            Self::BlankExportName => formatter.write_str("NFS export name must not be blank"),
            Self::BlankClientSpec => formatter.write_str("NFS client spec must not be blank"),
            Self::BlankServiceName => {
                formatter.write_str("managed export service name must not be blank")
            }
            Self::InvalidShareName { value } => write!(
                formatter,
                "invalid SMB share name `{value}`; use letters, numbers, dots, dashes, and underscores"
            ),
            Self::InvalidExportName { value } => write!(
                formatter,
                "invalid NFS export name `{value}`; use letters, numbers, dots, dashes, and underscores"
            ),
            Self::RelativeSettledPath { path } => write!(
                formatter, "settled export path must be absolute: {}", path.display()
            ),
            Self::RelativeRecipePath { path } => write!(
                formatter,
                "managed export recipe path must be absolute: {}",
                path.display()
            ),
            Self::UnsupportedManagedExportHost { host } => write!(
                formatter,
                "managed read-only exports are only planned for Linux hosts, not {host}"
            ),
        }
    }
}

impl std::error::Error for FileExportRecipeError {}

fn validate_share_name(value: &str) -> Result<(), FileExportRecipeError> {
    if value.trim().is_empty() {
        return Err(FileExportRecipeError::BlankShareName);
    }

    if is_export_name(value) {
        Ok(())
    } else {
        Err(FileExportRecipeError::InvalidShareName {
            value: value.to_string(),
        })
    }
}

fn validate_export_name(value: &str) -> Result<(), FileExportRecipeError> {
    if value.trim().is_empty() {
        return Err(FileExportRecipeError::BlankExportName);
    }

    if is_export_name(value) {
        Ok(())
    } else {
        Err(FileExportRecipeError::InvalidExportName {
            value: value.to_string(),
        })
    }
}

fn validate_settled_path(path: &Path) -> Result<(), FileExportRecipeError> {
    if path.is_absolute() {
        Ok(())
    } else {
        Err(FileExportRecipeError::RelativeSettledPath {
            path: path.to_path_buf(),
        })
    }
}

fn validate_nfs_client_spec(value: &str) -> Result<(), FileExportRecipeError> {
    if value.trim().is_empty() {
        return Err(FileExportRecipeError::BlankClientSpec);
    }

    Ok(())
}

fn is_export_name(value: &str) -> bool {
    value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_'))
}

fn escape_smb_value(value: &str) -> String {
    value.replace(['\n', '\r'], " ")
}

#[cfg(test)]
mod tests {
    use super::{
        render_nfs_export_recipe, render_smb_export_recipe, FileExportRecipeError,
        NfsExportRecipeRequest, SmbExportRecipeRequest,
    };
    use crate::ids::StoreId;
    use std::path::PathBuf;

    #[test]
    fn renders_read_only_smb_recipe() {
        let request = SmbExportRecipeRequest::new(
            "generated_data",
            StoreId::new("generated").expect("store id"),
            "/srv/dasobjectstore/generated/settled",
        )
        .with_comment("Generated data");

        let recipe = render_smb_export_recipe(&request).expect("recipe renders");

        assert_eq!(recipe.share_name, "generated_data");
        assert_eq!(recipe.store_id.as_str(), "generated");
        assert!(recipe.smb_conf_snippet.contains("[generated_data]\n"));
        assert!(recipe
            .smb_conf_snippet
            .contains("path = /srv/dasobjectstore/generated/settled\n"));
        assert!(recipe.smb_conf_snippet.contains("read only = yes\n"));
        assert!(recipe.smb_conf_snippet.contains("writable = no\n"));
        assert!(recipe
            .validation_notes
            .iter()
            .any(|note| note.contains("settled/protected")));
    }

    #[test]
    fn rejects_unsafe_share_names() {
        let request = SmbExportRecipeRequest::new(
            "bad share",
            StoreId::new("generated").expect("store id"),
            "/srv/dasobjectstore/generated/settled",
        );

        let err = render_smb_export_recipe(&request).expect_err("share name rejected");

        assert_eq!(
            err,
            FileExportRecipeError::InvalidShareName {
                value: "bad share".to_string()
            }
        );
    }

    #[test]
    fn rejects_relative_settled_paths() {
        let request = SmbExportRecipeRequest::new(
            "generated_data",
            StoreId::new("generated").expect("store id"),
            "generated/settled",
        );

        let err = render_smb_export_recipe(&request).expect_err("relative path rejected");

        assert_eq!(
            err,
            FileExportRecipeError::RelativeSettledPath {
                path: PathBuf::from("generated/settled")
            }
        );
    }

    #[test]
    fn strips_newlines_from_comments() {
        let request = SmbExportRecipeRequest::new(
            "generated_data",
            StoreId::new("generated").expect("store id"),
            "/srv/dasobjectstore/generated/settled",
        )
        .with_comment("Generated\ndata");

        let recipe = render_smb_export_recipe(&request).expect("recipe renders");

        assert!(recipe
            .smb_conf_snippet
            .contains("comment = Generated data\n"));
    }

    #[test]
    fn renders_read_only_nfs_recipe() {
        let request = NfsExportRecipeRequest::new(
            "generated_data",
            StoreId::new("generated").expect("store id"),
            "/srv/dasobjectstore/generated/settled",
            "192.168.10.0/24",
        );

        let recipe = render_nfs_export_recipe(&request).expect("recipe renders");

        assert_eq!(recipe.export_name, "generated_data");
        assert_eq!(recipe.store_id.as_str(), "generated");
        assert_eq!(
            recipe.exports_line,
            "/srv/dasobjectstore/generated/settled 192.168.10.0/24(ro,sync,no_subtree_check,root_squash)\n"
        );
        assert!(recipe
            .validation_notes
            .iter()
            .any(|note| note.contains("settled/protected")));
    }

    #[test]
    fn rejects_blank_nfs_client_spec() {
        let request = NfsExportRecipeRequest::new(
            "generated_data",
            StoreId::new("generated").expect("store id"),
            "/srv/dasobjectstore/generated/settled",
            " ",
        );

        let err = render_nfs_export_recipe(&request).expect_err("client spec rejected");

        assert_eq!(err, FileExportRecipeError::BlankClientSpec);
    }

    #[test]
    fn rejects_relative_nfs_settled_paths() {
        let request = NfsExportRecipeRequest::new(
            "generated_data",
            StoreId::new("generated").expect("store id"),
            "generated/settled",
            "localhost",
        );

        let err = render_nfs_export_recipe(&request).expect_err("relative path rejected");

        assert_eq!(
            err,
            FileExportRecipeError::RelativeSettledPath {
                path: PathBuf::from("generated/settled")
            }
        );
    }
}
