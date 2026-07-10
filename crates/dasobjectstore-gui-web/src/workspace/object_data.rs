use super::*;

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum ObjectBrowserDownloadState {
    Idle,
    Starting { label: String },
    Started { filename: String, detail: String },
    PermissionDenied { message: String },
    Error { message: String },
}

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ObjectStoreCreateFormState {
    pub(super) open: bool,
    pub(super) store_id: String,
    pub(super) writer_group: String,
    pub(super) enclosure_id: String,
    pub(super) object_type: String,
    pub(super) required_copies: u8,
    pub(super) public: bool,
    pub(super) writeable: bool,
    pub(super) store_class: String,
    pub(super) capacity_behavior: String,
    pub(super) retention: String,
    pub(super) endpoint_export_mode: String,
    pub(super) bucket: String,
    pub(super) ssd_root: String,
    pub(super) planning: bool,
    pub(super) plan: Option<GuiActionPlanResponse>,
    pub(super) confirmation_phrase: String,
    pub(super) submitting: bool,
    pub(super) submitted: Option<CreateObjectStoreResponse>,
    pub(super) error: Option<String>,
}

#[cfg(target_arch = "wasm32")]
impl ObjectStoreCreateFormState {
    pub(super) fn from_view(view: Option<&ObjectStoresPageResponse>) -> Self {
        let default_store_class = view
            .map(|view| view.create_object_store.defaults.store_class.clone())
            .unwrap_or_else(|| "generated_data".to_string());
        let default_copies = view
            .map(|view| view.create_object_store.defaults.required_copies)
            .unwrap_or(1);
        let endpoint_export_mode = view
            .map(|view| {
                view.create_object_store
                    .defaults
                    .endpoint_export_mode
                    .clone()
            })
            .unwrap_or_else(|| "s3_bucket".to_string());
        let writer_group = view
            .and_then(|view| view.groups.first())
            .map(|group| group.group_name.clone())
            .unwrap_or_default();
        let selected_enclosure = view.and_then(|view| view.mounted_enclosures.first());
        let enclosure_id = selected_enclosure
            .map(|enclosure| enclosure.enclosure_id.clone())
            .unwrap_or_default();
        let ssd_root = selected_enclosure
            .map(enclosure_ssd_root)
            .unwrap_or_else(|| "/srv/dasobjectstore/ssd".to_string());

        Self {
            open: false,
            store_id: String::new(),
            writer_group,
            enclosure_id,
            object_type: "naive".to_string(),
            required_copies: default_copies,
            public: false,
            writeable: true,
            store_class: default_store_class,
            capacity_behavior: "backpressure_by_priority".to_string(),
            retention: "retain_until_deleted".to_string(),
            endpoint_export_mode,
            bucket: String::new(),
            ssd_root,
            planning: false,
            plan: None,
            confirmation_phrase: String::new(),
            submitting: false,
            submitted: None,
            error: None,
        }
    }

    pub(super) fn reset_plan(&mut self) {
        self.plan = None;
        self.submitted = None;
        self.error = None;
    }
}

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ObjectStoreConfigureFormState {
    pub(super) open: bool,
    pub(super) selected_store_id: String,
    pub(super) ingest_mode: String,
    pub(super) confirmation_marker: String,
    pub(super) store_class: String,
    pub(super) required_copies: u8,
    pub(super) writer_group: String,
    pub(super) public: bool,
    pub(super) writeable: bool,
    pub(super) capacity_behavior: String,
    pub(super) retention: String,
    pub(super) endpoint_export_mode: String,
    pub(super) ssd_root: String,
    pub(super) planning: bool,
    pub(super) plan: Option<GuiActionPlanResponse>,
    pub(super) error: Option<String>,
    pub(super) submitting: bool,
    pub(super) submitted: Option<ObjectStoreIngestPolicyResponse>,
}

