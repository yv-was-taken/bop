use crate::audit::{self, Finding};
use crate::detect::HardwareInfo;
use crate::profile::HardwareProfile;
use crate::sysfs::SysfsRoot;

#[derive(Debug)]
pub struct Framework16Amd;

impl HardwareProfile for Framework16Amd {
    fn name(&self) -> &str {
        "Framework Laptop 16 (AMD Ryzen 7040 Series)"
    }

    fn matches(&self, hw: &HardwareInfo) -> bool {
        hw.dmi.is_framework_16() && hw.cpu.is_amd()
    }

    fn audit(&self, hw: &HardwareInfo) -> Vec<Finding> {
        let sysfs = SysfsRoot::system();
        let mut findings = Vec::new();

        findings.extend(audit::kernel_params::check(hw));
        findings.extend(audit::cpu_power::check(hw));
        findings.extend(audit::gpu_power::check(hw));
        findings.extend(audit::pci_power::check(hw));
        findings.extend(audit::usb_power::check(&sysfs));
        findings.extend(audit::audio::check(&sysfs));
        findings.extend(audit::network_power::check(hw));
        findings.extend(audit::sleep::check(hw, &sysfs));
        findings.extend(audit::services::check());
        findings.extend(audit::display::check(hw, &sysfs));
        findings.extend(audit::sysctl::check(&sysfs));

        findings
    }
}
