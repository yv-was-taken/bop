use crate::audit::{Finding, Severity};
use crate::detect::HardwareInfo;

pub fn check(hw: &HardwareInfo) -> Vec<Finding> {
    let mut findings = Vec::new();

    // Check ASPM policy
    // powersave (L0s + L1) is the safe default; powersupersave (L1.1/L1.2) can
    // cause WiFi dropouts and NVMe stutter on some hardware, so it's reserved
    // for aggressive mode.
    if let Some(ref policy) = hw.pci.aspm_policy {
        match policy.as_str() {
            "default" => {
                findings.push(
                    Finding::new(
                        Severity::Medium,
                        "PCIe",
                        "ASPM policy at 'default' - not using link sleep states",
                    )
                    .current("default")
                    .recommended("powersave")
                    .impact("~0.5-1W savings from PCIe link power management")
                    .path("/sys/module/pcie_aspm/parameters/policy")
                    .weight(6),
                );
            }
            "performance" => {
                findings.push(
                    Finding::new(
                        Severity::High,
                        "PCIe",
                        "ASPM disabled (performance mode) - PCIe links always active",
                    )
                    .current("performance")
                    .recommended("powersave")
                    .impact("~1-2W savings from PCIe link power management")
                    .path("/sys/module/pcie_aspm/parameters/policy")
                    .weight(8),
                );
            }
            "powersave" | "powersupersave" => {
                // Optimal
            }
            _ => {}
        }
    }

    // Check per-device runtime PM
    let non_auto = hw.pci.devices_without_runtime_pm();
    if !non_auto.is_empty() {
        findings.push(
            Finding::new(
                Severity::Medium,
                "PCIe",
                format!(
                    "{}/{} PCI devices not using runtime power management",
                    non_auto.len(),
                    hw.pci.devices.len()
                ),
            )
            .current(format!("{} devices set to 'on'", non_auto.len()))
            .recommended("All devices set to 'auto'")
            .impact("~0.5W savings from idle device power gating")
            .path("/sys/bus/pci/devices/*/power/control")
            .weight(5),
        );
    }

    findings
}
