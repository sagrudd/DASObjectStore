use super::endpoint::FileIngestEntry;
use super::{DaemonIngestFilesRuntimeError, IngestJobId};
use crate::api::DaemonIngressOrigin;
use crate::runtime::capacity_provider::CapacityAdmissionProvider;
use dasobjectstore_core::ids::StoreId;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Arc;

pub(super) fn reservation_scope(request: &super::SubmitIngestFilesRequest) -> String {
    let scope = request
        .client_request_id
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| request.source_path.to_string_lossy().into_owned());
    let digest = Sha256::digest(scope.as_bytes());
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub(super) struct IngestCapacityReservations {
    provider: Option<Arc<dyn CapacityAdmissionProvider>>,
    store_id: StoreId,
    subobject_name: Option<String>,
    scope: String,
    reservations: HashMap<String, String>,
}

impl IngestCapacityReservations {
    pub(super) fn new(
        provider: Option<Arc<dyn CapacityAdmissionProvider>>,
        store_id: StoreId,
        subobject_name: Option<String>,
        scope: String,
    ) -> Self {
        Self {
            provider,
            store_id,
            subobject_name,
            scope,
            reservations: HashMap::new(),
        }
    }

    pub(super) fn admit(
        &mut self,
        job_id: &IngestJobId,
        entry: &FileIngestEntry,
        copies: u8,
        ingress_origin: DaemonIngressOrigin,
    ) -> Result<(), DaemonIngestFilesRuntimeError> {
        let Some(provider) = &self.provider else {
            return Ok(());
        };
        let reservation_id = format!("{job_id}/{}/{}", self.scope, entry.object_id);
        let response = match self.subobject_name.as_deref() {
            Some(subobject) => provider.admit_subobject_ingest(
                self.store_id.as_str(),
                subobject,
                entry.size_bytes,
                copies,
                ingress_origin,
                &reservation_id,
            ),
            None => provider.admit_ingest(
                self.store_id.as_str(),
                entry.size_bytes,
                copies,
                ingress_origin,
                &reservation_id,
            ),
        }
        .map_err(|error| {
            DaemonIngestFilesRuntimeError::CommandFailed(format!(
                "capacity admission failed for {}: {error}",
                entry.relative_path.display()
            ))
        })?;
        if response.decision != crate::api::CapacityAdmissionDecision::Admitted {
            return Err(DaemonIngestFilesRuntimeError::CommandFailed(format!(
                "capacity admission rejected for {}: {}",
                entry.relative_path.display(),
                response
                    .message
                    .unwrap_or_else(|| "capacity policy rejected the object".to_string())
            )));
        }
        self.reservations
            .insert(entry.object_id.as_str().to_string(), reservation_id);
        Ok(())
    }

    pub(super) fn commit(
        &mut self,
        entry: &FileIngestEntry,
    ) -> Result<(), DaemonIngestFilesRuntimeError> {
        let Some(reservation_id) = self.reservations.get(entry.object_id.as_str()) else {
            return Ok(());
        };
        let Some(provider) = &self.provider else {
            return Ok(());
        };
        let result = match self.subobject_name.as_deref() {
            Some(subobject) => provider.commit_subobject(&self.store_id, subobject, reservation_id),
            None => provider.commit(&self.store_id, reservation_id),
        };
        result.map_err(|error| {
            DaemonIngestFilesRuntimeError::CommandFailed(format!(
                "capacity commit failed for {}: {error}",
                entry.relative_path.display()
            ))
        })?;
        self.reservations.remove(entry.object_id.as_str());
        Ok(())
    }

    pub(super) fn release(
        &mut self,
        entry: &FileIngestEntry,
    ) -> Result<(), DaemonIngestFilesRuntimeError> {
        let Some(reservation_id) = self.reservations.remove(entry.object_id.as_str()) else {
            return Ok(());
        };
        let Some(provider) = &self.provider else {
            return Ok(());
        };
        let result = match self.subobject_name.as_deref() {
            Some(subobject) => {
                provider.release_subobject(&self.store_id, subobject, &reservation_id)
            }
            None => provider.release(&self.store_id, &reservation_id),
        };
        result.map_err(|error| {
            DaemonIngestFilesRuntimeError::CommandFailed(format!(
                "capacity release failed for {} after source read failure: {error}",
                entry.relative_path.display()
            ))
        })
    }
}

impl Drop for IngestCapacityReservations {
    fn drop(&mut self) {
        let Some(provider) = &self.provider else {
            return;
        };
        for reservation_id in self.reservations.values() {
            let _ = match self.subobject_name.as_deref() {
                Some(subobject) => {
                    provider.release_subobject(&self.store_id, subobject, reservation_id)
                }
                None => provider.release(&self.store_id, reservation_id),
            };
        }
    }
}
