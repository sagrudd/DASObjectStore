use super::*;
use crate::api::{
    SynoptikonProjectionPrepareRequest, SynoptikonProjectionPrepareResponse,
    SynoptikonProjectionSettleRequest, SynoptikonProjectionSettleResponse,
    SYNOPTIKON_PROJECTION_FIXED_PEER_USER, SYNOPTIKON_PROJECTION_PREPARE_V1_SCHEMA,
    SYNOPTIKON_PROJECTION_SETTLE_V1_SCHEMA,
};
use std::time::{SystemTime, UNIX_EPOCH};

fn fixed_peer(actor: Option<&DaemonLocalActor>) -> Result<(), DaemonApiResponse> {
    let actor = actor.ok_or_else(|| denied("projection_peer_required"))?;
    if actor.username.as_deref() != Some(SYNOPTIKON_PROJECTION_FIXED_PEER_USER) {
        return Err(denied("projection_peer_mismatch"));
    }
    Ok(())
}

fn denied(code: &str) -> DaemonApiResponse {
    DaemonApiResponse::Error(DaemonApiErrorResponse::new(
        code,
        "Synoptikon projection requires the fixed packaged service peer",
    ))
}

fn now() -> Result<u64, DaemonApiResponse> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(|_| {
            DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                "projection_clock_unavailable",
                "trusted system time is unavailable",
            ))
        })
}

pub(super) fn prepare<S, C>(
    handler: &DaemonRequestHandler<S, C>,
    request: SynoptikonProjectionPrepareRequest,
    actor: Option<&DaemonLocalActor>,
) -> DaemonApiResponse
where
    S: DaemonServiceOrchestrator,
    C: DaemonClock,
{
    if let Err(response) = fixed_peer(actor) {
        return response;
    }
    let result = crate::runtime::prepare_synoptikon_projection_intent(
        &handler.synoptikon_projection_ledger_path,
        &request.logical_name,
        request.size_bytes,
        &request.sha256,
        match now() {
            Ok(now) => now,
            Err(response) => return response,
        },
    );
    match result {
        Ok((intent, exact_replay)) => {
            DaemonApiResponse::SynoptikonProjectionPrepared(SynoptikonProjectionPrepareResponse {
                schema_version: SYNOPTIKON_PROJECTION_PREPARE_V1_SCHEMA.to_owned(),
                intent_id: intent.intent_id,
                projection_id: intent.projection.projection_id,
                object_store_id: intent.projection.object_store_id,
                object_key: intent.projection.object_key,
                object_version: intent.projection.object_version,
                generation: intent.projection.generation,
                expected_size_bytes: intent.projection.source_size_bytes,
                expected_sha256: intent.projection.source_sha256,
                expires_at_unix_seconds: intent.projection.expires_at_unix_seconds,
                exact_replay,
            })
        }
        Err(error) => DaemonApiResponse::Error(DaemonApiErrorResponse::new(
            "projection_prepare_denied",
            error.to_string(),
        )),
    }
}

pub(super) fn settle<S, C>(
    handler: &DaemonRequestHandler<S, C>,
    request: SynoptikonProjectionSettleRequest,
    actor: Option<&DaemonLocalActor>,
) -> DaemonApiResponse
where
    S: DaemonServiceOrchestrator,
    C: DaemonClock,
{
    if let Err(response) = fixed_peer(actor) {
        return response;
    }
    let settled_at = match now() {
        Ok(now) => now,
        Err(response) => return response,
    };
    let result = crate::runtime::commit_projection_settlement(
        &handler.synoptikon_projection_ledger_path,
        &request.intent_id,
        |projection, authority_sequence| {
            handler.derive_synoptikon_projection_settlement(
                projection,
                authority_sequence,
                settled_at,
            )
        },
    );
    match result {
        Ok((settlement_id, settlement, exact_replay)) => {
            DaemonApiResponse::SynoptikonProjectionSettlement(SynoptikonProjectionSettleResponse {
                schema_version: SYNOPTIKON_PROJECTION_SETTLE_V1_SCHEMA.to_owned(),
                settlement_id,
                settlement,
                exact_replay,
            })
        }
        Err(error) => DaemonApiResponse::Error(DaemonApiErrorResponse::new(
            "projection_settlement_denied",
            error.to_string(),
        )),
    }
}
