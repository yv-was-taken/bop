pub mod framework16_amd;

use crate::audit::Finding;
use crate::detect::HardwareInfo;

/// A hardware profile encodes laptop-specific power optimization knowledge.
pub trait HardwareProfile: std::fmt::Debug {
    /// Display name for this profile
    fn name(&self) -> &str;

    /// Whether this profile matches the detected hardware
    fn matches(&self, hw: &HardwareInfo) -> bool;

    /// Run all audit checks specific to this hardware
    fn audit(&self, hw: &HardwareInfo) -> Vec<Finding>;
}

/// Registry of all known hardware profiles.
pub fn all_profiles() -> Vec<Box<dyn HardwareProfile>> {
    vec![Box::new(framework16_amd::Framework16Amd)]
}

/// Find the best matching profile for the detected hardware.
pub fn detect_profile(hw: &HardwareInfo) -> Option<Box<dyn HardwareProfile>> {
    all_profiles().into_iter().find(|p| p.matches(hw))
}
