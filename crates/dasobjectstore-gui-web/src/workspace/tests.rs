fn workspace_component_source() -> String {
    [
        include_str!("home.rs"),
        include_str!("enclosures.rs"),
        include_str!("object_stores.rs"),
        include_str!("remote_upload.rs"),
        include_str!("object_browser.rs"),
        include_str!("object_store_create.rs"),
        include_str!("object_store_configure.rs"),
        include_str!("subobjects.rs"),
        include_str!("users_groups.rs"),
        include_str!("../endpoints.rs"),
        include_str!("activity.rs"),
        include_str!("bioinformatics.rs"),
    ]
    .concat()
}

fn web_styles_source() -> String {
    [
        include_str!("../../styles/remote-upload.css"),
        include_str!("../../styles/home.css"),
        include_str!("../../styles/object-browser.css"),
        include_str!("../../styles/activity.css"),
        include_str!("../../styles/local-access.css"),
        include_str!("../../styles/endpoints.css"),
        include_str!("../../styles/auth.css"),
        include_str!("../../styles/enclosures.css"),
        include_str!("../../styles.css"),
    ]
    .concat()
}

fn object_browser_styles_source() -> &'static str {
    include_str!("../../styles/object-browser.css")
}

fn activity_styles_source() -> &'static str {
    include_str!("../../styles/activity.css")
}

#[test]
fn shared_web_primitives_cover_panes_tables_status_and_responsive_surfaces() {
    let source = workspace_component_source();
    let widgets = include_str!("../components/widgets.rs");
    let css = web_styles_source();

    for selector in [
        ".dos-task-pane",
        ".dos-product-footer",
        ".dos-table-wrap",
        ".dos-table",
        ".dos-status-pill",
        ".dos-status-badge",
        ".dos-capacity-bar",
        ".dos-segmented-control",
        ".dos-icon-button",
        ".dos-risky-confirmation",
        ".dos-inspector-drawer",
        "@media (max-width: 640px)",
    ] {
        assert!(
            css.contains(selector),
            "missing shared Web selector {selector}"
        );
    }
    let auth_css = include_str!("../../styles/auth.css");
    for selector in [
        ".dos-auth-shell",
        ".dos-auth-form",
        ".dos-auth-submit",
        ".dos-auth-error",
    ] {
        assert!(
            auth_css.contains(selector),
            "missing auth CSS selector {selector}"
        );
    }
    assert!(source.contains("dos-table-wrap"));
    assert!(source.contains("dos-table dos-object-browser-table"));
    for class in [
        "dos-task-pane",
        "dos-dense-table",
        "dos-status-badge",
        "dos-capacity-bar",
        "dos-segmented-control",
        "dos-icon-button",
        "dos-risky-confirmation",
        "dos-inspector-drawer",
    ] {
        assert!(
            widgets.contains(class),
            "missing shared widget class {class}"
        );
    }
    assert!(widgets.contains("role=\"dialog\""));
    assert!(widgets.contains("aria-modal=\"true\""));
    assert!(widgets.contains("aria-valuenow"));
    assert!(widgets.contains("aria-pressed"));
    assert!(widgets.contains("onsubmit={Callback::from"));

    let index = include_str!("../../index.html");
    let base_link = index.find("styles.css").expect("base sheet registered");
    for feature in [
        "styles/remote-upload.css",
        "styles/home.css",
        "styles/object-browser.css",
        "styles/activity.css",
    ] {
        let feature_link = index.find(feature).expect("feature sheet registered");
        assert!(
            feature_link < base_link,
            "{feature} must precede shared styles"
        );
        assert_eq!(
            index.matches(feature).count(),
            1,
            "{feature} registered once"
        );
    }
}

use super::{
    activity_category_summaries, activity_queue_summary, activity_task_progress_label,
    activity_workspace_api_path, admin_job_percent, admin_job_progress_text,
    admin_job_state_is_terminal, bioinformatics_derivation_source_summaries,
    bioinformatics_readiness_summaries, bioinformatics_summary_cards,
    bioinformatics_workspace_api_path, enclosure_card_summaries, enclosure_prepare_candidate,
    enclosure_prepare_confirmed, enclosure_retry_clears_job_state, enclosure_ssd_root,
    enclosures_workspace_api_path, endpoints_workspace_api_path,
    home_dashboard_api_path_with_window, home_dashboard_attention, home_dashboard_metrics,
    home_throughput_chart_max_tib, home_throughput_chart_points, home_throughput_chart_polyline,
    home_throughput_chart_segments, home_throughput_source_class, home_throughput_source_label,
    home_workspace_api_path, object_browser_download_disabled_reason,
    object_browser_file_download_available, object_browser_file_summaries,
    object_browser_folder_download_available, object_browser_folder_summaries,
    object_browser_initial_endpoint, object_browser_placement_summary,
    object_browser_placement_summary_state, object_store_bucket_default,
    object_store_card_summaries, object_store_configure_review_from_values,
    object_store_create_confirmation_matches, object_store_create_review_from_values,
    object_store_creation_fields_ready, objectstores_workspace_api_path,
    page_load_state_from_result_with_stale, primary_navigation_for_host,
    remote_upload_folder_count, remote_upload_workspace_api_path,
    subobject_registry_preview_from_values, users_groups_summary_cards,
    users_groups_workspace_api_path, ApiLoadState, EnclosureWizardState, RemoteUploadSelectedFile,
    RemoteUploadSelectionSummary, ThroughputDayResponse, WorkspacePage, ACTIVITY_WORKSPACE_ROUTE,
    BIOINFORMATICS_WORKSPACE_ROUTE, ENCLOSURES_WORKSPACE_ROUTE, ENDPOINTS_WORKSPACE_ROUTE,
    HOME_WORKSPACE_ROUTE, OBJECTSTORES_WORKSPACE_ROUTE, PRIMARY_NAVIGATION,
    REMOTE_UPLOAD_WORKSPACE_ROUTE,
};

#[test]
fn home_refresh_preserves_a_successful_snapshot_when_transport_fails() {
    let previous = ApiLoadState::success("last-known-good".to_string());
    let failed = Err(crate::api::ApiError {
        message: "daemon unavailable".to_string(),
        status: None,
    });
    let stale = page_load_state_from_result_with_stale(&previous, failed, |_| None);
    match stale {
        ApiLoadState::StaleData { value, message } => {
            assert_eq!(value, "last-known-good");
            assert!(message.contains("last successful snapshot"));
            assert!(message.contains("daemon unavailable"));
        }
        other => panic!("expected stale snapshot, got {}", other.state_name()),
    }

    let cold = ApiLoadState::<String>::Loading;
    let failed = Err(crate::api::ApiError {
        message: "daemon unavailable".to_string(),
        status: None,
    });
    let degraded = page_load_state_from_result_with_stale(&cold, failed, |_| None);
    assert_eq!(degraded.state_name(), "transport-error");
}
use super::{
    local_group_create_fields_ready, local_group_display_name,
    users_groups_view_with_group_assignment, users_groups_view_with_writer_group,
};
use crate::api::{
    ActivityCategoryResponse, ActivityTaskProgressResponse, ActivityTaskResponse,
    ActivityWorkspaceResponse, AdminJobCancelResponse, AdminJobProgress, AdminJobStatusResponse,
    AdminJobSummary, BioinformaticsContextCardResponse, BioinformaticsDerivationSourceResponse,
    BioinformaticsReadinessCardResponse, BioinformaticsWorkspaceResponse, CapacitySummaryResponse,
    DasEnclosureCardResponse, DestageQueueSummaryResponse, DriveCountSummaryResponse,
    EnclosureConnectionResponse, EnclosurePrepareAcceptedResponse, EnclosurePrepareHddDevice,
    EnclosurePrepareResponse, EnclosuresPageResponse, HomeDashboardResponse,
    IngestQueueSummaryResponse, LocalGroupMembershipResponse, LocalGroupOperationResponse,
    LocalUserAuthorityResponse, ObjectBrowserFileNodeResponse, ObjectBrowserFolderNodeResponse,
    ObjectBrowserPlacementResponse, ObjectStoresPageResponse, StandaloneUserAccountResponse,
    StorageGroupResponse, UsersGroupsCapabilitiesResponse, UsersGroupsWorkspaceResponse,
};
use crate::mount::FrontendHost;
use crate::stores::STORES_WORKSPACE_ROUTE;
use crate::users_groups::USERS_GROUPS_WORKSPACE_ROUTE;

#[test]
fn approved_mnemosyne_partial_asset_is_registered_with_provenance() {
    use sha2::{Digest, Sha256};

    const EXPECTED_SHA256: &str =
        "14f0b0d208b9c3358914aaba165b803e8d62bb4888ed5066b94d600e4acdcb90";
    let bytes = include_bytes!("../../assets/mnemosyne-biosciences-partial.png");
    let digest = Sha256::digest(bytes);
    assert_eq!(format!("{digest:x}"), EXPECTED_SHA256);

    let index = include_str!("../../index.html");
    assert_eq!(
        index
            .matches("assets/mnemosyne-biosciences-partial.png")
            .count(),
        1
    );
}

#[test]
fn approved_mnemosyne_brand_assets_are_registered_with_pinned_provenance() {
    use sha2::{Digest, Sha256};

    let index = include_str!("../../index.html");
    let assets = [
        (
            "assets/mnemosyne-biosciences-logo-icon-black.png",
            include_bytes!("../../assets/mnemosyne-biosciences-logo-icon-black.png") as &[u8],
            "53b533846d53dd6c9533574eb1c307964f99f7080362868e92285fec7853cb3c",
        ),
        (
            "assets/mnemosyne-biosciences-logo-master-mono.png",
            include_bytes!("../../assets/mnemosyne-biosciences-logo-master-mono.png") as &[u8],
            "d529ce803cd14119745d91b1c2ae23512b71f6c5c094e44226b3cd9586e34b55",
        ),
        (
            "assets/mnemosyne-biosciences-partial.png",
            include_bytes!("../../assets/mnemosyne-biosciences-partial.png") as &[u8],
            "14f0b0d208b9c3358914aaba165b803e8d62bb4888ed5066b94d600e4acdcb90",
        ),
    ];

    for (path, bytes, expected_sha256) in assets {
        let digest = Sha256::digest(bytes);
        assert_eq!(
            format!("{digest:x}"),
            expected_sha256,
            "asset provenance: {path}"
        );
        assert_eq!(index.matches(path).count(), 1, "Trunk registration: {path}");
        assert!(!bytes.is_empty(), "asset bytes: {path}");
    }
}

#[test]
fn primary_navigation_uses_redesign_labels() {
    let labels: Vec<_> = PRIMARY_NAVIGATION.iter().map(|page| page.label()).collect();

    assert_eq!(
        labels,
        vec![
            "Home",
            "Enclosures",
            "ObjectStores",
            "Endpoints",
            "Activity",
            "Local Access",
            "Bioinformatics"
        ]
    );
}

#[test]
fn primary_navigation_is_host_mode_aware_for_users_groups() {
    let standalone_labels: Vec<_> = primary_navigation_for_host(FrontendHost::Standalone)
        .iter()
        .map(|page| page.label())
        .collect();
    let synoptikon_labels: Vec<_> = primary_navigation_for_host(FrontendHost::Synoptikon)
        .iter()
        .map(|page| page.label())
        .collect();

    assert!(standalone_labels.contains(&"Activity"));
    assert!(synoptikon_labels.contains(&"Activity"));
    assert!(!standalone_labels.contains(&"Remote Upload"));
    assert!(!synoptikon_labels.contains(&"Remote Upload"));
    assert!(standalone_labels.contains(&"Endpoints"));
    assert!(!synoptikon_labels.contains(&"Endpoints"));
    assert!(standalone_labels.contains(&"Local Access"));
    assert!(!synoptikon_labels.contains(&"Local Access"));
}

