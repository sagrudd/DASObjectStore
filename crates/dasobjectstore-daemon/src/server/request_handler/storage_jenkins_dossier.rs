//! Daemon-owned retained Jenkins dossier evidence read-back.

use super::*;
use crate::api::{
    DaemonApiErrorResponse, JenkinsDossierEvidenceSettlementRequest,
    JenkinsDossierEvidenceSettlementResponse, ProviderStreamOpenRequest,
    PROVIDER_STREAM_SCHEMA_VERSION,
};

pub(super) fn settle<S, C>(
    handler: &DaemonRequestHandler<S, C>,
    request: JenkinsDossierEvidenceSettlementRequest,
    actor: Option<&DaemonLocalActor>,
) -> DaemonApiResponse
where
    S: DaemonServiceOrchestrator,
    C: DaemonClock,
{
    // `verified_subject` is required by the request schema. Supplying it to
    // this existing provider seam makes the daemon bind it to the fixed
    // service peer; there is no delegated POSIX or application-capability
    // alternative in this endpoint.
    let provider_request = ProviderStreamOpenRequest {
        schema_version: PROVIDER_STREAM_SCHEMA_VERSION.to_owned(),
        request_id: request.request_id.clone(),
        store_id: request.store_id(),
        object: dasobjectstore_core::BackendObjectKey {
            object_id: request.projection.object_id.clone(),
            version: request.projection.object_version,
        },
        delegated_actor: None,
        verified_subject: Some(request.verified_subject.clone()),
        application_capability: None,
        synoptikon_projection: None,
        range: None,
        condition: Default::default(),
        chunk_size_bytes: 64 * 1024,
    };
    let source = match handler.open_provider_stream(&provider_request, actor) {
        Ok(source) => source,
        Err(response) => return response,
    };
    if source.expected_size_bytes != request.projection.size_bytes {
        return DaemonApiResponse::Error(DaemonApiErrorResponse::new(
            "jenkins_dossier_readback_mismatch",
            "provider size differs from the canonical dossier evidence projection",
        ));
    }
    if source.expected_checksum.as_deref().is_some_and(|checksum| {
        checksum.trim_start_matches("sha256:") != request.projection.content_sha256
    }) {
        return DaemonApiResponse::Error(DaemonApiErrorResponse::new(
            "jenkins_dossier_readback_mismatch",
            "provider checksum differs from the canonical dossier evidence projection",
        ));
    }
    let evidence = match request.projection.project() {
        Ok(evidence) => evidence,
        Err(_) => {
            return DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                "jenkins_dossier_invalid_projection",
                "canonical dossier evidence projection is invalid",
            ))
        }
    };
    let mut reader = source.reader;
    let readback = match dasobjectstore_core::verify_jenkins_dossier_readback(evidence, &mut reader)
    {
        Ok(readback) => readback,
        Err(error) => {
            return DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                "jenkins_dossier_readback_mismatch",
                error.to_string(),
            ))
        }
    };
    DaemonApiResponse::JenkinsDossierEvidenceSettlement(JenkinsDossierEvidenceSettlementResponse {
        schema_version: crate::api::JENKINS_DOSSIER_EVIDENCE_SETTLEMENT_V1_SCHEMA.to_owned(),
        request_id: request.request_id,
        evidence: readback.evidence,
        size_bytes: readback.size_bytes,
        content_sha256: readback.content_sha256,
        observed_at_utc: handler.clock.now_utc(),
    })
}
