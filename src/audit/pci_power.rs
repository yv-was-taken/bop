use crate::audit::{Finding, Severity};
use crate::detect::HardwareInfo;

pub fn check(hw: &HardwareInfo) -> Vec<Finding> {
    check_with_opts(hw, false)
}

pub fn check_aggressive(hw: &HardwareInfo) -> Vec<Finding> {
    check_with_opts(hw, true)
}

fn check_with_opts(hw: &HardwareInfo, aggressive: bool) -> Vec<Finding> {
    let mut findings = Vec::new();

    // Check ASPM policy
    // Normal: powersave (L0s + L1) is the safe target.
    // Aggressive: powersupersave (L1.1/L1.2) for deepest sleep states.
    if let Some(ref policy) = hw.pci.aspm_policy {
        let target = if aggressive {
            "powersupersave"
        } else {
            "powersave"
        };

        match policy.as_str() {
            "default" => {
                findings.push(
                    Finding::new(
                        Severity::Medium,
                        "PCIe",
                        "ASPM policy at 'default' - not using link sleep states",
                    )
                    .current("default")
                    .recommended(target)
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
                    .recommended(target)
                    .impact("~1-2W savings from PCIe link power management")
                    .path("/sys/module/pcie_aspm/parameters/policy")
                    .weight(8),
                );
            }
            "powersave" if aggressive => {
                findings.push(
                    Finding::new(
                        Severity::Low,
                        "PCIe",
                        "ASPM at powersave - powersupersave enables deeper L1.1/L1.2 substates",
                    )
                    .current("powersave")
                    .recommended("powersupersave")
                    .impact("~0.2-0.5W additional savings (may cause WiFi/NVMe issues)")
                    .path("/sys/module/pcie_aspm/parameters/policy")
                    .weight(3),
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
