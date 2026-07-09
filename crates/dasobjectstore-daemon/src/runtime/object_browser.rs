//! Metadata-backed ObjectStore browser query helpers.

use crate::api::{
    DaemonRequestValidationError, ObjectBrowserBreadcrumb, ObjectBrowserChecksum,
    ObjectBrowserFileNode, ObjectBrowserFolderNode, ObjectBrowserPlacement,
    ObjectBrowserPlacementLocation, ObjectBrowserPlacementState, ObjectBrowserReadinessState,
    ObjectBrowserRequest, ObjectBrowserResponse, ObjectBrowserSort,
};
use dasobjectstore_core::ids::ObjectId;
use dasobjectstore_core::lifecycle::ObjectState;
use dasobjectstore_core::object_type::ObjectType;
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::fmt::{self, Display};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObjectBrowserMetadataEntry {
    pub object_id: ObjectId,
    pub path: String,
    pub object_type: ObjectType,
    pub size_bytes: u64,
    pub modified_at_utc: Option<String>,
    pub checksum: Option<ObjectBrowserChecksum>,
    pub lifecycle_state: ObjectState,
    pub placements: Vec<ObjectBrowserPlacement>,
}

pub fn query_object_browser_metadata(
    request: &ObjectBrowserRequest,
    entries: &[ObjectBrowserMetadataEntry],
) -> Result<ObjectBrowserResponse, ObjectBrowserQueryError> {
    request.validate()?;
    let prefix = normalize_prefix(request.prefix.as_deref());
    let search = request
        .search
        .as_deref()
        .map(|value| value.trim().to_ascii_lowercase());
    let offset = parse_cursor(request.page.cursor.as_deref())?;

    let mut folders = BTreeMap::<String, FolderAccumulator>::new();
    let mut files = Vec::new();

    for entry in entries {
        let path = normalize_path(&entry.path);
        let Some(remainder) = path_remainder(&path, &prefix) else {
            continue;
        };
        if remainder.is_empty() || !matches_search(&path, search.as_deref()) {
            continue;
        }

        if let Some((folder_name, _)) = remainder.split_once('/') {
            let folder_prefix = join_prefix(&prefix, folder_name);
            folders
                .entry(folder_prefix.clone())
                .or_insert_with(|| FolderAccumulator::new(folder_name, folder_prefix))
                .add(entry);
        } else {
            files.push(file_node(entry, path, request.include_placement));
        }
    }

    let mut nodes = folders
        .into_values()
        .map(|folder| BrowserNode::Folder(folder.finish()))
        .chain(files.into_iter().map(BrowserNode::File))
        .collect::<Vec<_>>();
    nodes.sort_by(|left, right| compare_nodes(left, right, request.sort));

    let total_entries = nodes.len() as u64;
    let end = offset
        .saturating_add(request.page.limit as usize)
        .min(nodes.len());
    let selected = nodes.get(offset..end).unwrap_or_default();
    let next_cursor = (end < nodes.len()).then(|| end.to_string());

    let mut response_folders = Vec::new();
    let mut response_files = Vec::new();
    for node in selected {
        match node {
            BrowserNode::Folder(folder) => response_folders.push(folder.clone()),
            BrowserNode::File(file) => response_files.push(file.clone()),
        }
    }

    Ok(ObjectBrowserResponse {
        endpoint: request.endpoint.clone(),
        prefix: prefix.clone(),
        breadcrumbs: breadcrumbs(&prefix),
        folders: response_folders,
        files: response_files,
        next_cursor,
        total_entries: Some(total_entries),
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ObjectBrowserQueryError {
    InvalidRequest(DaemonRequestValidationError),
    InvalidCursor { cursor: String },
}

impl Display for ObjectBrowserQueryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRequest(err) => err.fmt(formatter),
            Self::InvalidCursor { cursor } => {
                write!(
                    formatter,
                    "object browser cursor must be a byte offset: {cursor}"
                )
            }
        }
    }
}