#[test]
fn workspace_pages_build_expected_api_paths() {
    let base = "/products/dasobjectstore/api/v1/";

    assert_eq!(
        WorkspacePage::Home.api_path(base),
        "/products/dasobjectstore/api/v1/dashboard/home"
    );
    assert_eq!(
        WorkspacePage::Enclosures.api_path(base),
        "/products/dasobjectstore/api/v1/dashboard/enclosures"
    );
    assert_eq!(
        WorkspacePage::ObjectStores.api_path(base),
        "/products/dasobjectstore/api/v1/dashboard/object-stores"
    );
    assert_eq!(
        WorkspacePage::Activity.api_path(base),
        "/products/dasobjectstore/api/v1/workspaces/activity"
    );
    assert_eq!(
        WorkspacePage::RemoteUpload.api_path(base),
        "/products/dasobjectstore/api/v1/workspaces/remote-upload"
    );
    assert_eq!(
        WorkspacePage::Endpoints.api_path(base),
        "/products/dasobjectstore/api/v1/workspaces/endpoints"
    );
    assert_eq!(
        WorkspacePage::UsersGroups.api_path(base),
        "/products/dasobjectstore/api/v1/workspaces/users-groups"
    );
    assert_eq!(
        WorkspacePage::Bioinformatics.api_path(base),
        "/products/dasobjectstore/api/v1/workspaces/bioinformatics"
    );
}

#[test]
fn frontend_page_routes_use_dashboard_contracts() {
    assert_eq!(HOME_WORKSPACE_ROUTE, "dashboard/home");
    assert_eq!(ENCLOSURES_WORKSPACE_ROUTE, "dashboard/enclosures");
    assert_eq!(OBJECTSTORES_WORKSPACE_ROUTE, "dashboard/object-stores");
    assert_eq!(ACTIVITY_WORKSPACE_ROUTE, "workspaces/activity");
    assert_eq!(REMOTE_UPLOAD_WORKSPACE_ROUTE, "workspaces/remote-upload");
    assert_eq!(ENDPOINTS_WORKSPACE_ROUTE, "workspaces/endpoints");
    assert_eq!(home_workspace_api_path("/api/"), "/api/dashboard/home");
    assert_eq!(
        home_dashboard_api_path_with_window("/api/", "ten_days"),
        "/api/dashboard/home?telemetry_window=ten_days"
    );
    assert_eq!(
        enclosures_workspace_api_path("/api/"),
        "/api/dashboard/enclosures"
    );
    assert_eq!(
        objectstores_workspace_api_path("/api/"),
        "/api/dashboard/object-stores"
    );
    assert_eq!(
        activity_workspace_api_path("/api/"),
        "/api/workspaces/activity"
    );
    assert_eq!(
        remote_upload_workspace_api_path("/api/", "store&x y"),
        "/api/workspaces/remote-upload?store_id=store%26x%20y"
    );
    assert_eq!(
        endpoints_workspace_api_path("/api/"),
        "/api/workspaces/endpoints"
    );
    assert_eq!(
        users_groups_workspace_api_path("/api/"),
        "/api/workspaces/users-groups"
    );
}

#[test]
fn primary_navigation_promotes_users_groups_without_legacy_stores_holder() {
    let base = "/products/dasobjectstore/api/v1/";
    let primary_paths: Vec<_> = PRIMARY_NAVIGATION
        .iter()
        .map(|page| page.api_path(base))
        .collect();

    assert!(!primary_paths
        .iter()
        .any(|path| path.ends_with(STORES_WORKSPACE_ROUTE)));
    assert!(primary_paths
        .iter()
        .any(|path| path.ends_with(USERS_GROUPS_WORKSPACE_ROUTE)));
    assert!(primary_paths
        .iter()
        .any(|path| path.ends_with(ACTIVITY_WORKSPACE_ROUTE)));
    assert!(primary_paths
        .iter()
        .any(|path| path.ends_with(ENDPOINTS_WORKSPACE_ROUTE)));
    assert!(primary_paths
        .iter()
        .any(|path| path.ends_with(OBJECTSTORES_WORKSPACE_ROUTE)));
}

#[test]
fn activity_category_summaries_cover_daemon_job_states() {
    let view = ActivityWorkspaceResponse {
        ingest: Some(IngestQueueSummaryResponse {
            pressure: "normal".to_string(),
            queued_jobs: 2,
            active_jobs: 1,
            failed_jobs: 0,
            warnings: Vec::new(),
        }),
        destage: Some(DestageQueueSummaryResponse {
            pending_objects: 3,
            copying_objects: 1,
            verified_objects: 8,
            warnings: Vec::new(),
        }),
        categories: vec![
            ActivityCategoryResponse {
                kind: "system_administration".to_string(),
                label: "Administrator jobs".to_string(),
                description: "Privileged work".to_string(),
            },
            ActivityCategoryResponse {
                kind: "ingest".to_string(),
                label: "Ingest".to_string(),
                description: "Uploads".to_string(),
            },
            ActivityCategoryResponse {
                kind: "repair".to_string(),
                label: "Repair".to_string(),
                description: "Repair work".to_string(),
            },
        ],
        tasks: vec![
            ActivityTaskResponse {
                task_id: "job-admin".to_string(),
                kind: "system_administration".to_string(),
                state: "running".to_string(),
                label: "Create local writer group".to_string(),
                progress: None,
                updated_at_utc: "2026-07-09T00:00:00Z".to_string(),
                warnings: Vec::new(),
            },
            ActivityTaskResponse {
                task_id: "job-ingest".to_string(),
                kind: "ingest".to_string(),
                state: "queued".to_string(),
                label: "Ingest zymo".to_string(),
                progress: None,
                updated_at_utc: "2026-07-09T00:01:00Z".to_string(),
                warnings: Vec::new(),
            },
            ActivityTaskResponse {
                task_id: "job-repair".to_string(),
                kind: "repair".to_string(),
                state: "failed".to_string(),
                label: "Restore copy".to_string(),
                progress: None,
                updated_at_utc: "2026-07-09T00:02:00Z".to_string(),
                warnings: Vec::new(),
            },
            ActivityTaskResponse {
                task_id: "job-repair-cancelled".to_string(),
                kind: "repair".to_string(),
                state: "cancelled".to_string(),
                label: "Cancelled replacement".to_string(),
                progress: None,
                updated_at_utc: "2026-07-09T00:03:00Z".to_string(),
                warnings: Vec::new(),
            },
        ],
        warnings: Vec::new(),
    };

    let summaries = activity_category_summaries(&view);
    let queues = activity_queue_summary(&view);

    assert_eq!(summaries[0].active_count, 1);
    assert_eq!(summaries[0].state, "running");
    assert_eq!(summaries[1].waiting_count, 1);
    assert_eq!(summaries[1].state, "waiting");
    assert_eq!(summaries[2].failed_count, 1);
    assert_eq!(summaries[2].complete_count, 1);
    assert_eq!(summaries[2].state, "critical");
    assert_eq!(queues[0].value, "1 active");
    assert_eq!(queues[1].value, "1 copying");
}

#[test]
fn activity_task_progress_label_prefers_stage_percent_and_bytes() {
    let label = activity_task_progress_label(&ActivityTaskProgressResponse {
        stage: "remote_s3_transfer_running".to_string(),
        work_bytes_done: 512,
        work_bytes_total: 1024,
        work_units_done: 3,
        work_units_total: 9,
        percent_complete: Some(50),
        message: Some("remote upload copied 512 bytes".to_string()),
    });

    assert_eq!(label, "remote_s3_transfer_running · 50% · 512 / 1024 bytes");
}

#[test]
fn users_groups_summary_surfaces_authority_and_writer_policy() {
    let cards = users_groups_summary_cards(&users_groups_workspace_fixture());
    let values: Vec<_> = cards
        .iter()
        .map(|card| (card.label.as_str(), card.value.as_str()))
        .collect();

    assert!(values.contains(&("Authority adapter", "standalone")));
    assert!(values.contains(&("Local actor", "operator")));
    assert!(values.contains(&("Local users", "1")));
    assert!(values.contains(&("Access groups", "1")));
    assert!(values.contains(&("Access actions", "2")));
}

#[test]
fn users_groups_forms_gate_required_fields_before_acknowledgement() {
    assert!(local_group_create_fields_ready("mnemosyne-writers"));
    assert!(!local_group_create_fields_ready(" "));
}

#[test]
fn users_groups_live_create_updates_writer_policy_view() {
    let view =
        users_groups_view_with_writer_group(users_groups_workspace_fixture(), "mnemosyne_writers");

    assert_eq!(
        local_group_display_name("mnemosyne_writers"),
        "Mnemosyne Writers"
    );
    assert!(view
        .writer_groups
        .iter()
        .any(|group| group.group_name == "mnemosyne_writers"
            && group.display_name == "Mnemosyne Writers"
            && !group.current_user_member));
    assert_eq!(
        view.selected_group_name.as_deref(),
        Some("mnemosyne_writers")
    );
}

#[test]
fn users_groups_live_assignment_updates_current_user_membership_view() {
    let view =
        users_groups_view_with_writer_group(users_groups_workspace_fixture(), "mnemosyne_writers");
    let view = users_groups_view_with_group_assignment(view, "operator", "mnemosyne_writers");

    assert!(view
        .current_user
        .as_ref()
        .expect("fixture user")
        .groups
        .iter()
        .any(|group| group == "mnemosyne_writers"));
    assert!(view
        .writer_groups
        .iter()
        .any(|group| group.group_name == "mnemosyne_writers" && group.current_user_member));
    assert_eq!(view.selected_username.as_deref(), Some("operator"));
    assert_eq!(
        view.selected_group_name.as_deref(),
        Some("mnemosyne_writers")
    );
}

#[test]
fn bioinformatics_route_is_stable() {
    assert_eq!(BIOINFORMATICS_WORKSPACE_ROUTE, "workspaces/bioinformatics");
    assert_eq!(
        bioinformatics_workspace_api_path("/api/"),
        "/api/workspaces/bioinformatics"
    );
}

