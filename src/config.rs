use serde::{Deserialize, Serialize};

/// Top-level configuration for bop.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Config {
    pub brightness: BrightnessConfig,
}

/// Configuration for automatic backlight dimming on battery.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BrightnessConfig {
    /// Whether to automatically dim the backlight when switching to battery.
    pub auto_dim: bool,
    /// Target brightness as a percentage of current brightness (1-100).
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
