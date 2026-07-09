use crate::dashboard::{
    CapacitySummaryView, CreateObjectStoreAffordanceView, DasEnclosureCardView,
    DashboardHealthStateView, DashboardWarning, ObjectStoreCardView, ObjectStoresPageView,
    StorageGroupView, WriterPolicyReadinessView, REDESIGN_DASHBOARD_SCHEMA_VERSION,
};
use crate::groups_registry::{default_groups_registry_path, read_storage_groups_for_user};
use crate::home_aggregator::{env_path, now_utc_string, DEFAULT_SSD_ROOT};
use dasobjectstore_core::store::{ExportPolicy, MutabilityPolicy, PlacementStrategy};
use dasobjectstore_metadata::{
    read_store_contents, StoreContentsObject, StoreContentsRequest, LIVE_SQLITE_FILE_NAME,
    METADATA_DIR_NAME,
};
use dasobjectstore_object_service::{default_store_registry_path, read_store_registry};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
struct ObjectStoresAggregatorConfig {
    store_registry_path: PathBuf,
    live_sqlite_path: PathBuf,
    groups_registry_path: PathBuf,
    current_user_groups: Vec<String>,
    administrator: bool,
    mounted_enclosures: Option<Vec<DasEnclosureCardView>>,
}

impl ObjectStoresAggregatorConfig {
    fn from_env() -> Self {
        let default_live_sqlite_path = env_path("DASOBJECTSTORE_SSD_ROOT", DEFAULT_SSD_ROOT)
            .join(METADATA_DIR_NAME)
            .join(LIVE_SQLITE_FILE_NAME);
        Self {
            store_registry_path: default_store_registry_path(),
            live_sqlite_path: std::env::var_os("DASOBJECTSTORE_WEB_LIVE_SQLITE_PATH")
                .map(PathBuf::from)
                .unwrap_or(default_live_sqlite_path),
            groups_registry_path: default_groups_registry_path(),
            current_user_groups: Vec::new(),
            administrator: false,
            mounted_enclosures: None,
        }
    }
}

pub(crate) fn live_object_stores_dashboard() -> ObjectStoresPageView {
    build_object_stores_dashboard(ObjectStoresAggregatorConfig::from_env())
}

pub(crate) fn live_object_stores_dashboard_for_user(
    current_user_groups: Vec<String>,
    administrator: bool,
) -> ObjectStoresPageView {
    let mut config = ObjectStoresAggregatorConfig::from_env();
    config.current_user_groups = current_user_groups;
    config.administrator = administrator;
    build_object_stores_dashboard(config)
}

fn build_object_stores_dashboard(config: ObjectStoresAggregatorConfig) -> ObjectStoresPageView {
    let mut warnings = Vec::new();
    let groups_snapshot =
        read_storage_groups_for_user(&config.groups_registry_path, &config.current_user_groups);
    warnings.extend(groups_snapshot.warnings.clone());
    let stores = registry_object_store_cards(
        &config.store_registry_path,
        Some(&config.live_sqlite_path),
        &groups_snapshot.groups,
        &mut warnings,
    );
    let selected_store_id = stores.first().map(|store| store.store_id.clone());
    let mounted_enclosures = config.mounted_enclosures.unwrap_or_else(|| {
        crate::enclosures_aggregator::live_enclosures_dashboard_for_administrator(
            config.administrator,
        )
        .enclosures
    });
    let create_object_store = if !config.administrator {
        CreateObjectStoreAffordanceView::admin_required()
    } else if mounted_enclosures.is_empty() {
        CreateObjectStoreAffordanceView::enclosure_required()
    } else {
        CreateObjectStoreAffordanceView::enabled()
    };

    ObjectStoresPageView {
        schema_version: REDESIGN_DASHBOARD_SCHEMA_VERSION.to_string(),
        generated_at_utc: now_utc_string(),
        groups_file_path: groups_snapshot.path.display().to_string(),
        groups: groups_snapshot.groups,
        mounted_enclosures,
        stores,
        selected_store_id,
        create_object_store,
        warnings,
    }
}