#[test]
fn bioinformatics_readiness_cards_surface_workflow_handoff() {
    let view = BioinformaticsWorkspaceResponse {
        schema_version: "dasobjectstore.product_workspaces.v1".to_string(),
        available: true,
        supported_object_types: vec![
            "BAM".to_string(),
            "CRAM".to_string(),
            "POD5".to_string(),
            "FASTQ/FASTQ.GZ".to_string(),
            "FASTA".to_string(),
            "VCF/BCF".to_string(),
            "GFF/GTF".to_string(),
            "ENA/SRA".to_string(),
        ],
        readiness_cards: vec![
            BioinformaticsReadinessCardResponse {
                object_type: "pod5".to_string(),
                label: "POD5".to_string(),
                category: "Nanopore signal".to_string(),
                state: "workflow_ready".to_string(),
                primary_workflow: "Basecalling and signal provenance.".to_string(),
                handoff: "Basecalling readiness".to_string(),
                required_metadata: vec![
                    "flowcell/run identity".to_string(),
                    "sequencing kit".to_string(),
                ],
            },
            BioinformaticsReadinessCardResponse {
                object_type: "cram".to_string(),
                label: "CRAM".to_string(),
                category: "Compressed alignment".to_string(),
                state: "metadata_required".to_string(),
                primary_workflow: "Reference-backed analysis.".to_string(),
                handoff: "Genome analysis with reference binding".to_string(),
                required_metadata: vec!["reference genome".to_string()],
            },
        ],
        derivation_sources: vec![
            BioinformaticsDerivationSourceResponse {
                source_kind: "object_store_metadata".to_string(),
                source_id: "contract-object-store-object-type".to_string(),
                display_name: "ObjectStore object-type assignment".to_string(),
                object_type: "pod5".to_string(),
                parent_id: None,
                endpoint_export_mode: Some("s3_bucket".to_string()),
                mneion_binding_state: "binding_required".to_string(),
                governance_domain: None,
                workflow_roles: vec![
                    "sequencing_run_provenance".to_string(),
                    "basecalling_handoff".to_string(),
                ],
                evidence: vec!["ObjectStore object_type assignment".to_string()],
            },
            BioinformaticsDerivationSourceResponse {
                source_kind: "subobject_metadata".to_string(),
                source_id: "contract-subobject-lineage".to_string(),
                display_name: "SubObject lineage and object-type policy".to_string(),
                object_type: "fastq".to_string(),
                parent_id: Some("contract-object-store-object-type".to_string()),
                endpoint_export_mode: Some("dedicated_prefix".to_string()),
                mneion_binding_state: "binding_required".to_string(),
                governance_domain: None,
                workflow_roles: vec!["object_lineage".to_string()],
                evidence: vec!["SubObject parent relationship".to_string()],
            },
            BioinformaticsDerivationSourceResponse {
                source_kind: "mneion_binding".to_string(),
                source_id: "contract-mneion-governance-binding".to_string(),
                display_name: "Mneion governance-domain binding".to_string(),
                object_type: "mixed".to_string(),
                parent_id: None,
                endpoint_export_mode: None,
                mneion_binding_state: "binding_required".to_string(),
                governance_domain: Some("unassigned".to_string()),
                workflow_roles: vec!["governance_binding".to_string()],
                evidence: vec!["Mneion storage definition".to_string()],
            },
        ],
        sequencing_runs: vec![BioinformaticsContextCardResponse {
            label: "Sequencing run provenance".to_string(),
            state: "metadata_required".to_string(),
            summary: "Run metadata required.".to_string(),
            detail: "Bind flowcell, kit, and sample state.".to_string(),
            evidence: vec!["POD5 basecalling readiness".to_string()],
        }],
        object_lineage: vec![BioinformaticsContextCardResponse {
            label: "Object lineage".to_string(),
            state: "planned".to_string(),
            summary: "Lineage planned.".to_string(),
            detail: "Connect signal, reads, alignment, and variants.".to_string(),
            evidence: vec!["raw signal to reads".to_string()],
        }],
        workflow_handoffs: vec![BioinformaticsContextCardResponse {
            label: "Basecalling handoff".to_string(),
            state: "workflow_ready".to_string(),
            summary: "Basecalling ready.".to_string(),
            detail: "POD5 handoff state is available.".to_string(),
            evidence: vec!["POD5 readiness cards".to_string()],
        }],
        governance_bindings: vec![BioinformaticsContextCardResponse {
            label: "Mnemosyne governance binding".to_string(),
            state: "binding_required".to_string(),
            summary: "Binding required.".to_string(),
            detail: "Project and governance-domain binding is required.".to_string(),
            evidence: vec!["endpoint inventory bindings".to_string()],
        }],
        message: "Readiness cards available.".to_string(),
    };

    let cards = bioinformatics_readiness_summaries(&view);
    let derivation_sources = bioinformatics_derivation_source_summaries(&view);
    let context_cards = super::bioinformatics_context_summaries(&view);
    let metrics = bioinformatics_summary_cards(&view);

    assert_eq!(cards.len(), 2);
    assert_eq!(cards[0].label, "POD5");
    assert_eq!(cards[0].state_label, "Workflow ready");
    assert_eq!(cards[0].handoff, "Basecalling readiness");
    assert_eq!(cards[0].metadata, "flowcell/run identity; sequencing kit");
    assert_eq!(cards[1].state_label, "Metadata needed");
    assert_eq!(metrics[0].1, "2");
    assert_eq!(metrics[1].1, "1");
    assert_eq!(metrics[2].1, "1");
    assert_eq!(metrics[3].1, "4");
    assert_eq!(metrics[4].1, "3");
    assert_eq!(derivation_sources[0].source_kind, "object_store_metadata");
    assert_eq!(derivation_sources[0].parent, "top-level source");
    assert_eq!(
        derivation_sources[1].parent,
        "contract-object-store-object-type"
    );
    assert_eq!(
        derivation_sources[2].binding,
        "binding_required · unassigned"
    );
    assert_eq!(context_cards[0].section, "Sequencing Runs");
    assert_eq!(context_cards[1].state_label, "Planned");
    assert_eq!(context_cards[3].state_label, "Binding needed");
}

#[test]
fn bioinformatics_readiness_falls_back_to_supported_types() {
    let view = BioinformaticsWorkspaceResponse {
        schema_version: "dasobjectstore.product_workspaces.v1".to_string(),
        available: false,
        supported_object_types: vec!["FASTQ/FASTQ.GZ".to_string(), "ENA/SRA".to_string()],
        readiness_cards: Vec::new(),
        derivation_sources: Vec::new(),
        sequencing_runs: Vec::new(),
        object_lineage: Vec::new(),
        workflow_handoffs: Vec::new(),
        governance_bindings: Vec::new(),
        message: "Older payload.".to_string(),
    };

    let cards = bioinformatics_readiness_summaries(&view);

    assert_eq!(cards.len(), 2);
    assert_eq!(cards[0].label, "FASTQ/FASTQ.GZ");
    assert_eq!(cards[0].state_label, "Reserved");
    assert_eq!(cards[0].handoff, "Pending workflow contract");
}

#[test]
fn admin_job_terminal_states_are_stable_for_wizard_actions() {
    assert!(admin_job_state_is_terminal("complete"));
    assert!(admin_job_state_is_terminal("failed"));
    assert!(admin_job_state_is_terminal("cancelled"));
    assert!(!admin_job_state_is_terminal("queued"));
    assert!(!admin_job_state_is_terminal("running"));
    assert!(!admin_job_state_is_terminal("waiting"));
}

#[test]
fn admin_job_percent_prefers_daemon_percent_then_unit_progress() {
    let with_percent = AdminJobSummary {
        percent_complete: Some(42),
        ..admin_job_summary_fixture()
    };
    assert_eq!(admin_job_percent(&with_percent), Some(42));

    let by_units = AdminJobSummary {
        percent_complete: None,
        progress: AdminJobProgress {
            stage: "formatting".to_string(),
            work_units_done: 3,
            work_units_total: 4,
            ..AdminJobProgress::default()
        },
        ..admin_job_summary_fixture()
    };
    assert_eq!(admin_job_percent(&by_units), Some(75));
    assert_eq!(admin_job_progress_text(&by_units), "3 / 4 step(s)");
}

#[test]
fn admin_job_progress_text_prefers_byte_progress_when_available() {
    let job = AdminJobSummary {
        progress: AdminJobProgress {
            stage: "copying".to_string(),
            work_bytes_done: 512,
            work_bytes_total: 1024,
            work_units_done: 1,
            work_units_total: 4,
            message: None,
        },
        ..admin_job_summary_fixture()
    };

    assert_eq!(admin_job_progress_text(&job), "512 / 1024 byte(s)");
}

#[test]
fn enclosure_prepare_confirmation_requires_existing_data_acknowledgement() {
    assert!(enclosure_prepare_confirmed(
        true,
        true,
        " confirm prepare das "
    ));
    assert!(!enclosure_prepare_confirmed(
        true,
        false,
        "confirm prepare das"
    ));
    assert!(!enclosure_prepare_confirmed(
        false,
        true,
        "confirm prepare das"
    ));
}

#[test]
fn enclosure_retry_preserves_selection_but_clears_job_and_cancel_state() {
    let mut state = EnclosureWizardState {
        open: true,
        selected_ssd: "/dev/disk/by-id/nvme-ssd".to_string(),
        selected_hdds: vec!["/dev/disk/by-id/usb-qnap-1057".to_string()],
        mount_root: "/srv/dasobjectstore".to_string(),
        filesystem: "ext4".to_string(),
        owner: "stephen".to_string(),
        allow_format: true,
        existing_data_acknowledged: true,
        confirmation_phrase: "confirm prepare das".to_string(),
        submitting: false,
        job: Some(enclosure_prepare_response_fixture()),
        job_status: Some(AdminJobStatusResponse {
            job: AdminJobSummary {
                state: "failed".to_string(),
                failure_message: Some("existing data preflight failed".to_string()),
                ..admin_job_summary_fixture()
            },
        }),
        job_polling: true,
        job_status_error: Some("stale failure".to_string()),
        cancelling: true,
        cancellation: Some(AdminJobCancelResponse {
            job_id: "enclosure-prepare-1".to_string(),
            accepted: true,
            state: "cancelled".to_string(),
        }),
        cancel_error: Some("cancel failed".to_string()),
        error: Some("daemon failed".to_string()),
    };

    enclosure_retry_clears_job_state(&mut state);

    assert!(state.open);
    assert_eq!(state.selected_ssd, "/dev/disk/by-id/nvme-ssd");
    assert_eq!(state.selected_hdds.len(), 1);
    assert!(state.allow_format);
    assert!(state.existing_data_acknowledged);
    assert_eq!(state.confirmation_phrase, "confirm prepare das");
    assert!(state.job.is_none());
    assert!(state.job_status.is_none());
    assert!(!state.job_polling);
    assert!(state.cancellation.is_none());
    assert!(state.cancel_error.is_none());
    assert!(state.error.is_none());
}

#[test]
fn object_store_bucket_default_normalizes_store_name_for_s3() {
    assert_eq!(
        object_store_bucket_default("Zymo Fecal 2025.05/raw"),
        "zymo-fecal-2025-05-raw"
    );
    assert_eq!(
        object_store_bucket_default("...Generated_Data..."),
        "generated-data"
    );
}

#[test]
fn enclosure_ssd_root_derives_from_hdd_mount() {
    let enclosure = DasEnclosureCardResponse {
        enclosure_id: "qnap-tl-d800c-managed".to_string(),
        display_name: "QNAP TL-D800C".to_string(),
        mount_path: "/srv/dasobjectstore/hdd".to_string(),
        connection: EnclosureConnectionResponse {
            bus: "usb".to_string(),
            protocol: "uas/filesystem".to_string(),
            link_speed: "host reported".to_string(),
        },
        health: "healthy".to_string(),
        drive_count: DriveCountSummaryResponse {
            total: 8,
            mounted: 8,
            healthy: 8,
            watch: 0,
            suspect: 0,
            failed: 0,
        },
        capacity: CapacitySummaryResponse {
            total_tib: "100.0".to_string(),
            used_tib: "12.5".to_string(),
            free_tib: "87.5".to_string(),
            used_percent_basis_points: 1250,
        },
        last_seen_at_utc: "2026-07-08T08:30:00Z".to_string(),
        warnings: Vec::new(),
    };

    assert_eq!(enclosure_ssd_root(&enclosure), "/srv/dasobjectstore/ssd");
}

#[test]
fn object_store_creation_requires_identity_group_and_enclosure() {
    assert!(object_store_creation_fields_ready(
        "generated-data",
        "mnemosyne",
        "qnap-tl-d800c-managed"
    ));
    assert!(!object_store_creation_fields_ready(
        "",
        "mnemosyne",
        "qnap-tl-d800c-managed"
    ));
    assert!(!object_store_creation_fields_ready(
        "generated-data",
        "",
        "qnap-tl-d800c-managed"
    ));
    assert!(!object_store_creation_fields_ready(
        "generated-data",
        "mnemosyne",
        ""
    ));
}

#[test]
fn object_store_create_review_captures_policy_controls() {
    let review = object_store_create_review_from_values(
        "generated-data",
        "pod5",
        2,
        "bioinformatics",
        "s3_bucket",
        false,
        "qnap-tl-d800c-managed",
    );

    assert_eq!(
            review,
            "generated-data · type pod5 · 2 copy/copies · writer group bioinformatics · enclosure qnap-tl-d800c-managed · export s3_bucket · private · writeable until locked"
        );
}

#[test]
fn object_store_configure_review_captures_policy_controls() {
    let review = object_store_configure_review_from_values(
        "generated-data",
        3,
        "bioinformatics",
        "backpressure_by_priority",
        "tombstone_then_gc",
        "s3",
        true,
        false,
    );

    assert_eq!(
            review,
            "generated-data · 3 copy/copies · writer group bioinformatics · capacity backpressure_by_priority · retention tombstone_then_gc · export s3 · public · read-only"
        );
}

#[test]
fn subobject_registry_preview_captures_parent_type_and_routing() {
    let review = subobject_registry_preview_from_values(
        "pod5-raw",
        "store",
        "generated-data",
        "",
        "override",
        "pod5",
        "dedicated_prefix",
    );

    assert_eq!(
            review,
            "pod5-raw under generated-data · prefix generated-data/pod5-raw · object type pod5 · S3 routing dedicated_prefix"
        );
}

#[test]
fn object_store_create_confirmation_requires_exact_phrase() {
    assert!(object_store_create_confirmation_matches(
        "confirm create objectstore"
    ));
    assert!(object_store_create_confirmation_matches(
        " confirm create objectstore "
    ));
    assert!(!object_store_create_confirmation_matches(
        "confirm create object store"
    ));
    assert!(!object_store_create_confirmation_matches(
        "CONFIRM CREATE OBJECTSTORE"
    ));
}