impl std::error::Error for ObjectBrowserQueryError {}

impl From<DaemonRequestValidationError> for ObjectBrowserQueryError {
    fn from(err: DaemonRequestValidationError) -> Self {
        Self::InvalidRequest(err)
    }
}

#[derive(Clone)]
enum BrowserNode {
    Folder(ObjectBrowserFolderNode),
    File(ObjectBrowserFileNode),
}

impl BrowserNode {
    fn name(&self) -> &str {
        match self {
            Self::Folder(folder) => &folder.name,
            Self::File(file) => &file.name,
        }
    }

    fn size_bytes(&self) -> u64 {
        match self {
            Self::Folder(folder) => folder.total_size_bytes.unwrap_or_default(),
            Self::File(file) => file.size_bytes,
        }
    }

    fn modified_at_utc(&self) -> Option<&str> {
        match self {
            Self::Folder(_) => None,
            Self::File(file) => file.modified_at_utc.as_deref(),
        }
    }

    fn kind_rank(&self) -> u8 {
        match self {
            Self::Folder(_) => 0,
            Self::File(_) => 1,
        }
    }
}

struct FolderAccumulator {
    node: ObjectBrowserFolderNode,
}

impl FolderAccumulator {
    fn new(name: &str, prefix: String) -> Self {
        Self {
            node: ObjectBrowserFolderNode {
                name: name.to_string(),
                prefix,
                object_count: Some(0),
                total_size_bytes: Some(0),
                readiness: ObjectBrowserReadinessState::Available,
            },
        }
    }

    fn add(&mut self, entry: &ObjectBrowserMetadataEntry) {
        self.node.object_count = Some(self.node.object_count.unwrap_or_default() + 1);
        self.node.total_size_bytes =
            Some(self.node.total_size_bytes.unwrap_or_default() + entry.size_bytes);
        self.node.readiness = worst_readiness(
            self.node.readiness,
            readiness_for(entry.lifecycle_state, &entry.placements),
        );
    }

    fn finish(self) -> ObjectBrowserFolderNode {
        self.node
    }
}

fn file_node(
    entry: &ObjectBrowserMetadataEntry,
    path: String,
    include_placement: bool,
) -> ObjectBrowserFileNode {
    let copy_count = settled_copy_count(&entry.placements);
    ObjectBrowserFileNode {
        object_id: entry.object_id.clone(),
        name: file_name(&path).to_string(),
        path,
        object_type: entry.object_type,
        size_bytes: entry.size_bytes,
        modified_at_utc: entry.modified_at_utc.clone(),
        checksum: entry.checksum.clone(),
        readiness: readiness_for(entry.lifecycle_state, &entry.placements),
        lifecycle_state: entry.lifecycle_state,
        copy_count,
        placements: include_placement
            .then(|| entry.placements.clone())
            .unwrap_or_default(),
    }
}

fn readiness_for(
    lifecycle_state: ObjectState,
    placements: &[ObjectBrowserPlacement],
) -> ObjectBrowserReadinessState {
    if lifecycle_state == ObjectState::RedownloadRequired {
        return ObjectBrowserReadinessState::RedownloadRequired;
    }
    if placements
        .iter()
        .any(|placement| matches!(placement.state, ObjectBrowserPlacementState::Missing))
    {
        return ObjectBrowserReadinessState::Unavailable;
    }
    if placements
        .iter()
        .any(|placement| matches!(placement.state, ObjectBrowserPlacementState::Degraded))
    {
        return ObjectBrowserReadinessState::Degraded;
    }
    if settled_copy_count(placements) > 0 {
        return ObjectBrowserReadinessState::Available;
    }
    if placements.iter().any(|placement| {
        matches!(
            placement.location,
            ObjectBrowserPlacementLocation::SsdLanding
        )
    }) {
        return ObjectBrowserReadinessState::SsdOnly;
    }
    match lifecycle_state {
        ObjectState::ReceivedOnSsd | ObjectState::HashVerified => {
            ObjectBrowserReadinessState::SsdOnly
        }
        ObjectState::PlacementPlanned
        | ObjectState::CopyingToHdd
        | ObjectState::HddCopyVerified
        | ObjectState::Protected
        | ObjectState::SsdEvictionEligible => ObjectBrowserReadinessState::Settling,
        ObjectState::RedownloadRequired => ObjectBrowserReadinessState::RedownloadRequired,
    }
}

