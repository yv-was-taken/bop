pub mod framework16_amd;
pub mod generic_laptop;

use crate::audit::Finding;
use crate::detect::HardwareInfo;

/// A hardware profile encodes laptop-specific power optimization knowledge.
pub trait HardwareProfile: std::fmt::Debug {
    /// Display name for this profile
    fn name(&self) -> &str;

    /// Whether this profile matches the detected hardware
    fn matches(&self, hw: &HardwareInfo) -> bool;

    /// Run all audit checks specific to this hardware
    fn audit(&self, hw: &HardwareInfo) -> Vec<Finding> {
        self.audit_with_opts(hw, false)
    }

    /// Run audit checks with aggressive mode option
    fn audit_with_opts(&self, hw: &HardwareInfo, aggressive: bool) -> Vec<Finding>;
}

/// Registry of all known hardware profiles.
/// Specific profiles first, generic fallback last.
pub fn all_profiles() -> Vec<Box<dyn HardwareProfile>> {
    vec![
        Box::new(framework16_amd::Framework16Amd),
        Box::new(generic_laptop::GenericLaptop),
    ]
}

/// Find the best matching profile for the detected hardware.
pub fn detect_profile(hw: &HardwareInfo) -> Option<Box<dyn HardwareProfile>> {
    all_profiles().into_iter().find(|p| p.matches(hw))
}