#[test]
fn shared_api_load_state_names_cover_page_contract() {
    let success = ApiLoadState::success("payload");
    let empty = ApiLoadState::<&str>::empty("empty");
    let permission_denied = ApiLoadState::<&str>::permission_denied("denied");
    let transport_error = ApiLoadState::<&str>::transport_error("offline");
    let stale = ApiLoadState::stale_data("payload", "stale");
    let states = [
        ApiLoadState::<&str>::Loading.state_name(),
        success.state_name(),
        empty.state_name(),
        permission_denied.state_name(),
        transport_error.state_name(),
        stale.state_name(),
    ];

    assert_eq!(
        states,
        [
            "loading",
            "success",
            "empty",
            "permission-denied",
            "transport-error",
            "stale-data",
        ]
    );
}

#[test]
fn authenticated_pages_do_not_expose_fixture_fallback_helpers() {
    let source = workspace_component_source();

    assert!(!source.contains(&format!("{}{}", "fallback_", "dashboard_metrics")));
    assert!(!source.contains(&format!("{}{}", "fallback_", "enclosures")));
    assert!(!source.contains(&format!("{}{}", "fallback_", "object_stores")));
}

fn admin_job_summary_fixture() -> AdminJobSummary {
    AdminJobSummary {
        job_id: "enclosure-prepare-1".to_string(),
        kind: "enclosure_preparation".to_string(),
        state: "running".to_string(),
        progress: AdminJobProgress::default(),
        percent_complete: None,
        submitted_at_utc: "2026-07-08T20:00:00Z".to_string(),
        updated_at_utc: "2026-07-08T20:00:01Z".to_string(),
        actor: Some("stephen".to_string()),
        failure_message: None,
    }
}

fn enclosure_prepare_response_fixture() -> EnclosurePrepareResponse {
    EnclosurePrepareResponse {
        accepted: EnclosurePrepareAcceptedResponse {
            job_id: "enclosure-prepare-1".to_string(),
            kind: "enclosure_preparation".to_string(),
            accepted_at_utc: "2026-07-08T20:00:00Z".to_string(),
            dry_run: false,
        },
        ssd_device: "/dev/disk/by-id/nvme-ssd".to_string(),
        hdd_devices: vec![EnclosurePrepareHddDevice {
            disk_id: "qnap-1057".to_string(),
            device_path: "/dev/disk/by-id/usb-qnap-1057".to_string(),
        }],
        mount_root: "/srv/dasobjectstore".to_string(),
        filesystem: "ext4".to_string(),
        owner: Some("stephen".to_string()),
        administrator_actor: Some("stephen".to_string()),
        client_request_id: Some("prepare-1".to_string()),
    }
}

#[test]
fn object_stores_live_payload_maps_to_card_summaries() {
    let payload = serde_json::json!({
        "schema_version": "dasobjectstore.web_redesign.v1",
        "generated_at_utc": "2026-07-08T08:00:00Z",
        "stores": [{
            "store_id": "zymo_fecal_2025.05",
            "display_name": "zymo_fecal_2025.05",
            "store_class": "generated_data",
            "object_type": "pod5",
            "health": "healthy",
            "required_copies": 2,
            "object_count": 42,
            "capacity": {
                "total_tib": "100.0",
                "used_tib": "12.5",
                "free_tib": "87.5",
                "used_percent_basis_points": 1250
            },
            "capacity_status": {
                "pressure": "warning",
                "logical_limit_bytes": 1000000,
                "used_bytes": 400000,
                "reserved_bytes": 100000,
                "logical_available_bytes": 500000,
                "backend_free_bytes": 2000000,
                "backend_available_bytes": 1900000,
                "ssd_available_bytes": 700000,
                "copy_count": 2,
                "requires_ssd_staging": true,
                "warning_threshold_basis_points": 7500,
                "critical_threshold_basis_points": 9000,
                "admission_block_reason": null
            },
            "placement_policy": "fractional_free_space",
            "endpoint_export_mode": "s3_bucket",
            "writer_group": "bioinformatics",
            "public": false,
            "writeable": true,
            "created_at_utc": "2026-07-08T08:00:00Z",
            "last_ingested_at_utc": "2026-07-08T08:30:00Z",
            "warnings": [{
                "code": "store_watch",
                "message": "Store warning."
            }]
        }],
        "selected_store_id": "zymo_fecal_2025.05",
        "create_object_store": {
            "enabled": false,
            "action_kind": "store_create",
            "label": "Create ObjectStore",
            "required_fields": [],
            "optional_fields": [],
            "defaults": {
                "store_class": "generated_data",
                "required_copies": 2,
                "endpoint_export_mode": "s3_bucket"
            },
            "store_class_options": [],
            "copy_count_options": [1, 2, 3],
            "confirmation_required": true,
            "blocked_reason": "admin required"
        },
        "warnings": []
    });
    let view = serde_json::from_value::<ObjectStoresPageResponse>(payload)
        .expect("object stores payload decodes");

    let summaries = object_store_card_summaries(&view);

    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0].id, "zymo_fecal_2025.05");
    assert_eq!(summaries[0].label, "generated_data");
    assert_eq!(summaries[0].object_type, "pod5");
    assert_eq!(summaries[0].access, "private / writeable");
    assert!(summaries[0].policy.contains("2 required copy/copies"));
    assert!(summaries[0].capacity.contains("12.5 TiB used"));
    assert!(summaries[0].capacity_status.contains("warning"));
    assert!(summaries[0].capacity_status.contains("logical used 400000"));
    assert_eq!(summaries[0].writer_group, "bioinformatics");
    assert_eq!(summaries[0].endpoint, "s3_bucket");
    assert!(summaries[0].upload_allowed);
    assert_eq!(summaries[0].warning_count, 1);
}

#[test]
fn object_browser_payload_maps_to_dense_view_summaries() {
    let view = serde_json::from_value::<ObjectStoresPageResponse>(serde_json::json!({
        "schema_version": "dasobjectstore.web_redesign.v1",
        "generated_at_utc": "2026-07-09T10:00:00Z",
        "stores": [{
            "store_id": "ENA",
            "display_name": "ENA",
            "store_class": "reproducible_cache",
            "object_type": "ena_sra",
            "health": "healthy",
            "required_copies": 1,
            "object_count": 2,
            "capacity": null,
            "placement_policy": "fractional_free_space",
            "endpoint_export_mode": "s3_bucket",
            "writer_group": "mnemosyne",
            "public": true,
            "writeable": true,
            "created_at_utc": null,
            "last_ingested_at_utc": null,
            "warnings": []
        }],
        "selected_store_id": "ENA",
        "create_object_store": {
            "enabled": false,
            "action_kind": "store_create",
            "label": "Create ObjectStore",
            "required_fields": [],
            "optional_fields": [],
            "defaults": {
                "store_class": "generated_data",
                "required_copies": 1,
                "endpoint_export_mode": "s3_bucket"
            },
            "store_class_options": [],
            "copy_count_options": [1],
            "confirmation_required": true,
            "blocked_reason": "admin required"
        },
        "warnings": []
    }))
    .expect("object store view decodes");
    let store_summaries = object_store_card_summaries(&view);
    assert_eq!(
        store_summaries[0].capacity_status,
        "live capacity status unavailable (daemon provider not connected)"
    );
    let folders = vec![ObjectBrowserFolderNodeResponse {
        name: "Xenognostikon".to_string(),
        prefix: "Xenognostikon".to_string(),
        object_count: Some(2),
        total_size_bytes: Some(2 * 1024 * 1024 * 1024),
        readiness: "available".to_string(),
    }];
    let files = vec![ObjectBrowserFileNodeResponse {
        object_id: "Xenognostikon/Vervet/sample.fastq.gz".to_string(),
        name: "sample.fastq.gz".to_string(),
        path: "Xenognostikon/Vervet/sample.fastq.gz".to_string(),
        object_type: "fastq".to_string(),
        size_bytes: 1536,
        modified_at_utc: Some("2026-07-09T10:00:00Z".to_string()),
        checksum: None,
        readiness: "ssd_only".to_string(),
        lifecycle_state: "ReceivedOnSsd".to_string(),
        copy_count: 1,
        placements: vec![ObjectBrowserPlacementResponse {
            disk_id: Some("qnap-1057".to_string()),
            disk_label: Some("QNAP bay 1".to_string()),
            location: "hdd_settled".to_string(),
            state: "verified".to_string(),
            size_bytes: 1536,
            checksum: None,
            verified_at_utc: Some("2026-07-09T10:01:00Z".to_string()),
        }],
    }];

    let folder_summaries = object_browser_folder_summaries(&folders);
    let file_summaries = object_browser_file_summaries(&files);

    assert_eq!(
        object_browser_initial_endpoint(&view).as_deref(),
        Some("ENA")
    );
    assert_eq!(folder_summaries[0].objects, "2 object(s)");
    assert_eq!(folder_summaries[0].size, "2.0 GiB");
    assert_eq!(folder_summaries[0].readiness, "Available");
    assert_eq!(file_summaries[0].object_type, "Fastq");
    assert_eq!(file_summaries[0].size, "1.5 KiB");
    assert_eq!(file_summaries[0].readiness, "Ssd Only");
    assert_eq!(file_summaries[0].lifecycle, "Received On Ssd");
    assert_eq!(file_summaries[0].copies, "1 copy/copies");
    assert_eq!(file_summaries[0].placement_summary, "1 HDD settled");
    assert_eq!(
        file_summaries[0].placements[0].disk_label.as_deref(),
        Some("QNAP bay 1")
    );
    assert!(object_browser_folder_download_available(
        &folder_summaries[0].readiness
    ));
    assert!(!object_browser_file_download_available(
        &file_summaries[0].readiness,
        &file_summaries[0].placements,
    ));
    assert!(object_browser_download_disabled_reason(
        &file_summaries[0].readiness,
        &file_summaries[0].placements,
    )
    .contains("verified settled HDD"));

    let mut available_file = file_summaries[0].clone();
    available_file.readiness = "Available".to_string();
    assert!(object_browser_file_download_available(
        &available_file.readiness,
        &available_file.placements,
    ));

    let multi_copy = vec![
        ObjectBrowserPlacementResponse {
            disk_id: Some("qnap-1057".to_string()),
            disk_label: Some("QNAP bay 1".to_string()),
            location: "hdd_settled".to_string(),
            state: "verified".to_string(),
            size_bytes: 1536,
            checksum: None,
            verified_at_utc: Some("2026-07-09T10:01:00Z".to_string()),
        },
        ObjectBrowserPlacementResponse {
            disk_id: Some("qnap-1058".to_string()),
            disk_label: Some("QNAP bay 2".to_string()),
            location: "hdd_settled".to_string(),
            state: "verified".to_string(),
            size_bytes: 1536,
            checksum: None,
            verified_at_utc: Some("2026-07-09T10:01:00Z".to_string()),
        },
        ObjectBrowserPlacementResponse {
            disk_id: Some("ssd-landing".to_string()),
            disk_label: Some("Landing SSD".to_string()),
            location: "ssd_landing".to_string(),
            state: "pending".to_string(),
            size_bytes: 1536,
            checksum: None,
            verified_at_utc: None,
        },
    ];
    assert_eq!(
        object_browser_placement_summary(&multi_copy),
        "1 SSD landing · 2 HDD settled · 2 verified HDD copies · 1 pending"
    );
    assert_eq!(
        object_browser_placement_summary_state(&multi_copy),
        "pending"
    );

    let degraded = vec![ObjectBrowserPlacementResponse {
        disk_id: Some("qnap-1059".to_string()),
        disk_label: None,
        location: "hdd_settled".to_string(),
        state: "missing".to_string(),
        size_bytes: 1536,
        checksum: None,
        verified_at_utc: None,
    }];
    assert_eq!(
        object_browser_placement_summary(&degraded),
        "1 HDD settled · 1 degraded/missing"
    );
    assert_eq!(
        object_browser_placement_summary_state(&degraded),
        "degraded"
    );
}

