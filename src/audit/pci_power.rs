use crate::audit::{Finding, Severity};
use crate::detect::HardwareInfo;
use crate::preset::{Preset, PresetKnobs};

pub fn check(hw: &HardwareInfo) -> Vec<Finding> {
    check_with_knobs(hw, &Preset::Moderate.knobs())
}

pub fn check_aggressive(hw: &HardwareInfo) -> Vec<Finding> {
    check_with_knobs(hw, &Preset::Supersaver.knobs())
}

pub fn check_with_preset(hw: &HardwareInfo, preset: Preset) -> Vec<Finding> {
    check_with_knobs(hw, &preset.knobs())
}

/// Rank ASPM policies from least power-saving (0) to most (3).
fn aspm_rank(policy: &str) -> u8 {
    match policy {
        "performance" => 0,
        "default" => 1,
        "powersave" => 2,
        "powersupersave" => 3,
        _ => 1,
    }
}

pub fn check_with_knobs(hw: &HardwareInfo, knobs: &PresetKnobs) -> Vec<Finding> {
    let mut findings = Vec::new();

    // Check ASPM policy — only when the knob requests a target policy.
    // Mirror build_plan's needs_change logic: for target "powersave", accept
    // current "powersave" or "powersupersave" as no-op; otherwise exact match.
    if let Some(target) = knobs.aspm_policy.as_deref()
        && let Some(ref current) = hw.pci.aspm_policy
    {
        let needs_change = match target {
            "powersave" => !matches!(current.as_str(), "powersave" | "powersupersave"),
            other => current.as_str() != other,
        };

        if needs_change {
            let current_rank = aspm_rank(current);
            let target_rank = aspm_rank(target);
            let moving_to_power_saving = target_rank > current_rank;

            let (severity, weight, impact) = if moving_to_power_saving {
                match current.as_str() {
                    "performance" => (
                        Severity::High,
                        8,
                        "~1-2W savings from PCIe link power management",
                    ),
                    "default" => (
                        Severity::Medium,
                        6,
                        "~0.5-1W savings from PCIe link power management",
                    ),
                    "powersave" => (
                        Severity::Low,
                        3,
                        "~0.2-0.5W additional savings (may cause WiFi/NVMe issues)",
                    ),
                    _ => (Severity::Low, 3, "ASPM policy will be adjusted for power saving"),
                }
            } else {
                // Moving toward performance
                (
                    Severity::Info,
                    1,
                    "ASPM policy will be adjusted (may increase power use)",
                )
            };
            findings.push(
                Finding::new(
                    severity,
                    "PCIe",
                    format!("ASPM policy at '{}' — target is '{}'", current, target),
                )
                .current(current.as_str())
                .recommended(target)
                .impact(impact)
                .path("/sys/module/pcie_aspm/parameters/policy")
                .weight(weight),
            );
        }
    }

    // Check per-device runtime PM
    if knobs.pci_runtime_pm {
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
    }

    findings
}
