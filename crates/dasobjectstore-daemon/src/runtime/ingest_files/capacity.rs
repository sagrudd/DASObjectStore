use super::endpoint::FileIngestEntry;
use super::{DaemonIngestFilesRuntimeError, IngestJobId};
use crate::api::DaemonIngressOrigin;
use crate::runtime::capacity_provider::CapacityAdmissionProvider;
use dasobjectstore_core::ids::StoreId;
use std::collections::HashMap;
use std::sync::Arc;

pub(super) struct IngestCapacityReservations {
    provider: Option<Arc<dyn CapacityAdmissionProvider>>,
    store_id: StoreId,
    reservations: HashMap<String, String>,
}

impl IngestCapacityReservations {
    pub(super) fn new(
        provider: Option<Arc<dyn CapacityAdmissionProvider>>,
        store_id: StoreId,
    ) -> Self {
        Self {
            provider,
            store_id,
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
        let reservation_id = format!("{job_id}/{}", entry.object_id);
        let response = provider
            .admit_ingest(
                self.store_id.as_str(),
                entry.size_bytes,
                copies,
                ingress_origin,
                &reservation_id,
            )
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
        provider
            .commit(&self.store_id, reservation_id)
            .map_err(|error| {
                DaemonIngestFilesRuntimeError::CommandFailed(format!(
                    "capacity commit failed for {}: {error}",
                    entry.relative_path.display()
                ))
            })?;
        self.reservations.remove(entry.object_id.as_str());
        Ok(())
    }
}

impl Drop for IngestCapacityReservations {
    fn drop(&mut self) {
        let Some(provider) = &self.provider else {
            return;
        };
        for reservation_id in self.reservations.values() {
            let _ = provider.release(&self.store_id, reservation_id);
        }
    }
}