#[cfg(target_arch = "wasm32")]
impl ObjectStoreConfigureFormState {
    pub(super) fn from_view(view: Option<&ObjectStoresPageResponse>) -> Self {
        let selected = view.and_then(|view| view.stores.first());
        Self {
            open: false,
            selected_store_id: selected
                .map(|store| store.store_id.clone())
                .unwrap_or_default(),
            ingest_mode: selected
                .and_then(|store| store.ingest_mode.clone())
                .unwrap_or_else(|| "ssd_first".to_string()),
            confirmation_marker: String::new(),
            store_class: selected
                .and_then(|store| store.store_class.clone())
                .or_else(|| view.map(|view| view.create_object_store.defaults.store_class.clone()))
                .unwrap_or_else(|| "generated_data".to_string()),
            required_copies: selected
                .and_then(|store| store.required_copies)
                .or_else(|| view.map(|view| view.create_object_store.defaults.required_copies))
                .unwrap_or(1),
            writer_group: selected
                .and_then(|store| store.writer_group.clone())
                .or_else(|| {
                    view.and_then(|view| view.groups.first())
                        .map(|group| group.group_name.clone())
                })
                .unwrap_or_default(),
            public: selected.and_then(|store| store.public).unwrap_or(false),
            writeable: selected.and_then(|store| store.writeable).unwrap_or(true),
            capacity_behavior: "backpressure_by_priority".to_string(),
            retention: "tombstone_then_gc".to_string(),
            endpoint_export_mode: selected
                .and_then(|store| store.endpoint_export_mode.clone())
                .or_else(|| {
                    view.map(|view| {
                        view.create_object_store
                            .defaults
                            .endpoint_export_mode
                            .clone()
                    })
                })
                .unwrap_or_else(|| "s3".to_string()),
            ssd_root: "/srv/dasobjectstore/ssd".to_string(),
            planning: false,
            plan: None,
            error: None,
            submitting: false,
            submitted: None,
        }
    }

    pub(super) fn apply_store(&mut self, store: &ObjectStoreCardResponse) {
        self.selected_store_id = store.store_id.clone();
        self.ingest_mode = store
            .ingest_mode
            .clone()
            .unwrap_or_else(|| "ssd_first".to_string());
        self.store_class = store
            .store_class
            .clone()
            .unwrap_or_else(|| "generated_data".to_string());
        self.required_copies = store.required_copies.unwrap_or(1);
        self.writer_group = store.writer_group.clone().unwrap_or_default();
        self.public = store.public.unwrap_or(false);
        self.writeable = store.writeable.unwrap_or(true);
        self.endpoint_export_mode = store
            .endpoint_export_mode
            .clone()
            .unwrap_or_else(|| "s3".to_string());
        self.reset_plan();
    }

    pub(super) fn reset_plan(&mut self) {
        self.plan = None;
        self.error = None;
        self.submitted = None;
    }
}

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct SubObjectFormState {
    pub(super) open: bool,
    pub(super) subobject_name: String,
    pub(super) parent_kind: String,
    pub(super) parent_store_id: String,
    pub(super) parent_subobject_name: String,
    pub(super) object_type_mode: String,
    pub(super) object_type: String,
    pub(super) s3_routing: String,
    pub(super) ssd_root: String,
    pub(super) planning: bool,
    pub(super) plan: Option<GuiActionPlanResponse>,
    pub(super) error: Option<String>,
}

#[cfg(target_arch = "wasm32")]
impl SubObjectFormState {
    pub(super) fn from_view(view: Option<&ObjectStoresPageResponse>) -> Self {
        Self {
            open: false,
            subobject_name: String::new(),
            parent_kind: "store".to_string(),
            parent_store_id: view
                .and_then(|view| view.stores.first())
                .map(|store| store.store_id.clone())
                .unwrap_or_default(),
            parent_subobject_name: String::new(),
            object_type_mode: "inherit".to_string(),
            object_type: "naive".to_string(),
            s3_routing: "inherit_parent".to_string(),
            ssd_root: "/srv/dasobjectstore/ssd".to_string(),
            planning: false,
            plan: None,
            error: None,
        }
    }

    pub(super) fn reset_plan(&mut self) {
        self.plan = None;
        self.error = None;
    }
}

