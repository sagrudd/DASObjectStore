use dasobjectstore_core::health::HealthSignals;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DiskHealthReport {
    pub device_path: Option<String>,
    pub model_hint: Option<String>,
    pub serial_hint: Option<String>,
    pub smart_passed: Option<bool>,
    pub signals: HealthSignals,
    pub warnings: Vec<String>,
}
