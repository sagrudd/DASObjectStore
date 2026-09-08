//! Role-correct internal retention composition: R observes, W conditionally puts.
use super::*;
use dasobjectstore_object_service::{
    retain_custody_object_with_readback, CustodyIntegrityReceiptV1, CustodyObjectInputV1,
};
#[cfg(test)]
mod tests;

pub(super) fn observe_state<R: ServiceCommandRunner>(
    runner: &R,
    endpoint: &str,
    bucket: &str,
    environment: &[(String, String)],
    object_key: &str,
    policy: &GarageCustodyS3Writer<'_, R>,
) -> Result<CustodyObjectState, ObjectServiceError> {
    let args = head_args(bucket, object_key, endpoint);
    match runner.run_with_display_args_and_env("aws", &args, &args, environment) {
        Ok(output) => {
            let head: GarageHead = serde_json::from_str(&output.stdout).map_err(|error| {
                invalid(format!("custody Garage HEAD response is invalid: {error}"))
            })?;
            let sha = head
                .metadata
                .dasobjectstore_sha256
                .as_deref()
                .ok_or_else(|| invalid("custody Garage object omits SHA-256 metadata"))?;
            // Use the actual writer's selected sealed retention, never a default.
            policy.validate_object_lock_metadata(&head.metadata)?;
            Ok(CustodyObjectState::Existing {
                content_sha256: sha.to_string(),
                content_length: head.content_length,
            })
        }
        Err(error) if is_missing_bucket_error(&error.to_string()) => {
            Ok(CustodyObjectState::Missing)
        }
        Err(error) => Err(runtime_error(error)),
    }
}

struct RoleSeparatedWriter<'a, 'w, 'r, R> {
    writer: &'a mut GarageCustodyS3Writer<'w, R>,
    reader: &'a GarageCustodyS3Reader<'r, R>,
}
impl<R: ServiceCommandRunner> CustodyObjectWriter for RoleSeparatedWriter<'_, '_, '_, R> {
    fn identity(&self) -> &str {
        self.writer.identity()
    }
    fn object_state(&mut self, key: &str) -> Result<CustodyObjectState, ObjectServiceError> {
        observe_state(
            self.reader.runner,
            &self.reader.endpoint,
            &self.reader.bucket,
            &self.reader.environment,
            key,
            self.writer,
        )
    }
    fn put_if_absent(&mut self, key: &str, bytes: &[u8]) -> Result<(), ObjectServiceError> {
        self.writer.put_if_absent(key, bytes)
    }
}

struct Readback<'a, 'r, R>(&'a GarageCustodyS3Reader<'r, R>);
impl<R: ServiceCommandRunner> CustodyObjectReader for Readback<'_, '_, R> {
    fn identity(&self) -> &str {
        self.0.identity()
    }
    fn read_exact(&mut self, key: &str) -> Result<Vec<u8>, ObjectServiceError> {
        self.0.read_exact_shared(key)
    }
}

/// Concrete internal bridge, not admission or a new public capability.
/// Existing retainer checks sealed identities before backend activity. Borrowing
/// the same R adapter for HEAD/GET neither clones secrets nor reconsumes a handoff.
pub(crate) fn retain_garage_custody_object_with_readback<R: ServiceCommandRunner>(
    path: impl AsRef<Path>,
    input: CustodyObjectInputV1,
    writer: &mut GarageCustodyS3Writer<'_, R>,
    reader: &GarageCustodyS3Reader<'_, R>,
) -> Result<CustodyIntegrityReceiptV1, ObjectServiceError> {
    if writer.endpoint != reader.endpoint
        || writer.bucket != reader.bucket
        || writer.identity == reader.identity
    {
        return Err(invalid("custody retention reader/writer binding mismatch"));
    }
    let mut combined = RoleSeparatedWriter { writer, reader };
    let mut readback = Readback(reader);
    retain_custody_object_with_readback(path, input, &mut combined, &mut readback)
}