pub(crate) fn registry_object_store_cards(
    registry_path: &Path,
    live_sqlite_path: Option<&Path>,
    groups: &[StorageGroupView],
    warnings: &mut Vec<DashboardWarning>,
) -> Vec<ObjectStoreCardView> {
    let definitions = match read_store_registry(registry_path) {
        Ok(definitions) => definitions,
        Err(error) => {
            warnings.push(DashboardWarning::new(
                "store_registry_unreadable",
                format!(
                    "ObjectStore registry {} could not be read: {error}.",
                    registry_path.display()
                ),
            ));
            return Vec::new();
        }
    };

    definitions
        .into_iter()
        .map(|definition| {
            let policy = definition.policy;
            let writer_group = definition.writer_group;
            let public = definition.public;
            let usage = live_sqlite_path
                .map(|path| store_usage_summary(path, definition.store_id.clone()))
                .unwrap_or_else(StoreUsageSummary::pending);
            let mut card_warnings = usage.warnings;
            if card_warnings.is_empty() && usage.object_count == 0 {
                card_warnings.push(DashboardWarning::new(
                    "store_empty",
                    "No objects have been recorded for this ObjectStore.",
                ));
            }
            ObjectStoreCardView {
                store_id: definition.store_id.to_string(),
                display_name: definition.store_id.to_string(),
                store_class: policy.class.name().to_string(),
                object_type: usage.object_type.unwrap_or_else(|| "naive".to_string()),
                health: DashboardHealthStateView::Healthy,
                required_copies: policy.copies,
                object_count: usage.object_count,
                capacity: used_capacity_summary(usage.used_bytes),
                placement_policy: placement_strategy_label(policy.placement_strategy).to_string(),
                endpoint_export_mode: export_policy_label(policy.export_policy).to_string(),
                writer_group: writer_group.clone(),
                public,
                writeable: policy.mutability_policy == MutabilityPolicy::Mutable
                    || policy.export_policy != ExportPolicy::Disabled,
                created_at_utc: "registry-managed".to_string(),
                last_ingested_at_utc: usage.last_updated_at_utc,
                writer_policy: writer_policy_readiness(writer_group.as_deref(), groups),
                warnings: card_warnings,
            }
        })
        .collect()
}

fn writer_policy_readiness(
    writer_group: Option<&str>,
    groups: &[StorageGroupView],
) -> WriterPolicyReadinessView {
    let Some(writer_group) = writer_group else {
        return WriterPolicyReadinessView::without_writer_group();
    };

    let group = groups
        .iter()
        .find(|group| group.group_name.as_str() == writer_group);
    match group {
        Some(group) if group.current_user_member => WriterPolicyReadinessView {
            writer_group: Some(writer_group.to_string()),
            group_defined: true,
            current_user_member: true,
            writeable_by_current_user: true,
            state: "ready".to_string(),
            message: "Current user belongs to the ObjectStore writer group.".to_string(),
        },
        Some(_) => WriterPolicyReadinessView {
            writer_group: Some(writer_group.to_string()),
            group_defined: true,
            current_user_member: false,
            writeable_by_current_user: false,
            state: "member_missing".to_string(),
            message: "Writer group is defined, but current user membership is not confirmed."
                .to_string(),
        },
        None => WriterPolicyReadinessView {
            writer_group: Some(writer_group.to_string()),
            group_defined: false,
            current_user_member: false,
            writeable_by_current_user: false,
            state: "group_unknown".to_string(),
            message: "Writer group is not present in the DASObjectStore groups registry."
                .to_string(),
        },
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct StoreUsageSummary {
    object_count: usize,
    used_bytes: u64,
    object_type: Option<String>,
    last_updated_at_utc: Option<String>,
    warnings: Vec<DashboardWarning>,
}

impl StoreUsageSummary {
    fn pending() -> Self {
        Self {
            warnings: vec![DashboardWarning::new(
                "store_usage_pending",
                "Object counts and per-store used capacity require daemon metadata aggregation.",
            )],
            ..Self::default()
        }
    }
}

fn store_usage_summary(
    live_sqlite_path: &Path,
    store_id: dasobjectstore_core::ids::StoreId,
) -> StoreUsageSummary {
    if !live_sqlite_path.exists() {
        return StoreUsageSummary::pending();
    }

    match read_store_contents(&StoreContentsRequest::new(
        live_sqlite_path,
        store_id.clone(),
    )) {
        Ok(snapshot) => usage_from_objects(&snapshot.objects),
        Err(error) => StoreUsageSummary {
            warnings: vec![DashboardWarning::new(
                "store_usage_unavailable",
                format!(
                    "Object usage for {store_id} could not be read from {}: {error}.",
                    live_sqlite_path.display()
                ),
            )],
            ..StoreUsageSummary::default()
        },
    }
}

fn usage_from_objects(objects: &[StoreContentsObject]) -> StoreUsageSummary {
    let object_count = objects.len();
    let used_bytes = objects.iter().map(|object| object.size_bytes).sum::<u64>();
    let object_types = objects
        .iter()
        .map(|object| object.object_type.as_str())
        .collect::<BTreeSet<_>>();
    let object_type = match object_types.len() {
        0 => None,
        1 => object_types.iter().next().map(|value| (*value).to_string()),
        _ => Some("mixed".to_string()),
    };
    let last_updated_at_utc = objects
        .iter()
        .map(|object| object.updated_at_utc.as_str())
        .max()
        .map(str::to_string);

    StoreUsageSummary {
        object_count,
        used_bytes,
        object_type,
        last_updated_at_utc,
        warnings: Vec::new(),
    }
}

fn used_capacity_summary(used_bytes: u64) -> CapacitySummaryView {
    CapacitySummaryView {
        total_tib: format_tib(used_bytes),
        used_tib: format_tib(used_bytes),
        free_tib: "0.0".to_string(),
        used_percent_basis_points: if used_bytes == 0 { 0 } else { 10_000 },
    }
}

fn format_tib(bytes: u64) -> String {
    const TIB: f64 = 1024.0 * 1024.0 * 1024.0 * 1024.0;
    format!("{:.1}", bytes as f64 / TIB)
}

fn placement_strategy_label(strategy: PlacementStrategy) -> &'static str {
    match strategy {
        PlacementStrategy::WeightedHealthCapacityPerformance => {
            "weighted_health_capacity_performance"
        }
    }
}

fn export_policy_label(policy: ExportPolicy) -> &'static str {
    match policy {
        ExportPolicy::S3 => "s3_bucket",
        ExportPolicy::ReadOnlyFileExport => "read_only_file_export",
        ExportPolicy::Disabled => "disabled",
    }
}