#[test]
fn object_browser_component_contract_covers_rows_downloads_and_empty_states() {
    let source = workspace_component_source();

    assert!(source.contains("dos-object-browser-table"));
    assert!(source.contains("<th>{ \"Name\" }</th>"));
    assert!(source.contains("<th>{ \"Placement\" }</th>"));
    assert!(source.contains("<th>{ \"Actions\" }</th>"));
    assert!(source.contains("dos-object-browser-folder"));
    assert!(source.contains("dos-object-browser-download"));
    assert!(source.contains("Download folder"));
    assert!(source.contains("Download\""));
    assert!(source.contains("disabled={!download_enabled}"));
    assert!(source.contains("render_object_browser_download_state"));
    assert!(source.contains("data-download-state=\"starting\""));
    assert!(source.contains("data-download-state=\"permission-denied\""));
    assert!(source.contains("render_object_browser_message(\"Empty\", message)"));
    assert!(
        source.contains("render_object_browser_message(\"Files\", \"No files in this folder.\")")
    );
}

#[test]
fn object_browser_component_contract_covers_placement_badges_and_no_overlap_css() {
    let source = workspace_component_source();
    let css = web_styles_source();

    assert!(source.contains("dos-object-browser-placement-stack"));
    assert!(source.contains("dos-object-browser-placement-summary"));
    assert!(source.contains("data-location={placement.location.clone()}"));
    assert!(source.contains("data-state={placement.state.clone()}"));
    assert!(source.contains("data-state={object_browser_state_key(&file.readiness)}"));
    assert!(source.contains("object_browser_placement_summary_state(placements)"));
    assert!(source.contains("object_browser_download_disabled_reason"));
    assert!(source.contains("object_browser_file_download_available"));

    assert!(css.contains(".dos-table-wrap {\n  max-width: 100%;\n  overflow-x: auto;"));
    assert!(css.contains(".dos-table {\n  width: 100%;\n  border-collapse: collapse;"));
    assert!(css.contains(".dos-object-browser-table {\n  min-width: 1040px;"));
    assert!(css.contains(".dos-object-browser-table td:first-child span"));
    assert!(css.contains("text-overflow: ellipsis;"));
    assert!(css.contains(".dos-object-browser-placements {\n  display: flex;\n  flex-wrap: wrap;"));
    assert!(css
        .contains(".dos-object-browser-placement {\n  display: inline-flex;\n  max-width: 220px;"));
    assert!(css.contains("@media (max-width: 980px)"));
    assert!(css.contains(".dos-object-browser-controls,\n  .dos-object-browser-folders {"));
    assert!(css.contains("grid-template-columns: repeat(2, minmax(0, 1fr));"));
    assert!(css.contains("@media (max-width: 640px)"));
    assert!(css.contains(".dos-object-browser-controls,\n  .dos-object-browser-folders {"));
    assert!(css.contains("grid-template-columns: 1fr;"));
}

#[test]
fn object_browser_css_is_feature_owned_and_registered_before_base_styles() {
    let base = include_str!("../../styles.css");
    let feature = object_browser_styles_source();
    let index = include_str!("../../index.html");

    assert!(!base.contains(".dos-object-browser"));
    for selector in [
        ".dos-object-browser-controls",
        ".dos-object-browser-folders",
        ".dos-object-browser-table",
        ".dos-object-browser-placement",
        ".dos-object-browser-download",
        "@media (max-width: 980px)",
        "@media (max-width: 640px)",
    ] {
        assert!(
            feature.contains(selector),
            "missing feature selector {selector}"
        );
    }
    let feature_link = index
        .find("styles/object-browser.css")
        .expect("object browser sheet registered");
    let base_link = index.find("styles.css").expect("base sheet registered");
    assert!(feature_link < base_link);
    assert_eq!(index.matches("styles/object-browser.css").count(), 1);
}

#[test]
fn enclosures_live_payload_maps_to_card_summaries() {
    let payload = serde_json::json!({
        "schema_version": "dasobjectstore.web_redesign.v1",
        "generated_at_utc": "2026-07-08T08:00:00Z",
        "enclosures": [{
            "enclosure_id": "qnap-tl-d800c-01",
            "display_name": "QNAP TL-D800C",
            "mount_path": "/srv/dasobjectstore/hdd",
            "connection": {
                "bus": "usb",
                "protocol": "uas",
                "link_speed": "10 Gb/s"
            },
            "health": "watch",
            "drive_count": {
                "total": 8,
                "mounted": 7,
                "healthy": 6,
                "watch": 1,
                "suspect": 0,
                "failed": 0
            },
            "capacity": {
                "total_tib": "100.0",
                "used_tib": "12.5",
                "free_tib": "87.5",
                "used_percent_basis_points": 1250
            },
            "last_seen_at_utc": "2026-07-08T08:00:00Z",
            "warnings": [{
                "code": "smart_watch",
                "message": "One member drive has a SMART warning."
            }]
        }],
        "selected_enclosure_id": "qnap-tl-d800c-01",
        "details": null,
        "warnings": []
    });
    let view = serde_json::from_value::<EnclosuresPageResponse>(payload)
        .expect("enclosures payload decodes");

    let summaries = enclosure_card_summaries(&view);

    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0].id, "qnap-tl-d800c-01");
    assert_eq!(summaries[0].name, "QNAP TL-D800C");
    assert!(summaries[0].label.contains("usb / uas / 10 Gb/s"));
    assert!(summaries[0].drives.contains("7 mounted of 8"));
    assert_eq!(summaries[0].capacity, "87.5 TiB free of 100.0 TiB");
    assert_eq!(summaries[0].warning_count, 1);
}

#[test]
fn enclosure_prepare_candidate_separates_ssd_and_hdd_devices() {
    let payload = serde_json::json!({
        "schema_version": "dasobjectstore.web_redesign.v1",
        "generated_at_utc": "2026-07-08T08:00:00Z",
        "add_enclosure": {
            "enabled": true,
            "action_kind": "enclosure_add",
            "label": "Add enclosure",
            "state": "ready",
            "administrator": true,
            "supported_enclosure_detected": true,
            "daemon_ready": true,
            "confirmation_required": true,
            "blocked_reason": null,
            "next_step": "Start supported DAS detection and preparation planning."
        },
        "enclosures": [{
            "enclosure_id": "qnap-tl-d800c-01",
            "display_name": "QNAP TL-D800C",
            "mount_path": "/srv/dasobjectstore",
            "connection": {"bus": "usb", "protocol": "uas", "link_speed": "10 Gb/s"},
            "health": "healthy",
            "drive_count": {"total": 3, "mounted": 3, "healthy": 3, "watch": 0, "suspect": 0, "failed": 0},
            "capacity": {"total_tib": "32.0", "used_tib": "0.0", "free_tib": "32.0", "used_percent_basis_points": 0},
            "last_seen_at_utc": "2026-07-08T08:00:00Z",
            "warnings": []
        }],
        "selected_enclosure_id": "qnap-tl-d800c-01",
        "details": {
            "enclosure_id": "qnap-tl-d800c-01",
            "vendor": "QNAP",
            "model": "TL-D800C",
            "serial": "TL-D800C-TEST",
            "firmware": null,
            "slots": [
                {
                    "slot_number": 0,
                    "drive_id": "nvme-landing",
                    "role": "ssd",
                    "device_path": "/dev/disk/by-id/nvme-landing",
                    "size_tib": "3.6",
                    "health": "healthy",
                    "mounted": true
                },
                {
                    "slot_number": 1,
                    "drive_id": "qnap-1057",
                    "role": "hdd",
                    "device_path": "/dev/disk/by-id/usb-qnap-1057",
                    "size_tib": "14.6",
                    "health": "healthy",
                    "mounted": true
                },
                {
                    "slot_number": 2,
                    "drive_id": "qnap-1058",
                    "role": "hdd",
                    "device_path": "/dev/disk/by-id/usb-qnap-1058",
                    "size_tib": "14.6",
                    "health": "healthy",
                    "mounted": true
                }
            ]
        },
        "warnings": []
    });
    let view = serde_json::from_value::<EnclosuresPageResponse>(payload)
        .expect("enclosures payload decodes");

    let candidate =
        enclosure_prepare_candidate(&view, "qnap-tl-d800c-01").expect("prepare candidate");

    assert!(candidate.ready());
    assert_eq!(candidate.ssd_devices.len(), 1);
    assert_eq!(candidate.hdd_devices.len(), 2);
    assert_eq!(
        candidate.ssd_devices[0].device_path,
        "/dev/disk/by-id/nvme-landing"
    );
    assert_eq!(candidate.hdd_devices[0].disk_id, "qnap-1057");
}

#[test]
fn home_dashboard_live_payload_maps_to_metrics_and_attention() {
    let payload = serde_json::json!({
        "schema_version": "dasobjectstore.web_redesign.v1",
        "generated_at_utc": "2026-07-08T08:00:00Z",
        "health": {
            "state": "watch",
            "label": "Watch",
            "warning_count": 1,
            "critical_count": 0,
            "action_count": 1,
            "last_checked_at_utc": null
        },
        "drives": {
            "total": 7,
            "mounted": 7,
            "healthy": 6,
            "watch": 1,
            "suspect": 0,
            "failed": 0
        },
        "capacity": {
            "total_tib": "100.0",
            "used_tib": "12.5",
            "free_tib": "87.5",
            "used_percent_basis_points": 1250
        },
        "mounted_enclosures": [],
        "telemetry_window": {
            "selected": "one_day",
            "selected_label": "1 day",
            "options": [
                { "value": "one_hour", "label": "1 hour", "selected": false },
                { "value": "one_day", "label": "1 day", "selected": true },
                { "value": "ten_days", "label": "10 days", "selected": false },
                { "value": "three_months", "label": "3 months", "selected": false }
            ]
        },
        "throughput_7d": {
            "window_days": 7,
            "read_tib": "1.0",
            "written_tib": "2.0",
            "ingest_tib": "2.5",
            "avg_read_mib_s": 120,
            "avg_write_mib_s": 240,
            "daily": [
                { "date": "2026-07-07", "read_tib": "0.1", "written_tib": "0.2", "ingest_tib": "0.2" },
                { "date": "2026-07-08", "read_tib": "0.2", "written_tib": "0.5", "ingest_tib": "0.5" },
                { "date": "2026-07-09", "read_tib": "0.4", "written_tib": "1.8", "ingest_tib": "1.8" }
            ]
        },
        "disk_io": {
            "available": true,
            "read_mib_s": 120,
            "write_mib_s": 240,
            "read_ops_s": 10,
            "write_ops_s": 20,
            "busiest_disk_id": "qnap-1057",
            "state": "nominal",
            "message": null
        },
        "cpu_usage": {
            "available": true,
            "usage_percent": 42,
            "load_average_1m": "0.84",
            "logical_core_count": 8,
            "state": "nominal",
            "message": null
        },
        "active_users": {
            "available": true,
            "active_sessions": 3,
            "distinct_logged_in_users": 2,
            "administrator_sessions": 1,
            "operator_sessions": 1,
            "remote_agent_sessions": 1,
            "state": "nominal",
            "message": null
        },
        "memory_stress": {
            "state": "elevated",
            "pressure_percent": 71,
            "swap_used_percent": 9,
            "page_cache_tib": "0.4",
            "warning": {
                "code": "memory_pressure_high",
                "message": "Memory pressure is elevated."
            }
        },
        "object_service": {
            "active": true,
            "remote_ready": true,
            "bind_address": "0.0.0.0",
            "port": 3900,
            "local_url": "http://127.0.0.1:3900",
            "remote_url": "http://192.168.1.192:3900",
            "service_state": "Up 1 minute",
            "message": null
        },
        "smart_warnings": {
            "warning_count": 1,
            "affected_drive_count": 1,
            "warnings": [{
                "drive_id": "qnap-1057",
                "severity": "warning",
                "attribute": "reallocated_sector_count",
                "message": "SMART attribute is above warning threshold."
            }]
        },
        "object_stores": [{
            "store_id": "zymo_fecal_2025.05",
            "display_name": "zymo_fecal_2025.05",
            "health": "healthy",
            "object_count": 42,
            "warnings": []
        }]
    });
    let view = serde_json::from_value::<HomeDashboardResponse>(payload).expect("dashboard decodes");

    let metrics = home_dashboard_metrics(&view);
    assert!(metrics
        .iter()
        .any(|metric| metric.label == "Drives" && metric.value == "7"));
    assert!(metrics
        .iter()
        .any(|metric| metric.label == "Capacity" && metric.value == "87.5 TiB free"));
    assert!(metrics
        .iter()
        .any(|metric| metric.label == "Throughput" && metric.state == "1 day"));
    let chart_points = home_throughput_chart_points(&view);
    assert_eq!(chart_points.len(), 3);
    assert_eq!(chart_points[0].date, "2026-07-07");
    assert_eq!(chart_points[2].date, "2026-07-09");
    assert!(chart_points
        .iter()
        .all(|point| point.x >= 48.0 && point.x <= 616.0));
    assert!(chart_points
        .iter()
        .all(|point| { point.y.is_some_and(|y| y >= 24.0 && y <= 144.0) }));
    assert_eq!(home_throughput_chart_max_tib(&chart_points), "1.8 TiB");
    assert_eq!(
        home_throughput_chart_polyline(&chart_points),
        "48.0,130.7 332.0,110.7 616.0,24.0"
    );
    assert!(metrics
        .iter()
        .any(|metric| metric.label == "Disk IO" && metric.value == "240 MiB/s write"));
    assert!(metrics
        .iter()
        .any(|metric| metric.label == "CPU" && metric.value == "42%"));
    assert!(metrics
        .iter()
        .any(|metric| metric.label == "Logged-in users" && metric.value == "2"));
    assert!(metrics
        .iter()
        .any(|metric| metric.label == "ObjectStores" && metric.value == "1"));
    assert!(metrics.iter().any(|metric| {
        metric.label == "S3 service" && metric.value == "http://192.168.1.192:3900"
    }));

    let attention = home_dashboard_attention(&view);
    assert!(attention
        .iter()
        .any(|item| item.title == "Appliance attention"));
    assert!(attention.iter().any(|item| item.title == "Memory stress"));
    assert!(attention.iter().any(|item| item.title == "SMART qnap-1057"));
}