#[cfg(any(target_arch = "wasm32", test))]
pub(super) fn object_store_bucket_default(store_id: &str) -> String {
    store_id
        .trim()
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '-' {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

#[cfg(any(target_arch = "wasm32", test))]
pub(super) fn enclosure_ssd_root(enclosure: &DasEnclosureCardResponse) -> String {
    let mount_path = enclosure.mount_path.trim_end_matches('/');
    if let Some(root) = mount_path.strip_suffix("/hdd") {
        format!("{root}/ssd")
    } else {
        format!("{mount_path}/ssd")
    }
}

#[cfg(any(target_arch = "wasm32", test))]
pub(super) fn object_store_creation_fields_ready(
    store_id: &str,
    writer_group: &str,
    enclosure_id: &str,
) -> bool {
    !store_id.trim().is_empty()
        && !writer_group.trim().is_empty()
        && !enclosure_id.trim().is_empty()
}

#[cfg(any(target_arch = "wasm32", test))]
pub(super) fn object_store_create_confirmation_matches(value: &str) -> bool {
    value.trim() == "confirm create objectstore"
}

#[cfg(any(target_arch = "wasm32", test))]
#[allow(clippy::too_many_arguments)]
pub(super) fn object_store_configure_review_from_values(
    store_id: &str,
    required_copies: u8,
    writer_group: &str,
    capacity_behavior: &str,
    retention: &str,
    endpoint_export_mode: &str,
    public: bool,
    writeable: bool,
) -> String {
    format!(
        "{} · {} copy/copies · writer group {} · capacity {} · retention {} · export {} · {} · {}",
        if store_id.trim().is_empty() {
            "no store selected"
        } else {
            store_id.trim()
        },
        required_copies,
        if writer_group.trim().is_empty() {
            "pending"
        } else {
            writer_group.trim()
        },
        capacity_behavior,
        retention,
        endpoint_export_mode,
        if public { "public" } else { "private" },
        if writeable { "writeable" } else { "read-only" }
    )
}

#[cfg(target_arch = "wasm32")]
pub(super) fn object_store_configure_review(state: &ObjectStoreConfigureFormState) -> String {
    object_store_configure_review_from_values(
        &state.selected_store_id,
        state.required_copies,
        &state.writer_group,
        &state.capacity_behavior,
        &state.retention,
        &state.endpoint_export_mode,
        state.public,
        state.writeable,
    )
}

#[cfg(any(target_arch = "wasm32", test))]
pub(super) fn subobject_registry_preview_from_values(
    subobject_name: &str,
    parent_kind: &str,
    parent_store_id: &str,
    parent_subobject_name: &str,
    object_type_mode: &str,
    object_type: &str,
    s3_routing: &str,
) -> String {
    let name = if subobject_name.trim().is_empty() {
        "unnamed-subobject"
    } else {
        subobject_name.trim()
    };
    let parent = if parent_kind == "subobject" {
        if parent_subobject_name.trim().is_empty() {
            "subobject:pending"
        } else {
            parent_subobject_name.trim()
        }
    } else if parent_store_id.trim().is_empty() {
        "store:pending"
    } else {
        parent_store_id.trim()
    };
    let object_type_label = if object_type_mode == "override" {
        format!("object type {object_type}")
    } else {
        "inherits object type".to_string()
    };

    format!(
        "{} under {} · prefix {}/{} · {} · S3 routing {}",
        name, parent, parent, name, object_type_label, s3_routing
    )
}

#[cfg(target_arch = "wasm32")]
pub(super) fn subobject_registry_preview(state: &SubObjectFormState) -> String {
    subobject_registry_preview_from_values(
        &state.subobject_name,
        &state.parent_kind,
        &state.parent_store_id,
        &state.parent_subobject_name,
        &state.object_type_mode,
        &state.object_type,
        &state.s3_routing,
    )
}

#[cfg(target_arch = "wasm32")]
pub(super) fn object_store_create_request_from_state(
    state: &ObjectStoreCreateFormState,
) -> CreateObjectStoreRequest {
    let bucket = if state.bucket.trim().is_empty() {
        object_store_bucket_default(&state.store_id)
    } else {
        state.bucket.trim().to_string()
    };
    CreateObjectStoreRequest {
        store_id: state.store_id.trim().to_string(),
        store_class: state.store_class.clone(),
        required_copies: state.required_copies,
        bucket: (!bucket.is_empty()).then_some(bucket),
        writer_group: state.writer_group.trim().to_string(),
        ssd_root: state.ssd_root.trim().to_string(),
        object_type: state.object_type.clone(),
        enclosure_id: (!state.enclosure_id.trim().is_empty())
            .then(|| state.enclosure_id.trim().to_string()),
        public: state.public,
        writeable: true,
        capacity_behavior: state.capacity_behavior.clone(),
        retention: state.retention.clone(),
        endpoint_export_mode: state.endpoint_export_mode.clone(),
        dry_run: false,
        client_request_id: None,
        confirmation_marker: Some("confirm create objectstore".to_string()),
    }
}

pub fn object_store_card_summaries(view: &ObjectStoresPageResponse) -> Vec<ObjectStoreCardSummary> {
    view.stores
        .iter()
        .map(|store| {
            let store_class = store
                .store_class
                .as_deref()
                .unwrap_or("unclassified")
                .to_string();
            let copies = store
                .required_copies
                .map(|copies| format!("{copies} required copy/copies"))
                .unwrap_or_else(|| "copy policy pending".to_string());
            let capacity = store
                .capacity
                .as_ref()
                .map(|capacity| {
                    format!(
                        "{} TiB used; {} TiB free",
                        capacity.used_tib, capacity.free_tib
                    )
                })
                .unwrap_or_else(|| "capacity pending".to_string());

            ObjectStoreCardSummary {
                id: store.store_id.clone(),
                label: store_class,
                name: store.display_name.clone(),
                health: store.health.clone(),
                object_type: store.object_type.as_deref().unwrap_or("naive").to_string(),
                access: format!(
                    "{} / {}",
                    if store.public.unwrap_or(false) {
                        "public"
                    } else {
                        "private"
                    },
                    if store.writeable.unwrap_or(false) {
                        "writeable"
                    } else {
                        "read-only"
                    }
                ),
                policy: format!(
                    "{}; {}",
                    copies,
                    store
                        .placement_policy
                        .as_deref()
                        .unwrap_or("placement pending")
                ),
                capacity,
                objects: format!("{} object(s)", store.object_count),
                writer_group: store
                    .writer_group
                    .as_deref()
                    .unwrap_or("writer group pending")
                    .to_string(),
                writer_policy: store
                    .writer_policy
                    .as_ref()
                    .map(|policy| policy.message.clone())
                    .unwrap_or_else(|| "Writer policy readiness pending".to_string()),
                endpoint: store
                    .endpoint_export_mode
                    .as_deref()
                    .unwrap_or("endpoint pending")
                    .to_string(),
                warning_count: store.warnings.len(),
                last_ingested: store
                    .last_ingested_at_utc
                    .as_deref()
                    .unwrap_or("no ingest recorded")
                    .to_string(),
            }
        })
        .collect()
}

#[cfg(any(target_arch = "wasm32", test))]
pub(super) fn object_browser_initial_endpoint(view: &ObjectStoresPageResponse) -> Option<String> {
    view.selected_store_id
        .as_ref()
        .filter(|store_id| !store_id.trim().is_empty())
        .cloned()
        .or_else(|| view.stores.first().map(|store| store.store_id.clone()))
}

#[cfg(any(target_arch = "wasm32", test))]
pub fn object_browser_folder_summaries(
    folders: &[ObjectBrowserFolderNodeResponse],
) -> Vec<ObjectBrowserFolderSummary> {
    folders
        .iter()
        .map(|folder| ObjectBrowserFolderSummary {
            name: folder.name.clone(),
            prefix: folder.prefix.clone(),
            objects: folder
                .object_count
                .map(|count| format!("{count} object(s)"))
                .unwrap_or_else(|| "object count pending".to_string()),
            size: folder
                .total_size_bytes
                .map(format_browser_bytes)
                .unwrap_or_else(|| "size pending".to_string()),
            readiness: labelize_state(&folder.readiness),
        })
        .collect()
}

#[cfg(any(target_arch = "wasm32", test))]
pub fn object_browser_file_summaries(
    files: &[ObjectBrowserFileNodeResponse],
) -> Vec<ObjectBrowserFileSummary> {
    files
        .iter()
        .map(|file| ObjectBrowserFileSummary {
            object_id: file.object_id.clone(),
            name: file.name.clone(),
            path: file.path.clone(),
            object_type: labelize_state(&file.object_type),
            size: format_browser_bytes(file.size_bytes),
            modified: file
                .modified_at_utc
                .as_deref()
                .unwrap_or("not recorded")
                .to_string(),
            readiness: labelize_state(&file.readiness),
            lifecycle: labelize_state(&file.lifecycle_state),
            copies: format!("{} copy/copies", file.copy_count),
            placement_summary: object_browser_placement_summary(&file.placements),
            placements: file.placements.clone(),
        })
        .collect()
}

#[cfg(any(target_arch = "wasm32", test))]
pub(super) fn format_browser_bytes(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    const GIB: f64 = MIB * 1024.0;
    const TIB: f64 = GIB * 1024.0;
    let bytes = bytes as f64;
    if bytes >= TIB {
        format!("{:.1} TiB", bytes / TIB)
    } else if bytes >= GIB {
        format!("{:.1} GiB", bytes / GIB)
    } else if bytes >= MIB {
        format!("{:.1} MiB", bytes / MIB)
    } else if bytes >= KIB {
        format!("{:.1} KiB", bytes / KIB)
    } else {
        format!("{bytes:.0} B")
    }
}

#[cfg(any(target_arch = "wasm32", test))]
pub(super) fn labelize_state(value: &str) -> String {
    let normalized = value.replace('-', "_");
    normalized
        .split('_')
        .filter(|part| !part.is_empty())
        .flat_map(split_camel_token)
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(any(target_arch = "wasm32", test))]
pub(super) fn split_camel_token(value: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    for character in value.chars() {
        if character.is_uppercase() && !current.is_empty() {
            words.push(titlecase_word(&current));
            current.clear();
        }
        current.push(character);
    }
    if !current.is_empty() {
        words.push(titlecase_word(&current));
    }
    words
}

#[cfg(any(target_arch = "wasm32", test))]
pub(super) fn titlecase_word(value: &str) -> String {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) => format!("{}{}", first.to_ascii_uppercase(), chars.as_str()),
        None => String::new(),
    }
}