#[cfg(test)]
mod tests {
    use super::{build_object_stores_dashboard, ObjectStoresAggregatorConfig};
    use crate::dashboard::{
        CapacitySummaryView, DasEnclosureCardView, DashboardHealthStateView,
        EnclosureConnectionView,
    };
    use dasobjectstore_core::ids::StoreId;
    use dasobjectstore_core::store::{StoreClass, StorePolicy};
    use dasobjectstore_metadata::LIVE_SCHEMA_SQL;
    use dasobjectstore_object_service::layout::StoreServiceDefinition;
    use rusqlite::{params, Connection};
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn object_stores_aggregator_builds_registry_backed_cards() {
        let root = temp_root("object-stores-live");
        let registry_path = root.join("stores.json");
        let live_sqlite_path = root.join("live.sqlite");
        let groups_registry_path = root.join("groups.json");
        let mut policy = StorePolicy::defaults_for(StoreClass::GeneratedData);
        policy.copies = 3;
        let definition = StoreServiceDefinition {
            store_id: StoreId::new("zymo_fecal_2025.05").expect("store id"),
            policy,
            bucket_name: Some("dos-zymo-fecal".to_string()),
            reader_group: None,
            writer_group: Some("bioinformatics".to_string()),
            public: false,
        };
        fs::write(
            &registry_path,
            serde_json::to_string_pretty(&vec![definition]).expect("registry json"),
        )
        .expect("registry write");
        create_live_sqlite_with_store_objects(&live_sqlite_path, "zymo_fecal_2025.05");
        fs::write(
            &groups_registry_path,
            r#"{"groups":[{"group_name":"bioinformatics","display_name":"Bioinformatics","source":"local_os"}]}"#,
        )
        .expect("groups write");

        let view = build_object_stores_dashboard(ObjectStoresAggregatorConfig {
            store_registry_path: registry_path,
            live_sqlite_path,
            groups_registry_path,
            current_user_groups: vec!["bioinformatics".to_string()],
            administrator: true,
            mounted_enclosures: Some(vec![mounted_enclosure_fixture()]),
        });

        assert_eq!(view.groups.len(), 1);
        assert_eq!(view.groups[0].group_name, "bioinformatics");
        assert!(view.groups[0].current_user_member);
        assert_eq!(view.stores.len(), 1);
        assert_eq!(
            view.selected_store_id.as_deref(),
            Some("zymo_fecal_2025.05")
        );
        assert_eq!(view.stores[0].store_id, "zymo_fecal_2025.05");
        assert_eq!(view.stores[0].health, DashboardHealthStateView::Healthy);
        assert_eq!(view.stores[0].required_copies, 3);
        assert_eq!(view.stores[0].object_count, 2);
        assert_eq!(view.stores[0].object_type, "pod5");
        assert_eq!(view.stores[0].capacity.used_tib, "2.0");
        assert_eq!(
            view.stores[0].last_ingested_at_utc.as_deref(),
            Some("2026-07-08T08:30:00Z")
        );
        assert_eq!(
            view.stores[0].writer_group.as_deref(),
            Some("bioinformatics")
        );
        assert_eq!(view.stores[0].endpoint_export_mode, "s3_bucket");
        assert!(view.stores[0].writeable);
        assert_eq!(view.stores[0].writer_policy.state, "ready");
        assert!(view.stores[0].writer_policy.group_defined);
        assert!(view.stores[0].writer_policy.writeable_by_current_user);
        assert_eq!(
            view.mounted_enclosures[0].enclosure_id,
            "qnap-tl-d800c-managed"
        );
        assert!(view.create_object_store.enabled);
        assert!(view.stores[0].warnings.is_empty());
        assert!(view.warnings.is_empty());
    }

