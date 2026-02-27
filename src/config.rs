use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Top-level bop configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct BopConfig {
    pub auto: AutoConfig,
    pub epp: EppConfig,
    pub brightness: BrightnessConfig,
    pub inhibitors: InhibitorConfig,
    pub notifications: NotificationConfig,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AutoConfig {
    /// Enable aggressive optimizations in auto mode.
    pub aggressive: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct EppConfig {
    /// When true, select EPP based on battery percentage thresholds.
    pub adaptive: bool,
    /// Battery percentage → EPP mapping, sorted ascending by battery_percent.
    pub thresholds: Vec<EppThreshold>,
}

impl Default for EppConfig {
    fn default() -> Self {
        Self {
            adaptive: false,
            thresholds: vec![
                EppThreshold {
                    battery_percent: 20,
                    epp_value: EppHint::Power,
                },
                EppThreshold {
                    battery_percent: 50,
                    epp_value: EppHint::BalancePower,
                },
                EppThreshold {
                    battery_percent: 100,
                    epp_value: EppHint::BalancePerformance,
                },
            ],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EppThreshold {
    pub battery_percent: u8,
    pub epp_value: EppHint,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EppHint {
    Performance,
    BalancePerformance,
    BalancePower,
    Power,
}

impl std::fmt::Display for EppHint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EppHint::Performance => write!(f, "performance"),
            EppHint::BalancePerformance => write!(f, "balance_performance"),
            EppHint::BalancePower => write!(f, "balance_power"),
            EppHint::Power => write!(f, "power"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BrightnessConfig {
    /// Automatically dim the backlight on battery.
    pub auto_dim: bool,
    /// Dim to this percentage of current brightness (0-100).
    pub dim_percent: u8,
}

impl Default for BrightnessConfig {
    fn default() -> Self {
        Self {
            auto_dim: false,
            dim_percent: 60,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct InhibitorConfig {
    /// How to behave when systemd inhibitors are active.
    pub mode: InhibitorMode,
}

impl Default for InhibitorConfig {
    fn default() -> Self {
        Self {
            mode: InhibitorMode::Reduced,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InhibitorMode {
    /// Skip optimizations entirely when inhibitors are active.
    Skip,
    /// Apply only safe subset (sysfs writes, no service changes).
    Reduced,
    /// Ignore inhibitors entirely.
    Full,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct NotificationConfig {
    /// Send desktop notifications on apply/revert.
    pub enabled: bool,
    /// Notify on successful apply.
    pub on_apply: bool,
    /// Notify on successful revert.
    pub on_revert: bool,
}

impl Default for NotificationConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            on_apply: true,
            on_revert: true,
        }
    }
}

const SYSTEM_CONFIG: &str = "/etc/bop/config.toml";

/// Load the system config file if it exists.
fn load_system() -> Option<toml::Value> {
    let path = Path::new(SYSTEM_CONFIG);
    let content = std::fs::read_to_string(path).ok()?;
    toml::from_str(&content).ok()
}

/// Load the user config file (~/.config/bop/config.toml) if it exists.
fn load_user() -> Option<toml::Value> {
    let dir = dirs::config_dir()?;
    let path = dir.join("bop").join("config.toml");
    let content = std::fs::read_to_string(path).ok()?;
    toml::from_str(&content).ok()
}

/// Recursively merge two TOML values. Tables are merged key-by-key;
/// all other types in `overlay` replace `base`.
fn merge_values(base: toml::Value, overlay: toml::Value) -> toml::Value {
    match (base, overlay) {
        (toml::Value::Table(mut base_table), toml::Value::Table(overlay_table)) => {
            for (key, overlay_val) in overlay_table {
                let merged = match base_table.remove(&key) {
                    Some(base_val) => merge_values(base_val, overlay_val),
                    None => overlay_val,
                };
                base_table.insert(key, merged);
            }
            toml::Value::Table(base_table)
        }
        (_, overlay) => overlay,
    }
}

/// Load config from a specific path, ignoring system/user files.
fn load_from_path(path: &Path) -> BopConfig {
    match std::fs::read_to_string(path) {
        Ok(content) => toml::from_str(&content).unwrap_or_else(|e| {
            eprintln!(
                "warning: failed to parse config at {}: {}",
                path.display(),
                e
            );
            BopConfig::default()
        }),
        Err(e) => {
            eprintln!(
                "warning: failed to read config at {}: {}",
                path.display(),
                e
            );
            BopConfig::default()
        }
    }
}

/// Load the merged config: system defaults, then user overrides.
/// If `override_path` is provided, use only that file instead.
pub fn load(override_path: Option<&PathBuf>) -> BopConfig {
    if let Some(path) = override_path {
        return load_from_path(path);
    }

    let system = load_system();
    let user = load_user();

    let merged = match (system, user) {
        (Some(s), Some(u)) => Some(merge_values(s, u)),
        (Some(v), None) | (None, Some(v)) => Some(v),
        (None, None) => None,
    };

    match merged {
        Some(value) => value.try_into().unwrap_or_else(|e| {
            eprintln!("warning: failed to deserialize config: {}", e);
            BopConfig::default()
        }),
        None => BopConfig::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = BopConfig::default();
        assert!(!config.auto.aggressive);
        assert!(!config.epp.adaptive);
        assert_eq!(config.epp.thresholds.len(), 3);
        assert_eq!(config.epp.thresholds[0].battery_percent, 20);
        assert_eq!(config.epp.thresholds[0].epp_value, EppHint::Power);
        assert!(!config.brightness.auto_dim);
        assert_eq!(config.brightness.dim_percent, 60);
        assert_eq!(config.inhibitors.mode, InhibitorMode::Reduced);
        assert!(!config.notifications.enabled);
        assert!(config.notifications.on_apply);
        assert!(config.notifications.on_revert);
    }

    #[test]
    fn test_epp_hint_display() {
        assert_eq!(EppHint::Performance.to_string(), "performance");
        assert_eq!(
            EppHint::BalancePerformance.to_string(),
            "balance_performance"
        );
        assert_eq!(EppHint::BalancePower.to_string(), "balance_power");
        assert_eq!(EppHint::Power.to_string(), "power");
    }

    #[test]
    fn test_merge_values_tables() {
        let base: toml::Value = toml::from_str(
            r#"
            [epp]
            adaptive = false
            [brightness]
            auto_dim = false
            dim_percent = 60
        "#,
        )
        .unwrap();

        let overlay: toml::Value = toml::from_str(
            r#"
            [epp]
            adaptive = true
        "#,
        )
        .unwrap();

        let merged = merge_values(base, overlay);
        let table = merged.as_table().unwrap();

        // epp.adaptive overridden
        let epp = table["epp"].as_table().unwrap();
        assert_eq!(epp["adaptive"].as_bool(), Some(true));

        // brightness preserved
        let brightness = table["brightness"].as_table().unwrap();
        assert_eq!(brightness["auto_dim"].as_bool(), Some(false));
        assert_eq!(brightness["dim_percent"].as_integer(), Some(60));
    }

    #[test]
    fn test_merge_values_overlay_replaces_scalar() {
        let base: toml::Value = toml::from_str("value = 1").unwrap();
        let overlay: toml::Value = toml::from_str("value = 2").unwrap();
        let merged = merge_values(base, overlay);
        assert_eq!(merged["value"].as_integer(), Some(2));
    }

    #[test]
    fn test_deserialize_partial_config() {
        let toml_str = r#"
            [epp]
            adaptive = true
        "#;
        let config: BopConfig = toml::from_str(toml_str).unwrap();
        assert!(config.epp.adaptive);
        // Defaults for everything else
        assert!(!config.auto.aggressive);
        assert!(!config.brightness.auto_dim);
        assert_eq!(config.brightness.dim_percent, 60);
    }

    #[test]
    fn test_deserialize_full_config() {
        let toml_str = r#"
            [auto]
            aggressive = true

            [epp]
            adaptive = true

            [[epp.thresholds]]
            battery_percent = 30
            epp_value = "power"

            [[epp.thresholds]]
            battery_percent = 70
            epp_value = "balance_power"

            [[epp.thresholds]]
            battery_percent = 100
            epp_value = "balance_performance"

            [brightness]
            auto_dim = true
            dim_percent = 40

            [inhibitors]
            mode = "skip"

            [notifications]
            enabled = true
            on_apply = true
            on_revert = false
        "#;
        let config: BopConfig = toml::from_str(toml_str).unwrap();
        assert!(config.auto.aggressive);
        assert!(config.epp.adaptive);
        assert_eq!(config.epp.thresholds.len(), 3);
        assert_eq!(config.epp.thresholds[0].battery_percent, 30);
        assert!(config.brightness.auto_dim);
        assert_eq!(config.brightness.dim_percent, 40);
        assert_eq!(config.inhibitors.mode, InhibitorMode::Skip);
        assert!(config.notifications.enabled);
        assert!(!config.notifications.on_revert);
    }

    #[test]
    fn test_load_from_nonexistent_path() {
        let config = load_from_path(Path::new("/nonexistent/config.toml"));
        // Should return defaults without panicking
        assert!(!config.epp.adaptive);
    }

    #[test]
    fn test_load_with_no_config_files() {
        // With no override and no system/user files, should return defaults
        let config = load(None);
        assert!(!config.epp.adaptive);
        assert!(!config.auto.aggressive);
    }

    #[test]
    fn test_roundtrip_serialize() {
        let config = BopConfig::default();
        let serialized = toml::to_string_pretty(&config).unwrap();
        let deserialized: BopConfig = toml::from_str(&serialized).unwrap();
        assert_eq!(config.epp.adaptive, deserialized.epp.adaptive);
        assert_eq!(
            config.brightness.dim_percent,
            deserialized.brightness.dim_percent
        );
    }
}