#[test]
fn home_dashboard_telemetry_cards_cover_full_data_and_per_disk_identity() {
    let payload = serde_json::json!({
        "schema_version": "dasobjectstore.web_redesign.v1",
        "generated_at_utc": "2026-07-09T20:07:00Z",
        "health": {
            "state": "healthy",
            "label": "Healthy",
            "warning_count": 0,
            "critical_count": 0,
            "action_count": 0,
            "last_checked_at_utc": "2026-07-09T20:07:00Z"
        },
        "drives": {
            "total": 8,
            "mounted": 8,
            "healthy": 8,
            "watch": 0,
            "suspect": 0,
            "failed": 0
        },
        "capacity": {
            "total_tib": "128.0",
            "used_tib": "64.0",
            "free_tib": "64.0",
            "used_percent_basis_points": 5000
        },
        "mounted_enclosures": [{
            "enclosure_id": "qnap-tl-d800c-01",
            "display_name": "QNAP TL-D800C",
            "mount_path": "/srv/dasobjectstore/hdd",
            "connection": {
                "bus": "usb",
                "protocol": "uas",
                "link_speed": "10 Gb/s"
            },
            "health": "healthy",
            "drive_count": {
                "total": 8,
                "mounted": 8,
                "healthy": 8,
                "watch": 0,
                "suspect": 0,
                "failed": 0
            },
            "capacity": {
                "total_tib": "128.0",
                "used_tib": "64.0",
                "free_tib": "64.0",
                "used_percent_basis_points": 5000
            },
            "last_seen_at_utc": "2026-07-09T20:07:00Z",
            "warnings": []
        }],
        "telemetry_window": {
            "selected": "three_months",
            "selected_label": "3 months",
            "options": [
                { "value": "one_hour", "label": "1 hour", "selected": false },
                { "value": "one_day", "label": "1 day", "selected": false },
                { "value": "ten_days", "label": "10 days", "selected": false },
                { "value": "three_months", "label": "3 months", "selected": true }
            ]
        },
        "throughput_7d": {
            "window_days": 92,
            "read_tib": "11.0",
            "written_tib": "13.5",
            "ingest_tib": "18.0",
            "avg_read_mib_s": 180,
            "avg_write_mib_s": 320,
            "daily": [
                { "date": "2026-04-09", "read_tib": "1.0", "written_tib": "2.0", "ingest_tib": "3.0" },
                { "date": "2026-05-09", "read_tib": "2.0", "written_tib": "4.0", "ingest_tib": "6.0" },
                { "date": "2026-06-09", "read_tib": "3.0", "written_tib": "6.0", "ingest_tib": "9.0" },
                { "date": "2026-07-09", "read_tib": "4.0", "written_tib": "8.0", "ingest_tib": "18.0" }
            ]
        },
        "disk_io": {
            "available": true,
            "read_mib_s": 96,
            "write_mib_s": 384,
            "read_ops_s": 31,
            "write_ops_s": 44,
            "busiest_disk_id": "qnap-bay-04",
            "state": "elevated",
            "message": null
        },
        "cpu_usage": {
            "available": true,
            "usage_percent": 86,
            "load_average_1m": "7.84",
            "logical_core_count": 16,
            "state": "high",
            "message": null
        },
        "active_users": {
            "available": true,
            "active_sessions": 9,
            "distinct_logged_in_users": 5,
            "administrator_sessions": 2,
            "operator_sessions": 3,
            "remote_agent_sessions": 4,
            "state": "nominal",
            "message": null
        },
        "memory_stress": {
            "state": "high",
            "pressure_percent": 88,
            "swap_used_percent": 22,
            "page_cache_tib": "1.7",
            "warning": {
                "code": "memory_pressure_high",
                "message": "Memory pressure is high."
            }
        },
        "object_service": {
            "active": true,
            "remote_ready": true,
            "bind_address": "0.0.0.0",
            "port": 3900,
            "local_url": "http://127.0.0.1:3900",
            "remote_url": "http://192.168.1.192:3900",
            "service_state": "Up 12 minutes",
            "message": null
        },
        "smart_warnings": {
            "warning_count": 0,
            "affected_drive_count": 0,
            "warnings": []
        },
        "object_stores": [{
            "store_id": "generated-data",
            "display_name": "generated-data",
            "health": "healthy",
            "object_count": 142,
            "warnings": []
        }]
    });
    let view = serde_json::from_value::<HomeDashboardResponse>(payload).expect("dashboard decodes");

    let metrics = home_dashboard_metrics(&view);
    let capacity = metrics
        .iter()
        .find(|metric| metric.label == "Capacity")
        .expect("capacity metric");
    let throughput = metrics
        .iter()
        .find(|metric| metric.label == "Throughput")
        .expect("throughput metric");
    let disk_io = metrics
        .iter()
        .find(|metric| metric.label == "Disk IO")
        .expect("disk io metric");
    let cpu = metrics
        .iter()
        .find(|metric| metric.label == "CPU")
        .expect("cpu metric");
    let active_users = metrics
        .iter()
        .find(|metric| metric.label == "Logged-in users")
        .expect("active users metric");
    let memory = metrics
        .iter()
        .find(|metric| metric.label == "Memory stress")
        .expect("memory metric");

    assert_eq!(capacity.value, "64.0 TiB free");
    assert_eq!(capacity.state, "50.0% used");
    assert_eq!(throughput.value, "18.0 TiB ingest");
    assert_eq!(throughput.state, "3 months");
    assert_eq!(disk_io.value, "384 MiB/s write");
    assert_eq!(
        disk_io.detail,
        "96 MiB/s read; 44 write ops/s; 31 read ops/s"
    );
    assert_eq!(disk_io.state, "elevated");
    assert_eq!(view.disk_io.busiest_disk_id.as_deref(), Some("qnap-bay-04"));
    assert_eq!(cpu.value, "86%");
    assert_eq!(cpu.detail, "load 7.84; 16 logical core(s)");
    assert_eq!(cpu.state, "high");
    assert_eq!(active_users.value, "5");
    assert_eq!(
        active_users.detail,
        "9 active session(s); 2 admin; 4 remote"
    );
    assert_eq!(memory.value, "88%");
    assert_eq!(memory.detail, "22% swap; 1.7 TiB page cache");
    assert_eq!(memory.state, "high");

    let chart_points = home_throughput_chart_points(&view);
    assert_eq!(chart_points.len(), 4);
    assert_eq!(chart_points[0].date, "2026-04-09");
    assert_eq!(chart_points[3].date, "2026-07-09");
    assert_eq!(home_throughput_chart_max_tib(&chart_points), "18 TiB");
    assert_eq!(
        home_throughput_chart_polyline(&chart_points),
        "48.0,124.0 237.3,104.0 426.7,84.0 616.0,24.0"
    );
}

#[test]
fn home_dashboard_telemetry_tests_sparse_missing_and_invalid_chart_samples() {
    let payload = serde_json::json!({
        "schema_version": "dasobjectstore.web_redesign.v1",
        "generated_at_utc": "2026-07-09T20:08:00Z",
        "health": {
            "state": "watch",
            "label": "Watch",
            "warning_count": 1,
            "critical_count": 0,
            "action_count": 1,
            "last_checked_at_utc": null
        },
        "drives": {
            "total": 4,
            "mounted": 3,
            "healthy": 2,
            "watch": 1,
            "suspect": 0,
            "failed": 1
        },
        "capacity": {
            "total_tib": "64.0",
            "used_tib": "12.0",
            "free_tib": "52.0",
            "used_percent_basis_points": 1875
        },
        "mounted_enclosures": [],
        "telemetry_window": {
            "selected": "ten_days",
            "selected_label": "10 days",
            "options": [
                { "value": "one_hour", "label": "1 hour", "selected": false },
                { "value": "one_day", "label": "1 day", "selected": false },
                { "value": "ten_days", "label": "10 days", "selected": true },
                { "value": "three_months", "label": "3 months", "selected": false }
            ]
        },
        "throughput_7d": {
            "window_days": 10,
            "read_tib": "0.0",
            "written_tib": "0.0",
            "ingest_tib": "0.0",
            "avg_read_mib_s": 0,
            "avg_write_mib_s": 0,
            "daily": [
                { "date": "2026-07-01", "read_tib": "0.0", "written_tib": "0.0", "ingest_tib": "" },
                { "date": "2026-07-02", "read_tib": "0.0", "written_tib": "0.0", "ingest_tib": "missing" },
                { "date": "2026-07-03", "read_tib": "0.0", "written_tib": "0.0", "ingest_tib": "-1.0" },
                { "date": "2026-07-04", "read_tib": "0.0", "written_tib": "0.0", "ingest_tib": "0.25 TiB" }
            ]
        },
        "disk_io": {
            "available": false,
            "read_mib_s": 0,
            "write_mib_s": 0,
            "read_ops_s": 0,
            "write_ops_s": 0,
            "busiest_disk_id": null,
            "state": "unavailable",
            "message": "Disk IO counters are unavailable for this host."
        },
        "cpu_usage": {
            "available": false,
            "usage_percent": null,
            "load_average_1m": null,
            "logical_core_count": null,
            "state": "unavailable",
            "message": "CPU telemetry was not sampled in this window."
        },
        "active_users": {
            "available": false,
            "active_sessions": 0,
            "distinct_logged_in_users": 0,
            "administrator_sessions": 0,
            "operator_sessions": 0,
            "remote_agent_sessions": 0,
            "state": "unavailable",
            "message": "Session telemetry is unavailable."
        },
        "memory_stress": {
            "state": "nominal",
            "pressure_percent": 0,
            "swap_used_percent": 0,
            "page_cache_tib": "0.0",
            "warning": null
        },
        "object_service": {
            "active": false,
            "remote_ready": false,
            "bind_address": "127.0.0.1",
            "port": 3900,
            "local_url": "http://127.0.0.1:3900",
            "remote_url": null,
            "service_state": null,
            "message": "S3-compatible object service is offline."
        },
        "smart_warnings": {
            "warning_count": 0,
            "affected_drive_count": 0,
            "warnings": []
        },
        "object_stores": []
    });
    let view = serde_json::from_value::<HomeDashboardResponse>(payload).expect("dashboard decodes");

    let metrics = home_dashboard_metrics(&view);
    let throughput = metrics
        .iter()
        .find(|metric| metric.label == "Throughput")
        .expect("throughput metric");
    let disk_io = metrics
        .iter()
        .find(|metric| metric.label == "Disk IO")
        .expect("disk io metric");
    let cpu = metrics
        .iter()
        .find(|metric| metric.label == "CPU")
        .expect("cpu metric");
    let active_users = metrics
        .iter()
        .find(|metric| metric.label == "Logged-in users")
        .expect("active users metric");

    assert_eq!(throughput.state, "10 days");
    assert_eq!(disk_io.value, "Unavailable");
    assert_eq!(
        disk_io.detail,
        "Disk IO counters are unavailable for this host."
    );
    assert_eq!(cpu.value, "Unavailable");
    assert_eq!(cpu.detail, "CPU telemetry was not sampled in this window.");
    assert_eq!(active_users.value, "Unavailable");
    assert_eq!(active_users.detail, "Session telemetry is unavailable.");

    let chart_points = home_throughput_chart_points(&view);
    assert_eq!(chart_points.len(), 4);
    assert_eq!(chart_points[0].date, "2026-07-01");
    assert_eq!(chart_points[1].date, "2026-07-02");
    assert_eq!(chart_points[2].date, "2026-07-03");
    assert_eq!(chart_points[3].date, "2026-07-04");
    assert_eq!(chart_points[0].ingest_tib, None);
    assert_eq!(chart_points[1].ingest_tib, None);
    assert_eq!(chart_points[2].ingest_tib, Some(0.0));
    assert_eq!(chart_points[3].ingest_tib, Some(0.25));
    assert_eq!(home_throughput_chart_max_tib(&chart_points), "0.2 TiB");
    assert_eq!(
        home_throughput_chart_polyline(&chart_points),
        "426.7,144.0 616.0,114.0"
    );
}

