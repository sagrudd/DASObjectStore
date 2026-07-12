//! Product-facing workspace view models and bootstrap projections.

use super::*;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ProductHomeWorkspaceView {
    pub schema_version: String,
    pub health: ProductHealthSummaryView,
    pub capacity: ProductCapacitySummaryView,
    pub throughput_7d: ProductThroughputSummaryView,
    pub memory: ProductMemoryStressView,
    pub smart_warnings: Vec<ProductSmartWarningView>,
    pub warnings: Vec<DashboardWarning>,
}

impl ProductHomeWorkspaceView {
    pub fn bootstrap() -> Self {
        Self {
            schema_version: PRODUCT_WORKSPACES_SCHEMA_VERSION.to_string(),
            health: ProductHealthSummaryView {
                appliance_state: "bootstrap".to_string(),
                write_ready: false,
                drive_count: 0,
                mounted_enclosure_count: 0,
                object_store_count: 0,
                smart_warning_count: 0,
            },
            capacity: ProductCapacitySummaryView {
                total_bytes: 0,
                used_bytes: 0,
                available_bytes: 0,
                protected_used_bytes: 0,
            },
            throughput_7d: ProductThroughputSummaryView {
                ingress_bytes: 0,
                destage_bytes: 0,
                average_ingress_mib_s: None,
                peak_ingress_mib_s: None,
            },
            memory: ProductMemoryStressView {
                used_bytes: 0,
                total_bytes: 0,
                stress_percent: 0,
                pressure_state: "unknown".to_string(),
            },
            smart_warnings: Vec::new(),
            warnings: vec![DashboardWarning::new(
                "live_inventory_pending",
                "Live enclosure, SMART, capacity, throughput, and memory data are pending daemon inventory integration.",
            )],
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProductHealthSummaryView {
    pub appliance_state: String,
    pub write_ready: bool,
    pub drive_count: usize,
    pub mounted_enclosure_count: usize,
    pub object_store_count: usize,
    pub smart_warning_count: usize,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProductCapacitySummaryView {
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub available_bytes: u64,
    pub protected_used_bytes: u64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ProductThroughputSummaryView {
    pub ingress_bytes: u64,
    pub destage_bytes: u64,
    pub average_ingress_mib_s: Option<f64>,
    pub peak_ingress_mib_s: Option<f64>,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProductMemoryStressView {
    pub used_bytes: u64,
    pub total_bytes: u64,
    pub stress_percent: u8,
    pub pressure_state: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProductSmartWarningView {
    pub disk_id: String,
    pub enclosure_id: Option<String>,
    pub bay_label: Option<String>,
    pub warning: String,
    pub severity: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProductEnclosuresWorkspaceView {
    pub schema_version: String,
    pub administrator_actions_enabled: bool,
    pub add_enclosure: EnclosureAddWorkflowView,
    pub enclosures: Vec<ProductEnclosureCardView>,
    pub warnings: Vec<DashboardWarning>,
}

impl ProductEnclosuresWorkspaceView {
    pub fn bootstrap() -> Self {
        Self {
            schema_version: PRODUCT_WORKSPACES_SCHEMA_VERSION.to_string(),
            administrator_actions_enabled: false,
            add_enclosure: EnclosureAddWorkflowView::bootstrap(false),
            enclosures: Vec::new(),
            warnings: vec![DashboardWarning::new(
                "enclosure_inventory_pending",
                "Live DAS enclosure inventory is pending daemon-backed discovery.",
            )],
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct EnclosureAddWorkflowView {
    pub enabled: bool,
    pub requires_sudo_administrator: bool,
    pub steps: Vec<String>,
    pub blocked_reason: Option<String>,
}

impl EnclosureAddWorkflowView {
    fn bootstrap(administrator_actions_enabled: bool) -> Self {
        Self {
            enabled: administrator_actions_enabled,
            requires_sudo_administrator: true,
            steps: vec![
                "Detect supported DAS enclosure".to_string(),
                "Identify SSD landing media".to_string(),
                "Identify eligible HDD media".to_string(),
                "Review format and data-loss plan".to_string(),
                "Submit daemon preparation job".to_string(),
            ],
            blocked_reason: (!administrator_actions_enabled).then(|| {
                "Current user must have sudo-derived DASObjectStore administrator rights."
                    .to_string()
            }),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProductEnclosureCardView {
    pub enclosure_id: String,
    pub display_name: String,
    pub vendor: Option<String>,
    pub product: Option<String>,
    pub branding: String,
    pub topology: Option<String>,
    pub mounted: bool,
    pub ssd_count: usize,
    pub hdd_count: usize,
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub available_bytes: u64,
    pub health_state: String,
    pub drives: Vec<ProductEnclosureDriveView>,
    pub warnings: Vec<DashboardWarning>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProductEnclosureDriveView {
    pub disk_id: String,
    pub role: String,
    pub device_path: Option<String>,
    pub bay_label: Option<String>,
    pub capacity_bytes: u64,
    pub used_bytes: u64,
    pub health_state: String,
    pub smart_warning_count: u16,
    pub mounted_path: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProductObjectStoresWorkspaceView {
    pub schema_version: String,
    pub administrator_actions_enabled: bool,
    pub groups_file_path: String,
    pub groups: Vec<StorageGroupView>,
    pub create: ObjectStoreCreateWorkflowView,
    pub object_stores: Vec<ProductObjectStoreCardView>,
    pub warnings: Vec<DashboardWarning>,
}

impl ProductObjectStoresWorkspaceView {
    pub fn bootstrap() -> Self {
        let administrator_actions_enabled = false;
        Self {
            schema_version: PRODUCT_WORKSPACES_SCHEMA_VERSION.to_string(),
            administrator_actions_enabled,
            groups_file_path: "/opt/dasobjectstore/groups.json".to_string(),
            groups: Vec::new(),
            create: ObjectStoreCreateWorkflowView::bootstrap(administrator_actions_enabled),
            object_stores: Vec::new(),
            warnings: vec![DashboardWarning::new(
                "object_store_inventory_pending",
                "Live object-store inventory and group policy are pending daemon-backed discovery.",
            )],
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ObjectStoreCreateWorkflowView {
    pub enabled: bool,
    pub requires_sudo_administrator: bool,
    pub supported_store_types: Vec<String>,
    pub supported_redundancy: Vec<u8>,
    pub required_fields: Vec<String>,
    pub blocked_reason: Option<String>,
}

impl ObjectStoreCreateWorkflowView {
    fn bootstrap(administrator_actions_enabled: bool) -> Self {
        Self {
            enabled: administrator_actions_enabled,
            requires_sudo_administrator: true,
            supported_store_types: vec![
                "naive".to_string(),
                "bam".to_string(),
                "pod5".to_string(),
                "fastq".to_string(),
                "ena_sra".to_string(),
            ],
            supported_redundancy: vec![1, 2, 3],
            required_fields: vec![
                "store name".to_string(),
                "writer group".to_string(),
                "enclosure".to_string(),
                "object type".to_string(),
                "redundancy".to_string(),
            ],
            blocked_reason: (!administrator_actions_enabled).then(|| {
                "Current user must have sudo-derived DASObjectStore administrator rights."
                    .to_string()
            }),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProductObjectStoreCardView {
    pub store_id: String,
    pub display_name: String,
    pub writer_group: String,
    pub enclosure_id: String,
    pub object_type: String,
    pub redundancy: u8,
    pub public: bool,
    pub writeable: bool,
    pub used_bytes: u64,
    pub object_count: usize,
    pub warnings: Vec<DashboardWarning>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProductBioinformaticsWorkspaceView {
    pub schema_version: String,
    pub available: bool,
    pub supported_object_types: Vec<String>,
    pub readiness_cards: Vec<ProductBioinformaticsReadinessCardView>,
    pub derivation_sources: Vec<ProductBioinformaticsDerivationSourceView>,
    pub sequencing_runs: Vec<ProductBioinformaticsContextCardView>,
    pub object_lineage: Vec<ProductBioinformaticsContextCardView>,
    pub workflow_handoffs: Vec<ProductBioinformaticsContextCardView>,
    pub governance_bindings: Vec<ProductBioinformaticsContextCardView>,
    pub message: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProductBioinformaticsReadinessCardView {
    pub object_type: String,
    pub label: String,
    pub category: String,
    pub state: String,
    pub primary_workflow: String,
    pub handoff: String,
    pub required_metadata: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProductBioinformaticsContextCardView {
    pub label: String,
    pub state: String,
    pub summary: String,
    pub detail: String,
    pub evidence: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProductBioinformaticsDerivationSourceView {
    pub source_kind: String,
    pub source_id: String,
    pub display_name: String,
    pub object_type: String,
    pub parent_id: Option<String>,
    pub endpoint_export_mode: Option<String>,
    pub mneion_binding_state: String,
    pub governance_domain: Option<String>,
    pub workflow_roles: Vec<String>,
    pub evidence: Vec<String>,
}

impl ProductBioinformaticsWorkspaceView {
    pub fn bootstrap() -> Self {
        let readiness_cards = vec![
            ProductBioinformaticsReadinessCardView {
                object_type: "bam".to_string(),
                label: "BAM".to_string(),
                category: "Alignment".to_string(),
                state: "workflow_ready".to_string(),
                primary_workflow: "Genome alignment inspection, variant calling, coverage, and QC handoff.".to_string(),
                handoff: "Genome/transcriptome analysis".to_string(),
                required_metadata: vec![
                    "reference genome".to_string(),
                    "sample or run identity".to_string(),
                    "index readiness".to_string(),
                ],
            },
            ProductBioinformaticsReadinessCardView {
                object_type: "cram".to_string(),
                label: "CRAM".to_string(),
                category: "Compressed alignment".to_string(),
                state: "metadata_required".to_string(),
                primary_workflow: "Reference-backed alignment analysis with storage-efficient archive handling.".to_string(),
                handoff: "Genome analysis with reference binding".to_string(),
                required_metadata: vec![
                    "reference genome".to_string(),
                    "reference checksum".to_string(),
                    "index readiness".to_string(),
                ],
            },
            ProductBioinformaticsReadinessCardView {
                object_type: "pod5".to_string(),
                label: "POD5".to_string(),
                category: "Nanopore signal".to_string(),
                state: "workflow_ready".to_string(),
                primary_workflow: "Basecalling, run QC, methylation-aware analysis, and signal-level provenance.".to_string(),
                handoff: "Basecalling readiness".to_string(),
                required_metadata: vec![
                    "flowcell/run identity".to_string(),
                    "sequencing kit".to_string(),
                    "sample sheet".to_string(),
                ],
            },
            ProductBioinformaticsReadinessCardView {
                object_type: "fastq".to_string(),
                label: "FASTQ / FASTQ.GZ".to_string(),
                category: "Reads".to_string(),
                state: "workflow_ready".to_string(),
                primary_workflow: "Read QC, alignment, assembly, taxonomic profiling, and transcriptome quantification.".to_string(),
                handoff: "Genome/transcriptome workflows".to_string(),
                required_metadata: vec![
                    "sample identity".to_string(),
                    "library strategy".to_string(),
                    "paired-end or single-end state".to_string(),
                ],
            },
            ProductBioinformaticsReadinessCardView {
                object_type: "fasta".to_string(),
                label: "FASTA".to_string(),
                category: "Reference or assembly".to_string(),
                state: "workflow_ready".to_string(),
                primary_workflow: "Reference registration, assembly handoff, indexing, and annotation workflows.".to_string(),
                handoff: "Reference and assembly workflows".to_string(),
                required_metadata: vec![
                    "organism or build label".to_string(),
                    "source/version".to_string(),
                    "index readiness".to_string(),
                ],
            },
            ProductBioinformaticsReadinessCardView {
                object_type: "vcf_bcf".to_string(),
                label: "VCF / BCF".to_string(),
                category: "Variants".to_string(),
                state: "workflow_ready".to_string(),
                primary_workflow: "Variant filtering, cohort comparison, annotation, and export.".to_string(),
                handoff: "Variant analysis".to_string(),
                required_metadata: vec![
                    "reference genome".to_string(),
                    "sample or cohort identity".to_string(),
                    "index readiness".to_string(),
                ],
            },
            ProductBioinformaticsReadinessCardView {
                object_type: "gff_gtf".to_string(),
                label: "GFF / GTF".to_string(),
                category: "Annotation".to_string(),
                state: "workflow_ready".to_string(),
                primary_workflow: "Genome annotation, transcript feature import, and quantification support.".to_string(),
                handoff: "Annotation and transcriptome workflows".to_string(),
                required_metadata: vec![
                    "reference build".to_string(),
                    "annotation source".to_string(),
                    "feature vocabulary".to_string(),
                ],
            },
            ProductBioinformaticsReadinessCardView {
                object_type: "ena_sra".to_string(),
                label: "ENA / SRA".to_string(),
                category: "Public repository dataset".to_string(),
                state: "catalogue_ready".to_string(),
                primary_workflow: "Public sequence dataset staging, accession tracking, reproducibility, and downstream ingest.".to_string(),
                handoff: "Repository accession workflows".to_string(),
                required_metadata: vec![
                    "accession".to_string(),
                    "study/project identity".to_string(),
                    "download manifest".to_string(),
                ],
            },
        ];
        Self {
            schema_version: PRODUCT_WORKSPACES_SCHEMA_VERSION.to_string(),
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
            readiness_cards,
            derivation_sources: vec![
                ProductBioinformaticsDerivationSourceView {
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
                    evidence: vec![
                        "ObjectStore object_type assignment".to_string(),
                        "ObjectStore endpoint export mode".to_string(),
                    ],
                },
                ProductBioinformaticsDerivationSourceView {
                    source_kind: "subobject_metadata".to_string(),
                    source_id: "contract-subobject-lineage".to_string(),
                    display_name: "SubObject lineage and object-type policy".to_string(),
                    object_type: "fastq".to_string(),
                    parent_id: Some("contract-object-store-object-type".to_string()),
                    endpoint_export_mode: Some("dedicated_prefix".to_string()),
                    mneion_binding_state: "binding_required".to_string(),
                    governance_domain: None,
                    workflow_roles: vec![
                        "object_lineage".to_string(),
                        "genome_transcriptome_handoff".to_string(),
                    ],
                    evidence: vec![
                        "SubObject parent relationship".to_string(),
                        "SubObject object_type override or inheritance".to_string(),
                    ],
                },
                ProductBioinformaticsDerivationSourceView {
                    source_kind: "mneion_binding".to_string(),
                    source_id: "contract-mneion-governance-binding".to_string(),
                    display_name: "Mneion governance-domain binding".to_string(),
                    object_type: "mixed".to_string(),
                    parent_id: None,
                    endpoint_export_mode: None,
                    mneion_binding_state: "binding_required".to_string(),
                    governance_domain: Some("unassigned".to_string()),
                    workflow_roles: vec![
                        "governance_binding".to_string(),
                        "audit_context".to_string(),
                    ],
                    evidence: vec![
                        "Endpoint inventory active binding".to_string(),
                        "Mneion storage definition".to_string(),
                    ],
                },
            ],
            sequencing_runs: vec![ProductBioinformaticsContextCardView {
                label: "Sequencing run provenance".to_string(),
                state: "metadata_required".to_string(),
                summary: "Run-level provenance is ready to bind once ObjectStore metadata exposes run identifiers.".to_string(),
                detail: "POD5, FASTQ, Nanopore run, and Illumina run objects should provide sample, instrument, flowcell or lane, kit, and acquisition timestamps before workflow dispatch.".to_string(),
                evidence: vec![
                    "POD5 basecalling readiness".to_string(),
                    "FASTQ sample/library metadata".to_string(),
                    "Nanopore and Illumina run object types".to_string(),
                ],
            }],
            object_lineage: vec![ProductBioinformaticsContextCardView {
                label: "Object lineage".to_string(),
                state: "planned".to_string(),
                summary: "Lineage will connect raw signal, reads, alignments, variants, references, and annotations.".to_string(),
                detail: "The Web console exposes the lineage surface now; the next API slice will derive parent/child state from ObjectStore and SubObject metadata rather than hard-coded workflow paths.".to_string(),
                evidence: vec![
                    "raw signal to basecalled reads".to_string(),
                    "reads to alignments".to_string(),
                    "references and annotations to downstream results".to_string(),
                ],
            }],
            workflow_handoffs: vec![
                ProductBioinformaticsContextCardView {
                    label: "Basecalling handoff".to_string(),
                    state: "workflow_ready".to_string(),
                    summary: "POD5 objects can advertise basecalling readiness and required run metadata.".to_string(),
                    detail: "Basecalling handoff should use daemon-owned ObjectStore metadata and Mnemosyne governance bindings before launching work.".to_string(),
                    evidence: vec![
                        "POD5 readiness cards".to_string(),
                        "sequencing kit metadata".to_string(),
                    ],
                },
                ProductBioinformaticsContextCardView {
                    label: "Genome/transcriptome handoff".to_string(),
                    state: "metadata_required".to_string(),
                    summary: "FASTQ, BAM/CRAM, FASTA, GFF/GTF, and VCF/BCF cards expose the metadata needed for analysis handoff.".to_string(),
                    detail: "Reference build, index, sample/cohort, library strategy, and annotation source bindings must be resolved before automatic orchestration.".to_string(),
                    evidence: vec![
                        "reference genome metadata".to_string(),
                        "alignment and variant object families".to_string(),
                        "annotation object families".to_string(),
                    ],
                },
            ],
            governance_bindings: vec![ProductBioinformaticsContextCardView {
                label: "Mnemosyne governance binding".to_string(),
                state: "binding_required".to_string(),
                summary: "Project and governance-domain bindings are represented as a first-class Bioinformatics view.".to_string(),
                detail: "Mneion/Mnemosyne project, governance domain, endpoint identity, and writer-policy membership must be attached before data orchestration is considered auditable.".to_string(),
                evidence: vec![
                    "endpoint inventory bindings".to_string(),
                    "writer group policy".to_string(),
                    "Mneion storage definitions".to_string(),
                ],
            }],
            message: "Bioinformatics readiness cards classify supported object types and the workflow handoff metadata needed for orchestration.".to_string(),
        }
    }
}