#[cfg(any(target_arch = "wasm32", test))]
pub(super) fn object_browser_placement_summary(
    placements: &[ObjectBrowserPlacementResponse],
) -> String {
    if placements.is_empty() {
        return "placement pending".to_string();
    }
    let ssd = placements
        .iter()
        .filter(|placement| placement.location == "ssd_landing")
        .count();
    let hdd = placements
        .iter()
        .filter(|placement| placement.location == "hdd_settled")
        .count();
    let external = placements
        .iter()
        .filter(|placement| placement.location == "external_endpoint")
        .count();
    let degraded_or_missing = placements
        .iter()
        .filter(|placement| matches!(placement.state.as_str(), "degraded" | "missing"))
        .count();
    let pending = placements
        .iter()
        .filter(|placement| placement.state == "pending")
        .count();
    let verified_hdd = placements
        .iter()
        .filter(|placement| placement.location == "hdd_settled" && placement.state == "verified")
        .count();

    let mut parts = Vec::new();
    if ssd > 0 {
        parts.push(format!("{ssd} SSD landing"));
    }
    if hdd > 0 {
        parts.push(format!("{hdd} HDD settled"));
    }
    if external > 0 {
        parts.push(format!("{external} external endpoint"));
    }
    if verified_hdd > 1 {
        parts.push(format!("{verified_hdd} verified HDD copies"));
    }
    if degraded_or_missing > 0 {
        parts.push(format!("{degraded_or_missing} degraded/missing"));
    }
    if pending > 0 {
        parts.push(format!("{pending} pending"));
    }
    parts.join(" · ")
}

#[cfg(any(target_arch = "wasm32", test))]
pub(super) fn object_browser_state_key(value: &str) -> String {
    value.trim().to_ascii_lowercase().replace(' ', "_")
}

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Debug, Eq, PartialEq, Properties)]
pub struct HomeDashboardProps {
    pub api_base_path: String,
}