#[test]
fn home_throughput_chart_preserves_invalid_sample_gaps() {
    let days = vec![
        ThroughputDayResponse {
            date: "2026-07-01".to_string(),
            read_tib: "0".to_string(),
            written_tib: "0".to_string(),
            ingest_tib: "1.0".to_string(),
        },
        ThroughputDayResponse {
            date: "2026-07-02".to_string(),
            read_tib: "0".to_string(),
            written_tib: "0".to_string(),
            ingest_tib: "missing".to_string(),
        },
        ThroughputDayResponse {
            date: "2026-07-03".to_string(),
            read_tib: "0".to_string(),
            written_tib: "0".to_string(),
            ingest_tib: "2.0".to_string(),
        },
    ];
    let points = super::throughput_chart_points(&days);
    assert_eq!(points.len(), 3);
    assert_eq!(points[1].ingest_tib, None);
    assert_eq!(
        home_throughput_chart_segments(&points),
        vec!["48.0,84.0".to_string(), "616.0,24.0".to_string()]
    );
}

#[test]
fn home_telemetry_dom_contract_prevents_jitter_overlap_and_mobile_breakage() {
    let source = workspace_component_source();
    let css = web_styles_source();
    let activity_css = activity_styles_source();

    assert!(source.contains("<div class=\"dos-metric-grid\">"));
    assert!(source.contains("home_dashboard_metrics(view).into_iter().map(render_metric_card)"));
    assert!(source.contains("render_home_throughput_chart(view)"));
    assert!(source.contains("class=\"dos-card dos-home-chart-card\""));
    assert!(source.contains("data-throughput-source"));
    assert!(source.contains("<div class=\"dos-home-chart-frame\">"));
    assert!(source.contains("class=\"dos-home-throughput-chart\""));
    assert!(source.contains("viewBox={format!"));
    assert!(source.contains("HOME_THROUGHPUT_CHART_WIDTH"));
    assert!(source.contains("HOME_THROUGHPUT_CHART_HEIGHT"));
    assert!(source.contains("class={format!(\"dos-chart-line {source_class}\")}"));
    assert!(source.contains("home_throughput_chart_segments"));
    assert!(source.contains("class=\"dos-chart-point\""));
    assert!(source.contains("class=\"dos-chart-empty\""));
    assert!(source.contains("dos-home-chart-badges"));
    assert!(source.contains("dos-telemetry-source"));
    assert!(source.contains("dos-chart-line {source_class}"));
    assert!(source.contains("dos-chart-message"));
    assert!(source.contains("dos-chart-gap-message"));
    assert!(source.contains("dos-home-telemetry-toolbar"));
    assert!(source.contains("dos-window-segments"));

    assert!(css.contains(".dos-metric-grid,\n.dos-store-grid,\n.dos-attention-grid {"));
    assert!(css.contains("grid-template-columns: repeat(4, minmax(0, 1fr));"));
    assert!(css.contains(".dos-card {\n  min-height: 140px;"));
    assert!(css.contains(".dos-metric-card strong,\n.dos-store-card strong {"));
    assert!(css.contains("overflow-wrap: anywhere;"));
    assert!(css.contains(".dos-home-telemetry-toolbar {\n  display: flex;\n  flex-wrap: wrap;"));
    assert!(css.contains(".dos-window-segments {\n  display: flex;\n  flex-wrap: wrap;"));
    assert!(css.contains(".dos-window-segment {\n  min-height: 32px;"));
    assert!(css.contains(".dos-home-chart-card {\n  min-height: 280px;"));
    assert!(css.contains(".dos-home-chart-badges {\n  display: flex;"));
    assert!(css.contains(".dos-telemetry-source-daemon {"));
    assert!(css.contains(".dos-telemetry-source-unavailable {"));
    assert!(css.contains(".dos-chart-line.dos-telemetry-source-legacy {"));
    assert!(css.contains(".dos-chart-gap-message {\n  fill: #56666d;"));
    assert!(css.contains(".dos-home-chart-frame {\n  height: 210px;\n  overflow: hidden;"));
    assert!(css.contains(
        ".dos-home-throughput-chart {\n  display: block;\n  width: 100%;\n  height: 210px;"
    ));
    assert!(css.contains(".dos-chart-axis {\n  stroke: #70828a;"));
    assert!(css.contains("vector-effect: non-scaling-stroke;"));
    assert!(css.contains(
        ".dos-chart-label,\n.dos-chart-empty,\n.dos-chart-gap-message {\n  fill: #56666d;"
    ));
    assert!(css.contains("@media (max-width: 980px)"));
    assert!(css.contains(
        ".dos-metric-grid,\n  .dos-store-grid {\n    grid-template-columns: repeat(2, minmax(0, 1fr));"
    ));
    assert!(activity_css
        .contains(".dos-activity-grid {\n    grid-template-columns: repeat(2, minmax(0, 1fr));"));
    assert!(css.contains("@media (max-width: 640px)"));
    assert!(css.contains(".dos-metric-grid,\n  .dos-store-grid,\n  .dos-form-grid,"));
    assert!(activity_css.contains(
        ".dos-activity-grid,\n  .dos-activity-queues {\n    grid-template-columns: 1fr;"
    ));
    assert!(css.contains("grid-template-columns: 1fr;"));
}

#[test]
fn enclosures_css_is_feature_owned_and_registered_before_base_styles() {
    let html = include_str!("../../index.html");
    let enclosures_link = "styles/enclosures.css";
    assert_eq!(html.matches(enclosures_link).count(), 1);
    assert!(html.find(enclosures_link).unwrap() < html.find("styles.css").unwrap());

    let css = include_str!("../../styles/enclosures.css");
    for selector in [
        ".dos-two-column",
        ".dos-enclosure-card",
        ".dos-detail-list",
        ".dos-drive-card",
        ".dos-slot-list",
    ] {
        assert!(
            css.contains(selector),
            "missing enclosure CSS selector {selector}"
        );
    }
    let base = include_str!("../../styles.css");
    for selector in [
        ".dos-two-column",
        ".dos-enclosure-card",
        ".dos-detail-list",
        ".dos-drive-card",
        ".dos-slot-list",
    ] {
        assert!(
            !base.contains(selector),
            "enclosure selector leaked into base CSS: {selector}"
        );
    }
}

#[test]
fn activity_css_is_feature_owned_and_registered_before_base_styles() {
    let base = include_str!("../../styles.css");
    let feature = activity_styles_source();
    let index = include_str!("../../index.html");

    assert!(!base.contains(".dos-activity"));
    assert!(!base.contains(".dos-task-list"));
    assert!(!base.contains(".dos-task-card"));
    for selector in [
        ".dos-activity-grid",
        ".dos-activity-queues",
        ".dos-activity-tasks",
        ".dos-task-list",
        ".dos-task-card",
        "@media (max-width: 980px)",
        "@media (max-width: 640px)",
    ] {
        assert!(
            feature.contains(selector),
            "missing Activity selector {selector}"
        );
    }
    let feature_link = index
        .find("styles/activity.css")
        .expect("Activity sheet registered");
    let base_link = index.find("styles.css").expect("base sheet registered");
    assert!(feature_link < base_link);
    assert_eq!(index.matches("styles/activity.css").count(), 1);
}

#[test]
fn home_throughput_source_contract_has_distinct_visual_states() {
    assert_eq!(
        home_throughput_source_label("daemon_disk_io"),
        "Daemon telemetry"
    );
    assert_eq!(
        home_throughput_source_class("legacy_file"),
        "dos-telemetry-source-legacy"
    );
    assert_eq!(
        home_throughput_source_class("unavailable"),
        "dos-telemetry-source-unavailable"
    );
}

#[test]
fn home_dashboard_attention_surfaces_capacity_enclosure_and_store_signals() {
    let payload = serde_json::json!({
        "schema_version": "dasobjectstore.web_redesign.v1",
        "generated_at_utc": "2026-07-08T08:00:00Z",
        "health": {
            "state": "watch",
            "label": "Watch",
            "warning_count": 0,
            "critical_count": 0,
            "action_count": 0,
            "last_checked_at_utc": null
        },
        "drives": {
            "total": 7,
            "mounted": 7,
            "healthy": 6,
            "watch": 0,
            "suspect": 1,
            "failed": 0
        },
        "capacity": {
            "total_tib": "100.0",
            "used_tib": "91.0",
            "free_tib": "9.0",
            "used_percent_basis_points": 9100
        },
        "mounted_enclosures": [{
            "enclosure_id": "tl-d800c-1",
            "display_name": "QNAP TL-D800C",
            "mount_path": "/srv/dasobjectstore",
            "connection": {
                "bus": "usb",
                "protocol": "uas",
                "link_speed": "10Gbps"
            },
            "health": "healthy",
            "drive_count": {
                "total": 7,
                "mounted": 7,
                "healthy": 6,
                "watch": 1,
                "suspect": 0,
                "failed": 0
            },
            "capacity": {
                "total_tib": "100.0",
                "used_tib": "91.0",
                "free_tib": "9.0",
                "used_percent_basis_points": 9100
            },
            "last_seen_at_utc": "2026-07-08T08:00:00Z",
            "warnings": [{
                "code": "enclosure_usb_reset",
                "message": "USB reset observed on this enclosure."
            }]
        }],
        "throughput_7d": {
            "window_days": 7,
            "read_tib": "1.0",
            "written_tib": "2.0",
            "ingest_tib": "2.5",
            "avg_read_mib_s": 120,
            "avg_write_mib_s": 240
        },
        "ingest": {
            "pressure": "critical",
            "queued_jobs": 2,
            "active_jobs": 1,
            "failed_jobs": 1,
            "jobs": [],
            "warnings": [{
                "code": "ingest_critical_pressure",
                "message": "SSD ingest pressure is critical; new writes may be blocked."
            }]
        },
        "destage": {
            "pending_objects": 3,
            "copying_objects": 1,
            "verified_objects": 4,
            "objects": [],
            "warnings": [{
                "code": "destage_objects_need_review",
                "message": "One or more destage objects need review before SSD eviction."
            }]
        },
        "memory_stress": {
            "state": "nominal",
            "pressure_percent": 31,
            "swap_used_percent": 0,
            "page_cache_tib": "0.4",
            "warning": null
        },
        "object_service": {
            "active": true,
            "remote_ready": false,
            "bind_address": "127.0.0.1",
            "port": 3900,
            "local_url": "http://127.0.0.1:3900",
            "remote_url": null,
            "service_state": "Up 1 minute",
            "message": "S3-compatible object service is bound to loopback."
        },
        "smart_warnings": {
            "warning_count": 0,
            "affected_drive_count": 0,
            "warnings": []
        },
        "object_stores": [{
            "store_id": "zymo_fecal_2025.05",
            "display_name": "zymo_fecal_2025.05",
            "health": "healthy",
            "object_count": 42,
            "endpoint_export_mode": null,
            "warnings": []
        }]
    });
    let view = serde_json::from_value::<HomeDashboardResponse>(payload).expect("dashboard decodes");

    let attention = home_dashboard_attention(&view);

    assert!(attention.iter().any(|item| item.title == "Drive health"
        && item.state == "warning"
        && item.detail.contains("1 suspect")));
    assert!(attention
        .iter()
        .any(|item| item.title == "Capacity pressure"
            && item.state == "critical"
            && item.detail.contains("91.0 TiB used")));
    assert!(attention.iter().any(|item| item.title == "Ingest queue"
        && item.state == "critical"
        && item.detail.contains("SSD ingest pressure is critical")));
    assert!(attention.iter().any(|item| item.title == "Destage queue"
        && item.state == "warning"
        && item.detail.contains("destage objects need review")));
    assert!(attention
        .iter()
        .any(|item| item.title == "Enclosure QNAP TL-D800C"
            && item.state == "warning"
            && item.detail.contains("USB reset")));
    assert!(attention
        .iter()
        .any(|item| item.title == "ObjectStore zymo_fecal_2025.05"
            && item.state == "warning"
            && item.detail.contains("object-service export mode")));
}

