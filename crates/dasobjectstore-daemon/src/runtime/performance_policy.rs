use serde::{Deserialize, Serialize};
use std::fmt::{self, Display};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub const AUTHORITATIVE_PERFORMANCE_DIR_NAME: &str = "performance";
pub const AUTHORITATIVE_PERFORMANCE_RECOMMENDATION_FILE_NAME: &str =
    "authoritative-recommendation.json";
pub const PERFORMANCE_RECOMMENDATION_SCHEMA: &str =
    "dasobjectstore.performance_test.recommendation.v1";

pub fn authoritative_performance_recommendation_path(state_dir: impl AsRef<Path>) -> PathBuf {
    state_dir
        .as_ref()
        .join(AUTHORITATIVE_PERFORMANCE_DIR_NAME)
        .join(AUTHORITATIVE_PERFORMANCE_RECOMMENDATION_FILE_NAME)
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AuthoritativeIngestPolicy {
    pub strategy: String,
    pub hdd_settlement_concurrency: usize,
    pub remote_upload_route: String,
    pub external_disk_route: String,
    pub nvme_source_route: String,
}

impl Default for AuthoritativeIngestPolicy {
    fn default() -> Self {
        Self {
            strategy: "ssd_hdd_pipeline".to_string(),
            hdd_settlement_concurrency: 1,
            remote_upload_route: "ssd_first".to_string(),
            external_disk_route: "ssd_first".to_string(),
            nvme_source_route: "ssd_hdd_pipeline".to_string(),
        }
    }
}

pub fn read_authoritative_ingest_policy(
    path: &Path,
) -> Result<Option<AuthoritativeIngestPolicy>, AuthoritativePerformancePolicyError> {
    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(AuthoritativePerformancePolicyError::Io(err)),
    };
    let artifact: PerformanceRecommendationArtifact =
        serde_json::from_str(&content).map_err(AuthoritativePerformancePolicyError::Json)?;
    artifact.to_ingest_policy()
}

#[derive(Debug)]
pub enum AuthoritativePerformancePolicyError {
    Io(io::Error),
    Json(serde_json::Error),
    UnsupportedSchema(String),
    MissingAuthoritativePolicy,
}

impl Display for AuthoritativePerformancePolicyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(
                formatter,
                "authoritative performance policy IO failed: {err}"
            ),
            Self::Json(err) => {
                write!(
                    formatter,
                    "authoritative performance policy JSON failed: {err}"
                )
            }
            Self::UnsupportedSchema(schema) => write!(
                formatter,
                "unsupported authoritative performance policy schema: {schema}"
            ),
            Self::MissingAuthoritativePolicy => formatter.write_str(
                "performance recommendation is not marked authoritative for daemon policy use",
            ),
        }
    }
}

impl std::error::Error for AuthoritativePerformancePolicyError {}

#[derive(Clone, Debug, Deserialize)]
struct PerformanceRecommendationArtifact {
    schema: String,
    recommendation: PerformanceRecommendationSection,
    #[serde(default)]
    daemon_policy: Option<PerformanceDaemonPolicySection>,
}

impl PerformanceRecommendationArtifact {
    fn to_ingest_policy(
        &self,
    ) -> Result<Option<AuthoritativeIngestPolicy>, AuthoritativePerformancePolicyError> {
        if self.schema != PERFORMANCE_RECOMMENDATION_SCHEMA {
            return Err(AuthoritativePerformancePolicyError::UnsupportedSchema(
                self.schema.clone(),
            ));
        }
        let daemon_policy = self
            .daemon_policy
            .as_ref()
            .ok_or(AuthoritativePerformancePolicyError::MissingAuthoritativePolicy)?;
        if !daemon_policy.authoritative {
            return Err(AuthoritativePerformancePolicyError::MissingAuthoritativePolicy);
        }
        let hdd_settlement = daemon_policy.ssd_hdd_settlement.as_ref();
        let source_routes = daemon_policy.source_routes.as_ref();
        let hdd_settlement_concurrency = hdd_settlement
            .and_then(|policy| policy.hdd_concurrency)
            .unwrap_or(self.recommendation.hdd_concurrency)
            .clamp(1, 32);
        Ok(Some(AuthoritativeIngestPolicy {
            strategy: self.recommendation.strategy.clone(),
            hdd_settlement_concurrency,
            remote_upload_route: source_routes
                .and_then(|routes| routes.remote_upload.as_ref())
                .cloned()
                .unwrap_or_else(|| "ssd_first".to_string()),
            external_disk_route: source_routes
                .and_then(|routes| routes.external_disk_ingress.as_ref())
                .cloned()
                .unwrap_or_else(|| "ssd_first".to_string()),
            nvme_source_route: source_routes
                .and_then(|routes| routes.nvme_source_ingress.as_ref())
                .cloned()
                .unwrap_or_else(|| self.recommendation.strategy.clone()),
        }))
    }
}

