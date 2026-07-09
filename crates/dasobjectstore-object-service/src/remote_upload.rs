//! Remote S3-compatible upload planning.

use crate::layout::validate_bucket_name;
use crate::provider::ObjectServiceError;
use dasobjectstore_core::ids::StoreId;
use dasobjectstore_core::ingress::{IngressLandingMode, IngressOrigin};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteS3AuthAuthority {
    Mneion,
    LocalPassword,
}

impl RemoteS3AuthAuthority {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Mneion => "mneion",
            Self::LocalPassword => "local_password",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemoteS3UploadPlanRequest {
    pub store_id: StoreId,
    pub bucket_name: String,
    pub endpoint_url: String,
    pub region: String,
    pub profile_name: String,
    pub credential_reference: String,
    pub auth_authority: RemoteS3AuthAuthority,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemoteS3UploadPlan {
    pub store_id: StoreId,
    pub ingress_origin: IngressOrigin,
    pub landing_mode: IngressLandingMode,
    pub bucket_name: String,
    pub endpoint_url: String,
    pub region: String,
    pub profile_name: String,
    pub credential_reference: String,
    pub auth_authority: RemoteS3AuthAuthority,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    pub credential_instruction: String,
    pub aws_profile_commands: Vec<String>,
    pub aws_s3api_put_object_command: String,
    pub aws_s3_cp_command: String,
    pub aws_s3_sync_command: String,
}

pub fn plan_remote_s3_upload(
    request: RemoteS3UploadPlanRequest,
) -> Result<RemoteS3UploadPlan, ObjectServiceError> {
    let bucket_name = require_non_blank("bucket_name", request.bucket_name)?;
    validate_bucket_name(&bucket_name)?;
    let endpoint_url = require_non_blank("endpoint_url", request.endpoint_url)?;
    if !endpoint_url.starts_with("http://") && !endpoint_url.starts_with("https://") {
        return Err(ObjectServiceError::InvalidConfiguration(
            "endpoint_url must start with http:// or https://".to_string(),
        ));
    }
    let region = require_non_blank("region", request.region)?;
    let profile_name = require_non_blank("profile_name", request.profile_name)?;
    let credential_reference =
        require_non_blank("credential_reference", request.credential_reference)?;
    let username = request
        .username
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    if request.auth_authority == RemoteS3AuthAuthority::LocalPassword && username.is_none() {
        return Err(ObjectServiceError::InvalidConfiguration(
            "local_password authentication requires username".to_string(),
        ));
    }

    let credential_instruction = match request.auth_authority {
        RemoteS3AuthAuthority::Mneion => format!(
            "Authenticate to Mneion and request S3 credentials for {credential_reference}; configure the returned access key and secret key in the AWS profile below."
        ),
        RemoteS3AuthAuthority::LocalPassword => format!(
            "Authenticate to the DASObjectStore appliance as local user {} and request or rotate S3 credentials for {credential_reference}; configure the returned access key and secret key in the AWS profile below.",
            username.as_deref().unwrap_or("<username>")
        ),
    };

    let quoted_profile = shell_quote(&profile_name);
    let quoted_region = shell_quote(&region);
    let quoted_endpoint = shell_quote(&endpoint_url);
    let quoted_bucket_uri = shell_quote(&format!("s3://{bucket_name}/<object-key>"));
    let quoted_bucket_prefix_uri = shell_quote(&format!("s3://{bucket_name}/<prefix>/"));
    let quoted_bucket = shell_quote(&bucket_name);

    Ok(RemoteS3UploadPlan {
        store_id: request.store_id,
        ingress_origin: IngressOrigin::RemoteS3,
        landing_mode: IngressOrigin::RemoteS3.landing_mode(),
        bucket_name,
        endpoint_url,
        region,
        profile_name,
        credential_reference,
        auth_authority: request.auth_authority,
        username,
        credential_instruction,
        aws_profile_commands: vec![
            format!("aws configure set profile.{quoted_profile}.region {quoted_region}"),
            format!("aws configure set profile.{quoted_profile}.s3.addressing_style path"),
            format!(
                "aws configure set profile.{quoted_profile}.aws_access_key_id \"$DASOBJECTSTORE_S3_ACCESS_KEY_ID\""
            ),
            format!(
                "aws configure set profile.{quoted_profile}.aws_secret_access_key \"$DASOBJECTSTORE_S3_SECRET_ACCESS_KEY\""
            ),
        ],
        aws_s3api_put_object_command: format!(
            "aws --profile {quoted_profile} --endpoint-url {quoted_endpoint} s3api put-object --bucket {quoted_bucket} --key <object-key> --body <local-file>"
        ),
        aws_s3_cp_command: format!(
            "aws --profile {quoted_profile} --endpoint-url {quoted_endpoint} s3 cp <local-file> {quoted_bucket_uri}"
        ),
        aws_s3_sync_command: format!(
            "aws --profile {quoted_profile} --endpoint-url {quoted_endpoint} s3 sync <local-directory> {quoted_bucket_prefix_uri}"
        ),
    })
}

fn require_non_blank(field: &str, value: String) -> Result<String, ObjectServiceError> {
    let value = value.trim().to_string();
    if value.is_empty() {
        return Err(ObjectServiceError::InvalidConfiguration(format!(
            "{field} must not be blank"
        )));
    }
    Ok(value)
}

fn shell_quote(value: &str) -> String {
    if value
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || "-_./:=@%+".contains(character))
    {
        return value.to_string();
    }

    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::{plan_remote_s3_upload, RemoteS3AuthAuthority, RemoteS3UploadPlanRequest};
    use dasobjectstore_core::ids::StoreId;
    use dasobjectstore_core::ingress::{IngressLandingMode, IngressOrigin};

    #[test]
    fn renders_mneion_remote_upload_plan() {
        let plan = plan_remote_s3_upload(request(RemoteS3AuthAuthority::Mneion, None))
            .expect("plan renders");

        assert_eq!(plan.ingress_origin, IngressOrigin::RemoteS3);
        assert_eq!(plan.landing_mode, IngressLandingMode::SsdFirst);
        assert_eq!(plan.auth_authority, RemoteS3AuthAuthority::Mneion);
        assert_eq!(plan.bucket_name, "dos-generated-data");
        assert!(plan
            .credential_instruction
            .contains("Authenticate to Mneion"));
        assert!(plan
            .aws_s3api_put_object_command
            .contains("s3api put-object"));
        assert!(plan.aws_s3_cp_command.contains("s3 cp <local-file>"));
        assert!(plan
            .aws_s3_sync_command
            .contains("s3 sync <local-directory>"));
    }

    #[test]
    fn local_password_requires_username() {
        let err = plan_remote_s3_upload(request(RemoteS3AuthAuthority::LocalPassword, None))
            .expect_err("missing username must fail");

        assert!(err
            .to_string()
            .contains("local_password authentication requires username"));
    }

    #[test]
    fn renders_local_password_remote_upload_plan() {
        let plan =
            plan_remote_s3_upload(request(RemoteS3AuthAuthority::LocalPassword, Some("alice")))
                .expect("plan renders");

        assert_eq!(plan.username.as_deref(), Some("alice"));
        assert!(plan.credential_instruction.contains("local user alice"));
    }

    #[test]
    fn remote_upload_plan_serializes_stable_ingress_classification() {
        let plan = plan_remote_s3_upload(request(RemoteS3AuthAuthority::Mneion, None))
            .expect("plan renders");
        let json = serde_json::to_value(plan).expect("plan serializes");

        assert_eq!(json["ingress_origin"], serde_json::json!("remote_s3"));
        assert_eq!(json["landing_mode"], serde_json::json!("ssd_first"));
    }

    #[test]
    fn rejects_invalid_explicit_bucket_name() {
        let mut request = request(RemoteS3AuthAuthority::Mneion, None);
        request.bucket_name = "Invalid_Bucket".to_string();

        let err = plan_remote_s3_upload(request).expect_err("invalid bucket must fail");

        assert!(err
            .to_string()
            .contains("must contain only lowercase letters"));
    }

    fn request(
        auth_authority: RemoteS3AuthAuthority,
        username: Option<&str>,
    ) -> RemoteS3UploadPlanRequest {
        RemoteS3UploadPlanRequest {
            store_id: StoreId::new("generated-data").expect("store id"),
            bucket_name: "dos-generated-data".to_string(),
            endpoint_url: "http://appliance.local:3900".to_string(),
            region: "garage".to_string(),
            profile_name: "dasobjectstore-generated-data".to_string(),
            credential_reference: "secret://dasobjectstore/stores/generated-data/s3".to_string(),
            auth_authority,
            username: username.map(ToOwned::to_owned),
        }
    }
}
