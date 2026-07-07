//! Logical object type classification.

use serde::{Deserialize, Serialize};
use std::fmt::{self, Display};
use std::str::FromStr;

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ObjectType {
    #[default]
    Naive,
    Bam,
    Cram,
    Sam,
    Pod5,
    Fastq,
    Fasta,
    ReferenceGenome,
    EnaSra,
    Vcf,
    Bcf,
    Bed,
    GffGtf,
    CountMatrix,
    GeneExpressionMatrix,
    GenomeAssembly,
    TranscriptomeAssembly,
    AlignmentIndex,
    NanoporeRun,
    IlluminaRun,
    SingleCellFastq,
    AnnData,
}

impl ObjectType {
    pub const ALL: [Self; 22] = [
        Self::Naive,
        Self::Bam,
        Self::Cram,
        Self::Sam,
        Self::Pod5,
        Self::Fastq,
        Self::Fasta,
        Self::ReferenceGenome,
        Self::EnaSra,
        Self::Vcf,
        Self::Bcf,
        Self::Bed,
        Self::GffGtf,
        Self::CountMatrix,
        Self::GeneExpressionMatrix,
        Self::GenomeAssembly,
        Self::TranscriptomeAssembly,
        Self::AlignmentIndex,
        Self::NanoporeRun,
        Self::IlluminaRun,
        Self::SingleCellFastq,
        Self::AnnData,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Naive => "naive",
            Self::Bam => "bam",
            Self::Cram => "cram",
            Self::Sam => "sam",
            Self::Pod5 => "pod5",
            Self::Fastq => "fastq",
            Self::Fasta => "fasta",
            Self::ReferenceGenome => "reference_genome",
            Self::EnaSra => "ena_sra",
            Self::Vcf => "vcf",
            Self::Bcf => "bcf",
            Self::Bed => "bed",
            Self::GffGtf => "gff_gtf",
            Self::CountMatrix => "count_matrix",
            Self::GeneExpressionMatrix => "gene_expression_matrix",
            Self::GenomeAssembly => "genome_assembly",
            Self::TranscriptomeAssembly => "transcriptome_assembly",
            Self::AlignmentIndex => "alignment_index",
            Self::NanoporeRun => "nanopore_run",
            Self::IlluminaRun => "illumina_run",
            Self::SingleCellFastq => "single_cell_fastq",
            Self::AnnData => "ann_data",
        }
    }

    pub fn accepted_names() -> &'static str {
        "naive, bam, cram, sam, pod5, fastq, fasta, reference_genome, ena_sra, vcf, bcf, bed, gff_gtf, count_matrix, gene_expression_matrix, genome_assembly, transcriptome_assembly, alignment_index, nanopore_run, illumina_run, single_cell_fastq, ann_data"
    }
}

impl Display for ObjectType {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.name())
    }
}

impl FromStr for ObjectType {
    type Err = ObjectTypeParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let normalized = value.trim().to_ascii_lowercase().replace('-', "_");
        match normalized.as_str() {
            "naive" | "unknown" | "generic" | "folder" | "file" => Ok(Self::Naive),
            "bam" => Ok(Self::Bam),
            "cram" => Ok(Self::Cram),
            "sam" => Ok(Self::Sam),
            "pod5" => Ok(Self::Pod5),
            "fastq" | "fq" | "fastq_gz" | "fq_gz" => Ok(Self::Fastq),
            "fasta" | "fa" | "fna" | "faa" => Ok(Self::Fasta),
            "reference_genome" | "reference" | "ref_genome" => Ok(Self::ReferenceGenome),
            "ena_sra" | "ena" | "sra" | "ena_dataset" | "sra_dataset" => Ok(Self::EnaSra),
            "vcf" | "vcf_gz" => Ok(Self::Vcf),
            "bcf" => Ok(Self::Bcf),
            "bed" => Ok(Self::Bed),
            "gff_gtf" | "gff" | "gff3" | "gtf" => Ok(Self::GffGtf),
            "count_matrix" | "counts" | "expression_counts" => Ok(Self::CountMatrix),
            "gene_expression_matrix" | "expression_matrix" | "transcript_matrix" => {
                Ok(Self::GeneExpressionMatrix)
            }
            "genome_assembly" | "assembly" => Ok(Self::GenomeAssembly),
            "transcriptome_assembly" | "transcriptome" => Ok(Self::TranscriptomeAssembly),
            "alignment_index" | "bai" | "crai" | "csi" | "index" => Ok(Self::AlignmentIndex),
            "nanopore_run" | "ont_run" => Ok(Self::NanoporeRun),
            "illumina_run" | "illumina" => Ok(Self::IlluminaRun),
            "single_cell_fastq" | "scrna_fastq" | "single_cell_reads" => Ok(Self::SingleCellFastq),
            "ann_data" | "anndata" | "h5ad" => Ok(Self::AnnData),
            _ => Err(ObjectTypeParseError {
                value: value.to_string(),
            }),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObjectTypeParseError {
    value: String,
}

impl Display for ObjectTypeParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "unknown object type `{}`; expected one of: {}",
            self.value,
            ObjectType::accepted_names()
        )
    }
}

impl std::error::Error for ObjectTypeParseError {}

#[cfg(test)]
mod tests {
    use super::ObjectType;

    #[test]
    fn object_type_names_are_stable() {
        assert_eq!(ObjectType::Naive.name(), "naive");
        assert_eq!(ObjectType::Pod5.name(), "pod5");
        assert_eq!(ObjectType::EnaSra.name(), "ena_sra");
        assert_eq!(ObjectType::AnnData.name(), "ann_data");
    }

    #[test]
    fn parses_required_bioinformatics_types() {
        assert_eq!("bam".parse::<ObjectType>(), Ok(ObjectType::Bam));
        assert_eq!("pod5".parse::<ObjectType>(), Ok(ObjectType::Pod5));
        assert_eq!("fastq".parse::<ObjectType>(), Ok(ObjectType::Fastq));
        assert_eq!("sra".parse::<ObjectType>(), Ok(ObjectType::EnaSra));
    }

    #[test]
    fn serializes_as_snake_case() {
        let encoded = serde_json::to_string(&ObjectType::GeneExpressionMatrix)
            .expect("object type serializes");

        assert_eq!(encoded, "\"gene_expression_matrix\"");
    }
}