#[test]
fn home_dashboard_attention_clear_state_has_operator_copy() {
    let payload = serde_json::json!({
        "schema_version": "dasobjectstore.web_redesign.v1",
        "generated_at_utc": "2026-07-08T08:00:00Z",
        "health": {
            "state": "healthy",
            "label": "Healthy",
            "warning_count": 0,
            "critical_count": 0,
            "action_count": 0,
            "last_checked_at_utc": "2026-07-08T08:00:00Z"
        },
        "drives": {
            "total": 7,
            "mounted": 7,
            "healthy": 7,
            "watch": 0,
            "suspect": 0,
            "failed": 0
        },
        "capacity": {
            "total_tib": "100.0",
            "used_tib": "45.0",
            "free_tib": "55.0",
            "used_percent_basis_points": 4500
        },
        "mounted_enclosures": [{
            "enclosure_id": "tl-d800c-1",
            "display_name": "QNAP TL-D800C",
            "mount_path": "/srv/dasobjectstore",
            "connection": {
                "bus": "usb",
                "protocol": "uas",
                "link_speed": "10Gbps"
            },
            "health": "healthy",
            "drive_count": {
                "total": 7,
                "mounted": 7,
                "healthy": 7,
                "watch": 0,
                "suspect": 0,
                "failed": 0
            },
            "capacity": {
                "total_tib": "100.0",
                "used_tib": "45.0",
                "free_tib": "55.0",
                "used_percent_basis_points": 4500
            },
            "last_seen_at_utc": "2026-07-08T08:00:00Z",
            "warnings": []
        }],
        "throughput_7d": {
            "window_days": 7,
            "read_tib": "1.0",
            "written_tib": "2.0",
            "ingest_tib": "2.5",
            "avg_read_mib_s": 120,
            "avg_write_mib_s": 240
        },
        "ingest": {
            "pressure": "normal",
            "queued_jobs": 0,
            "active_jobs": 0,
            "failed_jobs": 0,
            "jobs": [],
            "warnings": []
        },
        "destage": {
            "pending_objects": 0,
            "copying_objects": 0,
            "verified_objects": 2,
            "objects": [],
            "warnings": []
        },
        "memory_stress": {
            "state": "nominal",
            "pressure_percent": 31,
            "swap_used_percent": 0,
            "page_cache_tib": "0.4",
            "warning": null
        },
        "object_service": {
            "active": true,
            "remote_ready": true,
            "bind_address": "0.0.0.0",
            "port": 3900,
            "local_url": "http://127.0.0.1:3900",
            "remote_url": "http://192.168.1.192:3900",
            "service_state": "Up 1 minute",
            "message": null
        },
        "smart_warnings": {
            "warning_count": 0,
            "affected_drive_count": 0,
            "warnings": []
        },
        "object_stores": [{
            "store_id": "zymo_fecal_2025.05",
            "display_name": "zymo_fecal_2025.05",
            "health": "healthy",
            "object_count": 42,
            "endpoint_export_mode": "s3_bucket",
            "warnings": []
        }]
    });
    let view = serde_json::from_value::<HomeDashboardResponse>(payload).expect("dashboard decodes");

    let attention = home_dashboard_attention(&view);

    assert_eq!(attention.len(), 1);
    assert_eq!(attention[0].title, "No operator attention required");
    assert!(!attention[0].detail.contains("bootstrapped"));
    assert!(!attention[0].detail.contains("fixture"));
}

#[test]
fn remote_upload_selection_summary_counts_files_folders_and_bytes() {
    let files = vec![
        RemoteUploadSelectedFile {
            display_path: "study-a/raw/r1.fastq.gz".to_string(),
            size_bytes: 2 * 1024 * 1024 * 1024,
        },
        RemoteUploadSelectedFile {
            display_path: "study-a/raw/r2.fastq.gz".to_string(),
            size_bytes: 3 * 1024 * 1024 * 1024,
        },
        RemoteUploadSelectedFile {
            display_path: "study-b/manifest.tsv".to_string(),
            size_bytes: 4096,
        },
    ];

    let summary = RemoteUploadSelectionSummary::from_files(&files);

    assert_eq!(remote_upload_folder_count(&files), 2);
    assert_eq!(summary.file_count, 3);
    assert_eq!(summary.folder_count, 2);
    assert_eq!(summary.total_size_label(), "5.0 GiB");
    assert_eq!(
        summary.largest_file_label(),
        "study-a/raw/r2.fastq.gz (3.0 GiB)"
    );
    assert_eq!(summary.sample_paths.len(), 3);
}

#[test]
fn remote_upload_component_contract_covers_drag_drop_agent_handoff() {
    let source = workspace_component_source();
    let css = web_styles_source();

    assert!(source.contains("dos-remote-upload-panel"));
    assert!(source.contains("dos-remote-upload-dropzone"));
    assert!(source.contains("Drop files or folders here"));
    assert!(source.contains("webkitdirectory=true"));
    assert!(source.contains("remote_upload_selected_files_from_list"));
    assert!(source.contains("webkitRelativePath"));
    assert!(source.contains("Confirm with local agent"));
    assert!(source.contains("Loopback agent coordination is the next remote-upload task."));
    assert!(source.contains("RemoteUploadSelectionSummary::from_files"));
    assert!(source.contains("render_remote_upload_target_context"));
    assert!(source.contains("data-target-store-id"));
    assert!(source.contains("paired_agent_ingress_origin"));
    assert!(source.contains("capacity: {} used; {} free"));
    assert!(source.contains("let selected_target = ready_stores"));
    assert!(source.contains("if selected_target.is_some()"));
    assert!(source.contains("pub target_store_id: String"));
    assert!(!source.contains("pub target_store_id: Option<String>"));
    assert!(
        source.contains("Select a writable ObjectStore from ObjectStores before choosing files.")
    );
    assert!(!source.contains("ready_stores.first()"));

    assert!(css.contains(".dos-remote-upload-panel"));
    assert!(css.contains(".dos-remote-upload-grid"));
    assert!(css.contains(".dos-remote-upload-dropzone"));
    assert!(css.contains(".dos-remote-upload-summary"));
    assert!(css.contains(".dos-remote-upload-samples"));
    assert!(css.contains(".dos-remote-upload-grid,\n  .dos-remote-upload-summary"));
}

#[test]
fn local_access_component_contract_is_users_first_and_task_pane_scoped() {
    let source = workspace_component_source();

    for marker in [
        "data-section=\"users-toolbar\"",
        "data-section=\"users-inventory\"",
        "data-section=\"groups-context\"",
        "data-step=\"identify-user\"",
        "data-step=\"qualification\"",
        "data-step=\"groups\"",
        "data-step=\"review\"",
        "Review and apply",
        "the browser never creates operating-system accounts",
        "dos-users-table",
    ] {
        assert!(
            source.contains(marker),
            "missing Local Access marker: {marker}"
        );
    }
    for header in [
        "Qualification",
        "Access groups",
        "Administrator",
        "Sessions",
    ] {
        assert!(
            source.contains(&format!("{header}")),
            "missing users table header: {header}"
        );
    }
    assert!(source.contains("disabled={!view.capabilities.administrator_actions_enabled}"));
    assert!(source.contains("TaskPaneMode::Closed"));
    assert!(source.contains("TaskPaneMode::Create"));
    assert!(source.contains("return_focus_to"));
    assert!(!source.contains("data-action=\"assign_local_user_to_group\""));
    let css = web_styles_source();
    for selector in [
        ".dos-users-toolbar",
        ".dos-users-table",
        ".dos-task-pane__section",
    ] {
        assert!(
            css.contains(selector),
            "missing Local Access style: {selector}"
        );
    }
}

#[test]
fn endpoints_component_contract_is_inventory_first_and_task_pane_scoped() {
    let source = workspace_component_source();
    for marker in [
        "data-section=\"endpoints-toolbar\"",
        "data-section=\"endpoint-inventory\"",
        "data-section=\"endpoint-identity\"",
        "data-section=\"endpoint-binding\"",
        "data-section=\"endpoint-review\"",
        "Add endpoint",
        "Edit",
        "endpoint_form_state_from_item",
        "refresh_endpoints_workspace",
        "return_focus_to",
    ] {
        assert!(
            source.contains(marker),
            "missing Endpoints marker: {marker}"
        );
    }
    assert!(source.contains("TaskPaneMode::Create"));
    assert!(source.contains("TaskPaneMode::Edit"));
    assert!(!source.contains("render_endpoint_upsert_card(form_state, api_base_path)"));
    let css = web_styles_source();
    for selector in [".dos-endpoints-toolbar", ".dos-endpoints-table"] {
        assert!(
            css.contains(selector),
            "missing Endpoints style: {selector}"
        );
    }
}

fn users_groups_workspace_fixture() -> UsersGroupsWorkspaceResponse {
    UsersGroupsWorkspaceResponse {
        host_mode: "standalone".to_string(),
        authentication_framework: prosopikon_core::ProsopikonAuthenticationFramework::Hybrid,
        device_token_requirement: prosopikon_core::ProsopikonDeviceTokenRequirement::NotRequired,
        current_user: Some(LocalUserAuthorityResponse {
            username: "operator".to_string(),
            groups: vec!["sudo".to_string(), "mnemosyne".to_string()],
            sudo_administrator: true,
        }),
        users: vec![StandaloneUserAccountResponse {
            username: "operator".to_string(),
            registered: true,
            created_at_unix_seconds: 1,
            registered_at_unix_seconds: Some(2),
            active_session_count: 1,
            qualification_state: "qualified".to_string(),
            groups: vec!["sudo".to_string(), "mnemosyne".to_string()],
            sudo_administrator: true,
        }],
        groups: vec![
            LocalGroupMembershipResponse {
                group_name: "sudo".to_string(),
                current_user_member: true,
                sudo_administrator_group: true,
            },
            LocalGroupMembershipResponse {
                group_name: "mnemosyne".to_string(),
                current_user_member: true,
                sudo_administrator_group: false,
            },
        ],
        groups_file_path: "/opt/dasobjectstore/groups.json".to_string(),
        writer_groups: vec![StorageGroupResponse {
            group_name: "mnemosyne".to_string(),
            display_name: "Mnemosyne".to_string(),
            source: "object_storage_group_registry".to_string(),
            current_user_member: true,
        }],
        operations: vec![
            LocalGroupOperationResponse {
                kind: "create_local_group".to_string(),
                label: "Create local writer/admin group".to_string(),
                requires_sudo_administrator: true,
                enabled: true,
                blocked_reason: None,
            },
            LocalGroupOperationResponse {
                kind: "assign_local_user_to_group".to_string(),
                label: "Assign local user to group".to_string(),
                requires_sudo_administrator: true,
                enabled: true,
                blocked_reason: None,
            },
        ],
        capabilities: UsersGroupsCapabilitiesResponse {
            product_local_user_registration: true,
            os_local_user_management: true,
            os_local_group_management: true,
            administrator_actions_enabled: true,
        },
        selected_username: Some("operator".to_string()),
        selected_group_name: Some("mnemosyne".to_string()),
        warnings: Vec::new(),
    }
}
