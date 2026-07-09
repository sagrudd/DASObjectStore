use crate::dashboard::{
    CapacitySummaryView, DashboardWarning, ObjectStoreCardView, REDESIGN_DASHBOARD_SCHEMA_VERSION,
};
use crate::home_aggregator::now_utc_string;
use dasobjectstore_core::ingress::{IngressLandingMode, IngressOrigin};
use dasobjectstore_core::remote_upload::RemoteUploadBackpressurePolicy;
use dasobjectstore_daemon::{
    remote_easyconnect_object_store_grants_for_actor, DaemonLocalActor,
    RemoteEasyconnectObjectStoreAccessPolicy,
};
use dasobjectstore_object_service::{
    bucket_name_for_definition, default_store_registry_path, read_store_registry,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemoteUploadWorkspaceView {
    pub schema_version: String,
    pub generated_at_utc: String,
    pub actor: RemoteUploadActorView,
    pub ingress_policy: RemoteUploadIngressPolicyView,
    pub backpressure_policy: RemoteUploadBackpressurePolicy,
    pub stores: Vec<RemoteUploadObjectStoreView>,
    pub warnings: Vec<DashboardWarning>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemoteUploadActorView {
    pub username: String,
    pub groups: Vec<String>,
    pub sudo_administrator: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemoteUploadIngressPolicyView {
    pub browser_ingress_origin: IngressOrigin,
    pub browser_landing_mode: IngressLandingMode,
    pub paired_agent_ingress_origin: IngressOrigin,
    pub paired_agent_landing_mode: IngressLandingMode,
}

impl RemoteUploadIngressPolicyView {
    fn standard() -> Self {
        Self {
            browser_ingress_origin: IngressOrigin::WebUpload,
            browser_landing_mode: IngressOrigin::WebUpload.landing_mode(),
            paired_agent_ingress_origin: IngressOrigin::RemoteS3,
            paired_agent_landing_mode: IngressOrigin::RemoteS3.landing_mode(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemoteUploadObjectStoreView {
    pub store_id: String,
    pub display_name: String,
    pub bucket: String,
    pub store_class: String,
    pub object_type: String,
    pub capacity: CapacitySummaryView,
    pub writer_group: Option<String>,
    pub writer_policy_state: String,
    pub public: bool,
    pub endpoint_export_mode: String,
    pub upload_allowed: bool,
    pub upload_state: String,
    pub upload_message: String,
    pub warnings: Vec<DashboardWarning>,
}

pub(crate) fn live_remote_upload_workspace_for_user(
    username: String,
    groups: Vec<String>,
    sudo_administrator: bool,
) -> RemoteUploadWorkspaceView {
    let dashboard = crate::object_stores_aggregator::live_object_stores_dashboard_for_user(
        groups.clone(),
        sudo_administrator,
    );
    let mut warnings = dashboard.warnings;
    let registry_path = default_store_registry_path();
    let definitions = match read_store_registry(&registry_path) {
        Ok(definitions) => definitions,
        Err(error) => {
            warnings.push(DashboardWarning::new(
                "store_registry_unreadable",
                format!(
                    "ObjectStore registry {} could not be read for remote upload filtering: {error}.",
                    registry_path.display()
                ),
            ));
            Vec::new()
        }
    };

    let actor = DaemonLocalActor::new(0)
        .with_username(username.clone())
        .with_groups(groups.clone());
    let policies = definitions
        .iter()
        .filter_map(|definition| {
            let bucket = match bucket_name_for_definition(definition) {
                Ok(bucket) => bucket,
                Err(error) => {
                    warnings.push(DashboardWarning::new(
                        "remote_upload_bucket_unavailable",
                        format!(
                            "ObjectStore {} bucket routing is invalid for remote uploads: {error}.",
                            definition.store_id
                        ),
                    ));
                    return None;
                }
            };
            Some(RemoteEasyconnectObjectStoreAccessPolicy {
                object_store: definition.store_id.to_string(),
                bucket,
                reader_group: definition.reader_group.clone(),
                writer_group: definition.writer_group.clone(),
                admin_group: None,
                public_read: definition.public,
                writable: true,
                object_type: definition.policy.class.name().to_string(),
            })
        })
        .collect::<Vec<_>>();
    let grants = match remote_easyconnect_object_store_grants_for_actor(&actor, &policies) {
        Ok(grants) => grants,
        Err(error) => {
            warnings.push(DashboardWarning::new(
                "remote_upload_grants_unavailable",
                format!("Remote upload ObjectStore grants could not be evaluated: {error}."),
            ));
            Vec::new()
        }
    };
    let grants_by_store = grants
        .into_iter()
        .map(|grant| (grant.object_store.clone(), grant))
        .collect::<BTreeMap<_, _>>();

    let stores = dashboard
        .stores
        .into_iter()
        .filter_map(|store| {
            let grant = grants_by_store.get(&store.store_id)?;
            Some(remote_upload_store_view(store, grant))
        })
        .collect::<Vec<_>>();

    if stores.is_empty() && warnings.is_empty() {
        warnings.push(DashboardWarning::new(
            "remote_upload_no_accessible_stores",
            "No ObjectStores are readable by the authenticated user for remote upload.",
        ));
    }

    RemoteUploadWorkspaceView {
        schema_version: REDESIGN_DASHBOARD_SCHEMA_VERSION.to_string(),
        generated_at_utc: now_utc_string(),
        actor: RemoteUploadActorView {
            username,
            groups,
            sudo_administrator,
        },
        ingress_policy: RemoteUploadIngressPolicyView::standard(),
        backpressure_policy: RemoteUploadBackpressurePolicy::default(),
        stores,
        warnings: unique_warnings(warnings),
    }
}

fn remote_upload_store_view(
    store: ObjectStoreCardView,
    grant: &dasobjectstore_daemon::RemoteEasyconnectObjectStoreGrant,
) -> RemoteUploadObjectStoreView {
    let upload_allowed = grant.can_write && store.writeable && store.endpoint_export_mode == "s3";
    let (upload_state, upload_message) = if upload_allowed {
        (
            "ready",
            "Remote upload is allowed for this ObjectStore and current user.",
        )
    } else if !grant.can_write {
        (
            "read_only",
            "The current user can inspect this ObjectStore but lacks writer-group upload rights.",
        )
    } else if !store.writeable {
        (
            "locked",
            "The ObjectStore is not currently writable by policy.",
        )
    } else {
        (
            "export_unavailable",
            "Remote uploads require an S3-exported ObjectStore endpoint.",
        )
    };

    RemoteUploadObjectStoreView {
        store_id: store.store_id,
        display_name: store.display_name,
        bucket: grant.bucket.clone(),
        store_class: store.store_class,
        object_type: store.object_type,
        capacity: store.capacity,
        writer_group: store.writer_group,
        writer_policy_state: store.writer_policy.state,
        public: store.public,
        endpoint_export_mode: store.endpoint_export_mode,
        upload_allowed,
        upload_state: upload_state.to_string(),
        upload_message: upload_message.to_string(),
        warnings: store.warnings,
    }
}

fn unique_warnings(warnings: Vec<DashboardWarning>) -> Vec<DashboardWarning> {
    let mut seen = BTreeSet::new();
    warnings
        .into_iter()
        .filter(|warning| seen.insert((warning.code.clone(), warning.message.clone())))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{remote_upload_store_view, RemoteUploadIngressPolicyView};
    use crate::dashboard::{
        CapacitySummaryView, DashboardHealthStateView, ObjectStoreCardView,
        WriterPolicyReadinessView,
    };
    use dasobjectstore_core::ingress::{IngressLandingMode, IngressOrigin};
    use dasobjectstore_core::remote_upload::{
        RemoteUploadBackpressureAction, RemoteUploadBackpressurePolicy,
    };
    use dasobjectstore_daemon::RemoteEasyconnectObjectStoreGrant;

    #[test]
    fn remote_upload_store_marks_writer_s3_store_ready() {
        let view = remote_upload_store_view(card(true, true, "s3"), &grant(true));

        assert!(view.upload_allowed);
        assert_eq!(view.upload_state, "ready");
        assert_eq!(view.bucket, "dos-generated");
    }

    #[test]
    fn remote_upload_store_denies_read_only_grant() {
        let view = remote_upload_store_view(card(true, true, "s3"), &grant(false));

        assert!(!view.upload_allowed);
        assert_eq!(view.upload_state, "read_only");
    }

    #[test]
    fn remote_upload_store_requires_s3_export() {
        let view = remote_upload_store_view(card(true, true, "disabled"), &grant(true));

        assert!(!view.upload_allowed);
        assert_eq!(view.upload_state, "export_unavailable");
    }

    #[test]
    fn remote_upload_workspace_uses_shared_ingress_policy() {
        let policy = RemoteUploadIngressPolicyView::standard();

        assert_eq!(policy.browser_ingress_origin, IngressOrigin::WebUpload);
        assert_eq!(policy.browser_landing_mode, IngressLandingMode::SsdFirst);
        assert_eq!(policy.paired_agent_ingress_origin, IngressOrigin::RemoteS3);
        assert_eq!(
            policy.paired_agent_landing_mode,
            IngressLandingMode::SsdFirst
        );

        let encoded = serde_json::to_value(policy).expect("policy serializes");
        assert_eq!(encoded["browser_ingress_origin"], "web_upload");
        assert_eq!(encoded["paired_agent_ingress_origin"], "remote_s3");
    }

    #[test]
    fn remote_upload_workspace_uses_bounded_backpressure_policy() {
        let policy = RemoteUploadBackpressurePolicy::default();

        assert_eq!(policy.max_s3_transfer_concurrency, 2);
        assert_eq!(policy.max_ssd_stage_queue_depth, 4);
        assert_eq!(
            policy.ssd_high_pressure_action,
            RemoteUploadBackpressureAction::PauseNewTransfers
        );

        let encoded = serde_json::to_value(policy).expect("policy serializes");
        assert_eq!(encoded["max_s3_transfer_concurrency"], 2);
        assert_eq!(encoded["ssd_high_pressure_action"], "pause_new_transfers");
    }

    fn card(
        writeable: bool,
        writer_member: bool,
        endpoint_export_mode: &str,
    ) -> ObjectStoreCardView {
        ObjectStoreCardView {
            store_id: "generated".to_string(),
            display_name: "generated".to_string(),
            store_class: "generated_data".to_string(),
            object_type: "fastq".to_string(),
            health: DashboardHealthStateView::Healthy,
            required_copies: 2,
            object_count: 4,
            capacity: CapacitySummaryView {
                total_tib: "4.0".to_string(),
                used_tib: "1.0".to_string(),
                free_tib: "3.0".to_string(),
                used_percent_basis_points: 2500,
            },
            placement_policy: "weighted".to_string(),
            endpoint_export_mode: endpoint_export_mode.to_string(),
            writer_group: Some("mnemosyne".to_string()),
            public: false,
            writeable,
            created_at_utc: "registry-managed".to_string(),
            last_ingested_at_utc: None,
            writer_policy: WriterPolicyReadinessView {
                writer_group: Some("mnemosyne".to_string()),
                group_defined: true,
                current_user_member: writer_member,
                writeable_by_current_user: writer_member,
                state: if writer_member {
                    "ready"
                } else {
                    "member_missing"
                }
                .to_string(),
                message: "writer policy".to_string(),
            },
            warnings: Vec::new(),
        }
    }

    fn grant(can_write: bool) -> RemoteEasyconnectObjectStoreGrant {
        RemoteEasyconnectObjectStoreGrant {
            object_store: "generated".to_string(),
            bucket: "dos-generated".to_string(),
            can_read: true,
            can_write,
            writer_group: Some("mnemosyne".to_string()),
            object_type: "generated_data".to_string(),
        }
    }
}