    #[test]
    fn object_stores_aggregator_reports_unreadable_registry() {
        let root = temp_root("object-stores-unreadable");
        let registry_path = root.join("registry-directory");
        fs::create_dir_all(&registry_path).expect("registry dir");

        let view = build_object_stores_dashboard(ObjectStoresAggregatorConfig {
            store_registry_path: registry_path,
            live_sqlite_path: root.join("missing-live.sqlite"),
            groups_registry_path: root.join("missing-groups.json"),
            current_user_groups: Vec::new(),
            administrator: false,
            mounted_enclosures: Some(Vec::new()),
        });

        assert!(view.stores.is_empty());
        assert_eq!(view.selected_store_id, None);
        assert!(!view.create_object_store.enabled);
        assert!(view
            .warnings
            .iter()
            .any(|warning| warning.code == "store_registry_unreadable"));
    }

    fn mounted_enclosure_fixture() -> DasEnclosureCardView {
        DasEnclosureCardView {
            enclosure_id: "qnap-tl-d800c-managed".to_string(),
            display_name: "QNAP TL-D800C".to_string(),
            mount_path: "/srv/dasobjectstore/hdd".to_string(),
            connection: EnclosureConnectionView {
                bus: "usb".to_string(),
                protocol: "uas/filesystem".to_string(),
                link_speed: "host reported".to_string(),
            },
            health: DashboardHealthStateView::Healthy,
            drive_count: crate::dashboard::DriveCountSummaryView {
                total: 8,
                mounted: 8,
                healthy: 8,
                watch: 0,
                suspect: 0,
                failed: 0,
            },
            capacity: CapacitySummaryView {
                total_tib: "100.0".to_string(),
                used_tib: "12.5".to_string(),
                free_tib: "87.5".to_string(),
                used_percent_basis_points: 1250,
            },
            last_seen_at_utc: "2026-07-08T08:30:00Z".to_string(),
            warnings: Vec::new(),
        }
    }

    fn create_live_sqlite_with_store_objects(path: &PathBuf, store_id: &str) {
        let connection = Connection::open(path).expect("open live sqlite");
        connection
            .execute_batch(LIVE_SCHEMA_SQL)
            .expect("live schema");
        connection
            .execute(
                "INSERT INTO pools (pool_id, state, created_at_utc, updated_at_utc)
                 VALUES ('pool-a', 'clean', '2026-07-08T08:00:00Z', '2026-07-08T08:00:00Z')",
                [],
            )
            .expect("pool insert");
        connection
            .execute(
                "INSERT INTO stores (
                    store_id, pool_id, class, policy_json, created_at_utc, updated_at_utc
                 ) VALUES (?1, 'pool-a', 'generated_data', '{}',
                    '2026-07-08T08:00:00Z', '2026-07-08T08:00:00Z')",
                params![store_id],
            )
            .expect("store insert");
        insert_object(
            &connection,
            store_id,
            "object-a",
            1_099_511_627_776_i64,
            "2026-07-08T08:10:00Z",
        );
        insert_object(
            &connection,
            store_id,
            "object-b",
            1_099_511_627_776_i64,
            "2026-07-08T08:30:00Z",
        );
    }

    fn insert_object(
        connection: &Connection,
        store_id: &str,
        object_id: &str,
        size_bytes: i64,
        updated_at_utc: &str,
    ) {
        connection
            .execute(
                "INSERT INTO objects (
                    object_id, store_id, object_type, state, size_bytes, content_hash,
                    created_at_utc, updated_at_utc
                 ) VALUES (
                    ?1, ?2, 'pod5', 'protected', ?3, 'sha256:test',
                    '2026-07-08T08:05:00Z', ?4
                 )",
                params![object_id, store_id, size_bytes, updated_at_utc],
            )
            .expect("object insert");
    }

    fn temp_root(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("dos-gui-{label}-{unique}"));
        fs::create_dir_all(&root).expect("temp root");
        root
    }
}