fn settled_copy_count(placements: &[ObjectBrowserPlacement]) -> u16 {
    placements
        .iter()
        .filter(|placement| {
            placement.location == ObjectBrowserPlacementLocation::HddSettled
                && placement.state == ObjectBrowserPlacementState::Verified
        })
        .count()
        .min(u16::MAX as usize) as u16
}

fn worst_readiness(
    left: ObjectBrowserReadinessState,
    right: ObjectBrowserReadinessState,
) -> ObjectBrowserReadinessState {
    if readiness_rank(left) >= readiness_rank(right) {
        left
    } else {
        right
    }
}

fn readiness_rank(readiness: ObjectBrowserReadinessState) -> u8 {
    match readiness {
        ObjectBrowserReadinessState::Available => 0,
        ObjectBrowserReadinessState::SsdOnly => 1,
        ObjectBrowserReadinessState::Settling => 2,
        ObjectBrowserReadinessState::Degraded => 3,
        ObjectBrowserReadinessState::RedownloadRequired => 4,
        ObjectBrowserReadinessState::Unavailable => 5,
    }
}

fn compare_nodes(left: &BrowserNode, right: &BrowserNode, sort: ObjectBrowserSort) -> Ordering {
    let primary = match sort {
        ObjectBrowserSort::NameAsc => compare_name(left, right),
        ObjectBrowserSort::NameDesc => compare_name(right, left),
        ObjectBrowserSort::SizeAsc => left.size_bytes().cmp(&right.size_bytes()),
        ObjectBrowserSort::SizeDesc => right.size_bytes().cmp(&left.size_bytes()),
        ObjectBrowserSort::ModifiedAsc => compare_modified(left, right),
        ObjectBrowserSort::ModifiedDesc => compare_modified(right, left),
    };
    primary
        .then_with(|| left.kind_rank().cmp(&right.kind_rank()))
        .then_with(|| compare_name(left, right))
}

fn compare_name(left: &BrowserNode, right: &BrowserNode) -> Ordering {
    left.name()
        .to_ascii_lowercase()
        .cmp(&right.name().to_ascii_lowercase())
}

fn compare_modified(left: &BrowserNode, right: &BrowserNode) -> Ordering {
    match (left.modified_at_utc(), right.modified_at_utc()) {
        (Some(left), Some(right)) => left.cmp(right),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

fn breadcrumbs(prefix: &str) -> Vec<ObjectBrowserBreadcrumb> {
    let mut current = String::new();
    prefix
        .split('/')
        .filter(|part| !part.is_empty())
        .map(|part| {
            current = join_prefix(&current, part);
            ObjectBrowserBreadcrumb {
                name: part.to_string(),
                prefix: current.clone(),
            }
        })
        .collect()
}

fn path_remainder<'a>(path: &'a str, prefix: &str) -> Option<&'a str> {
    if prefix.is_empty() {
        return Some(path);
    }
    if path == prefix {
        return Some("");
    }
    path.strip_prefix(prefix)?.strip_prefix('/')
}

fn matches_search(path: &str, search: Option<&str>) -> bool {
    search.is_none_or(|search| path.to_ascii_lowercase().contains(search))
}

fn parse_cursor(cursor: Option<&str>) -> Result<usize, ObjectBrowserQueryError> {
    cursor
        .map(|cursor| {
            cursor
                .parse::<usize>()
                .map_err(|_| ObjectBrowserQueryError::InvalidCursor {
                    cursor: cursor.to_string(),
                })
        })
        .transpose()
        .map(|cursor| cursor.unwrap_or_default())
}

fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn join_prefix(prefix: &str, name: &str) -> String {
    if prefix.is_empty() {
        name.to_string()
    } else {
        format!("{prefix}/{name}")
    }
}

fn normalize_prefix(prefix: Option<&str>) -> String {
    prefix.map(normalize_path).unwrap_or_default()
}

fn normalize_path(path: &str) -> String {
    path.split('/')
        .filter(|part| !part.trim().is_empty())
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use super::{query_object_browser_metadata, ObjectBrowserMetadataEntry};
    use crate::api::{
        ObjectBrowserChecksum, ObjectBrowserPageRequest, ObjectBrowserPlacement,
        ObjectBrowserPlacementLocation, ObjectBrowserPlacementState, ObjectBrowserReadinessState,
        ObjectBrowserRequest, ObjectBrowserSort,
    };
    use dasobjectstore_core::ids::{DiskId, ObjectId, StoreId};
    use dasobjectstore_core::lifecycle::ObjectState;
    use dasobjectstore_core::object_type::ObjectType;

    #[test]
    fn browses_immediate_prefix_children_with_breadcrumbs() {
        let entries = vec![
            entry("ENA/Xenognostikon/Vervet/SRR001.fastq.gz", 100),
            entry("ENA/Xenognostikon/Vervet/SRR002.fastq.gz", 200),
            entry("ENA/Xenognostikon/metadata.tsv", 10),
            entry("ENA/OtherStudy/readme.txt", 5),
        ];
        let request = request("ENA/Xenognostikon", ObjectBrowserSort::NameAsc, 100, None);

        let response = query_object_browser_metadata(&request, &entries).expect("query succeeds");

        assert_eq!(response.prefix, "ENA/Xenognostikon");
        assert_eq!(
            response
                .breadcrumbs
                .iter()
                .map(|breadcrumb| breadcrumb.prefix.as_str())
                .collect::<Vec<_>>(),
            ["ENA", "ENA/Xenognostikon"]
        );
        assert_eq!(response.folders.len(), 1);
        assert_eq!(response.folders[0].name, "Vervet");
        assert_eq!(response.folders[0].object_count, Some(2));
        assert_eq!(response.folders[0].total_size_bytes, Some(300));
        assert_eq!(response.files.len(), 1);
        assert_eq!(response.files[0].name, "metadata.tsv");
        assert!(response.next_cursor.is_none());
        assert_eq!(response.total_entries, Some(2));
    }

    #[test]
    fn searches_sorts_and_bounds_large_tree_pages() {
        let entries = (0..1_200)
            .map(|index| {
                entry(
                    &format!("runs/study-{index:04}/sample-{index:04}.fastq.gz"),
                    index as u64,
                )
            })
            .collect::<Vec<_>>();
        let request = ObjectBrowserRequest {
            search: Some("study-11".to_string()),
            page: ObjectBrowserPageRequest {
                cursor: None,
                limit: 25,
            },
            ..request("runs", ObjectBrowserSort::NameDesc, 25, None)
        };

        let response = query_object_browser_metadata(&request, &entries).expect("query succeeds");

        assert_eq!(response.folders.len(), 25);
        assert!(response.files.is_empty());
        assert_eq!(response.folders[0].name, "study-1199");
        assert_eq!(response.folders[24].name, "study-1175");
        assert_eq!(response.next_cursor, Some("25".to_string()));
        assert_eq!(response.total_entries, Some(100));
    }

    #[test]
    fn supports_cursor_offsets_across_combined_folder_and_file_results() {
        let entries = vec![
            entry("alpha/a.txt", 1),
            entry("beta/b.txt", 2),
            entry("root.txt", 3),
        ];
        let request = ObjectBrowserRequest {
            page: ObjectBrowserPageRequest {
                cursor: Some("1".to_string()),
                limit: 2,
            },
            ..request("", ObjectBrowserSort::NameAsc, 2, None)
        };

        let response = query_object_browser_metadata(&request, &entries).expect("query succeeds");

        assert_eq!(response.folders.len(), 1);
        assert_eq!(response.folders[0].name, "beta");
        assert_eq!(response.files.len(), 1);
        assert_eq!(response.files[0].name, "root.txt");
        assert!(response.next_cursor.is_none());
        assert_eq!(response.total_entries, Some(3));
    }

    #[test]
    fn derives_readiness_and_hides_placements_when_not_requested() {
        let mut ssd_only = entry("active/on-ssd.dat", 10);
        ssd_only.lifecycle_state = ObjectState::HashVerified;
        ssd_only.placements = vec![ObjectBrowserPlacement {
            disk_id: None,
            disk_label: Some("landing SSD".to_string()),
            location: ObjectBrowserPlacementLocation::SsdLanding,
            state: ObjectBrowserPlacementState::Verified,
            size_bytes: 10,
            checksum: None,
            verified_at_utc: None,
        }];
        let mut degraded = entry("active/degraded.dat", 20);
        degraded.placements[0].state = ObjectBrowserPlacementState::Degraded;
        let request = request("active", ObjectBrowserSort::NameAsc, 10, None);

        let response =
            query_object_browser_metadata(&request, &[ssd_only, degraded]).expect("query succeeds");

        assert_eq!(response.files[0].name, "degraded.dat");
        assert_eq!(
            response.files[0].readiness,
            ObjectBrowserReadinessState::Degraded
        );
        assert!(response.files[0].placements.is_empty());
        assert_eq!(response.files[1].name, "on-ssd.dat");
        assert_eq!(
            response.files[1].readiness,
            ObjectBrowserReadinessState::SsdOnly
        );
    }

    #[test]
    fn rejects_invalid_cursor_values() {
        let request = ObjectBrowserRequest {
            page: ObjectBrowserPageRequest {
                cursor: Some("not-an-offset".to_string()),
                limit: 10,
            },
            ..request("", ObjectBrowserSort::NameAsc, 10, None)
        };

        let err = query_object_browser_metadata(&request, &[]).expect_err("cursor is invalid");

        assert!(err.to_string().contains("not-an-offset"));
    }

    fn request(
        prefix: &str,
        sort: ObjectBrowserSort,
        limit: u16,
        search: Option<&str>,
    ) -> ObjectBrowserRequest {
        ObjectBrowserRequest {
            endpoint: StoreId::new("ena").expect("store id"),
            prefix: (!prefix.is_empty()).then(|| prefix.to_string()),
            search: search.map(str::to_string),
            sort,
            page: ObjectBrowserPageRequest {
                cursor: None,
                limit,
            },
            include_placement: false,
        }
    }

    fn entry(path: &str, size_bytes: u64) -> ObjectBrowserMetadataEntry {
        ObjectBrowserMetadataEntry {
            object_id: ObjectId::new(path).expect("object id"),
            path: path.to_string(),
            object_type: ObjectType::Fastq,
            size_bytes,
            modified_at_utc: Some(format!("2026-07-09T09:{:02}:00Z", size_bytes % 60)),
            checksum: Some(ObjectBrowserChecksum {
                algorithm: "sha256".to_string(),
                value: format!("checksum-{size_bytes}"),
                verified_at_utc: None,
            }),
            lifecycle_state: ObjectState::Protected,
            placements: vec![ObjectBrowserPlacement {
                disk_id: Some(DiskId::new("qnap-1057").expect("disk id")),
                disk_label: Some("QNAP bay 1".to_string()),
                location: ObjectBrowserPlacementLocation::HddSettled,
                state: ObjectBrowserPlacementState::Verified,
                size_bytes,
                checksum: None,
                verified_at_utc: Some("2026-07-09T09:00:00Z".to_string()),
            }],
        }
    }
}
