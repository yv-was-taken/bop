use crate::audit::{self, Finding};
use crate::detect::HardwareInfo;
use crate::profile::HardwareProfile;
use crate::sysfs::SysfsRoot;

/// Fallback profile for any laptop without a dedicated profile.
/// Runs hardware-agnostic audit checks that are safe for all machines.
#[derive(Debug)]
pub struct GenericLaptop;

impl HardwareProfile for GenericLaptop {
    fn name(&self) -> &str {
        "Generic Linux Laptop"
    }

    fn matches(&self, hw: &HardwareInfo) -> bool {
        hw.battery.present
    }

    fn audit_with_opts(&self, hw: &HardwareInfo, aggressive: bool) -> Vec<Finding> {
        let sysfs = SysfsRoot::system();
        let mut findings = Vec::new();

        if aggressive {
            findings.extend(audit::cpu_power::check_aggressive(hw));
            findings.extend(audit::pci_power::check_aggressive(hw));
            findings.extend(audit::usb_power::check_aggressive(&sysfs));
        } else {
            findings.extend(audit::cpu_power::check(hw));
            findings.extend(audit::pci_power::check(hw));
            findings.extend(audit::usb_power::check(&sysfs));
        }
        findings.extend(audit::audio::check(&sysfs));
        findings.extend(audit::network_power::check(hw));
        findings.extend(audit::sleep::check(hw, &sysfs));
        findings.extend(audit::services::check());
        findings.extend(audit::sysctl::check(&sysfs));

        findings
    }
}
