pub mod audio;
pub mod cpu_power;
pub mod display;
pub mod gpu_power;
pub mod kernel_params;
pub mod network_power;
pub mod pci_power;
pub mod services;
pub mod sleep;
pub mod usb_power;

use serde::Serialize;

/// Severity of an audit finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub enum Severity {
    Info,
    Low,
    Medium,
    High,
}

/// A single audit finding.
#[derive(Debug, Clone, Serialize)]
pub struct Finding {
    pub severity: Severity,
    pub category: String,
    pub description: String,
    pub current_value: String,
    pub recommended_value: String,
    pub impact: String,
    /// The sysfs/config path this finding relates to
    pub path: Option<String>,
    /// Weight for scoring (0-10)
    pub weight: u32,
}

impl Finding {
    pub fn new(
        severity: Severity,
        category: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        Self {
            severity,
            category: category.into(),
            description: description.into(),
            current_value: String::new(),
            recommended_value: String::new(),
            impact: String::new(),
            path: None,
            weight: 0,
        }
    }

    pub fn current(mut self, value: impl Into<String>) -> Self {
        self.current_value = value.into();
        self
    }

    pub fn recommended(mut self, value: impl Into<String>) -> Self {
        self.recommended_value = value.into();
        self
    }

    pub fn impact(mut self, value: impl Into<String>) -> Self {
        self.impact = value.into();
        self
    }

    pub fn path(mut self, value: impl Into<String>) -> Self {
        self.path = Some(value.into());
        self
    }

    pub fn weight(mut self, value: u32) -> Self {
        self.weight = value;
        self
    }
}

/// Calculate audit score (0-100) from findings.
/// 100 = no issues, lower = more/worse issues.
pub fn calculate_score(findings: &[Finding]) -> u32 {
    if findings.is_empty() {
        return 100;
    }

    let total_weight: u32 = findings.iter().map(|f| f.weight).sum();
    let max_possible = findings.len() as u32 * 10; // max weight per finding

    if max_possible == 0 {
        return 100;
    }

    let penalty_ratio = total_weight as f64 / max_possible as f64;
    let score = (100.0 * (1.0 - penalty_ratio)).round() as u32;
    score.min(100)
}