#[derive(Clone, Debug, Deserialize)]
struct PerformanceRecommendationSection {
    strategy: String,
    hdd_concurrency: usize,
}

#[derive(Clone, Debug, Deserialize)]
struct PerformanceDaemonPolicySection {
    #[serde(default)]
    authoritative: bool,
    #[serde(default)]
    source_routes: Option<PerformanceSourceRoutesSection>,
    #[serde(default)]
    ssd_hdd_settlement: Option<PerformanceSsdHddSettlementSection>,
}

#[derive(Clone, Debug, Deserialize)]
struct PerformanceSourceRoutesSection {
    #[serde(default)]
    remote_upload: Option<String>,
    #[serde(default)]
    external_disk_ingress: Option<String>,
    #[serde(default)]
    nvme_source_ingress: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
struct PerformanceSsdHddSettlementSection {
    #[serde(default)]
    hdd_concurrency: Option<usize>,
}

#[cfg(test)]
mod tests {
    use super::{
        authoritative_performance_recommendation_path, read_authoritative_ingest_policy,
        AuthoritativePerformancePolicyError, AUTHORITATIVE_PERFORMANCE_RECOMMENDATION_FILE_NAME,
    };
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn builds_authoritative_policy_path_under_state_dir() {
        assert_eq!(
            authoritative_performance_recommendation_path("/var/lib/dasobjectstore"),
            PathBuf::from("/var/lib/dasobjectstore")
                .join("performance")
                .join(AUTHORITATIVE_PERFORMANCE_RECOMMENDATION_FILE_NAME)
        );
    }

    #[test]
    fn reads_authoritative_ingest_policy_from_recommendation_json() {
        let root = temp_root("authoritative-policy");
        let path = root.join("policy.json");
        fs::create_dir_all(&root).expect("root");
        fs::write(
            &path,
            r#"{
              "schema": "dasobjectstore.performance_test.recommendation.v1",
              "recommendation": {
                "strategy": "direct_hdd_pipeline",
                "hdd_concurrency": 5
              },
              "daemon_policy": {
                "authoritative": true,
                "source_routes": {
                  "remote_upload": "ssd_first",
                  "external_disk_ingress": "ssd_first",
                  "nvme_source_ingress": "direct_hdd_pipeline"
                },
                "ssd_hdd_settlement": {
                  "hdd_concurrency": 4
                }
              }
            }"#,
        )
        .expect("policy file");

        let policy = read_authoritative_ingest_policy(&path)
            .expect("policy parses")
            .expect("policy exists");

        assert_eq!(policy.strategy, "direct_hdd_pipeline");
        assert_eq!(policy.hdd_settlement_concurrency, 4);
        assert_eq!(policy.remote_upload_route, "ssd_first");
        assert_eq!(policy.external_disk_route, "ssd_first");
        assert_eq!(policy.nvme_source_route, "direct_hdd_pipeline");
        cleanup(&root);
    }

    #[test]
    fn rejects_non_authoritative_recommendation_for_daemon_policy() {
        let root = temp_root("non-authoritative-policy");
        let path = root.join("policy.json");
        fs::create_dir_all(&root).expect("root");
        fs::write(
            &path,
            r#"{
              "schema": "dasobjectstore.performance_test.recommendation.v1",
              "recommendation": {
                "strategy": "ssd_hdd_pipeline",
                "hdd_concurrency": 2
              }
            }"#,
        )
        .expect("policy file");

        let err = read_authoritative_ingest_policy(&path).expect_err("not authoritative");

        assert!(matches!(
            err,
            AuthoritativePerformancePolicyError::MissingAuthoritativePolicy
        ));
        cleanup(&root);
    }

    fn temp_root(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "dasobjectstore-{label}-{}-{unique}",
            std::process::id()
        ))
    }

    fn cleanup(path: &Path) {
        let _ = fs::remove_dir_all(path);
    }
}
