use crate::audit::{self, Finding};
use crate::detect::HardwareInfo;
use crate::preset::{PlatformProfilePolicy, Preset, PresetKnobs, UsbPolicy};
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

    fn audit_with_opts(
        &self,
        hw: &HardwareInfo,
        _preset: Preset,
        knobs: &PresetKnobs,
    ) -> Vec<Finding> {
        if !knobs.has_any_active() {
            return Vec::new();
        }

        let sysfs = SysfsRoot::system();
        let mut findings = Vec::new();

        // Always-safe checks when any knob is active
        if knobs.audio_power_save {
            findings.extend(audit::audio::check(&sysfs));
        }
        if knobs.nmi_watchdog_disable || knobs.dirty_writeback.is_some() {
            findings.extend(audit::sysctl::check_with_knobs(&sysfs, knobs));
        }
        if knobs.kernel_params {
            findings.extend(audit::kernel_params::check(hw));
        }

        // Hardware-specific checks driven by knobs
        if knobs.epp.is_some() || knobs.platform_profile != PlatformProfilePolicy::NoChange || knobs.turbo_boost.is_some() {
            findings.extend(audit::cpu_power::check_with_knobs(hw, knobs));
        }
        if knobs.aspm_policy.is_some() || knobs.pci_runtime_pm {
            findings.extend(audit::pci_power::check_with_knobs(hw, knobs));
        }
        if knobs.usb_autosuspend != UsbPolicy::NoChange {
            findings.extend(audit::usb_power::check_with_knobs(&sysfs, knobs));
        }
        if knobs.gpu_dpm {
            findings.extend(audit::gpu_power::check(hw));
        }

        // Informational checks — run whenever doing real optimizations
        if knobs.epp.is_some() || knobs.pci_runtime_pm || knobs.gpu_dpm {
            findings.extend(audit::network_power::check(hw));
            findings.extend(audit::display::check(hw, &sysfs));
        }
        if knobs.epp.is_some() || knobs.pci_runtime_pm || knobs.gpu_dpm || knobs.acpi_wakeup_filter {
            findings.extend(audit::sleep::check(hw, &sysfs));
        }
        // Service conflict check — matches apply's has_any_active() gate
        if knobs.has_any_active() {
            findings.extend(audit::services::check());
        }

        findings
    }
}
