use crate::audit::{Finding, Severity};
use crate::detect::HardwareInfo;

pub fn check(hw: &HardwareInfo) -> Vec<Finding> {
    let mut findings = Vec::new();

    // Check EPP
    if let Some(ref epp) = hw.cpu.epp {
        match epp.as_str() {
            "performance" => {
                findings.push(
                    Finding::new(
                        Severity::High,
                        "CPU",
                        "EPP set to performance - maximum power consumption",
                    )
                    .current("performance")
                    .recommended("balance_power")
                    .impact("~2-3W savings")
                    .path("cpu*/cpufreq/energy_performance_preference")
                    .weight(8),
                );
            }
            "balance_performance" => {
                findings.push(
                    Finding::new(
                        Severity::Medium,
                        "CPU",
                        "EPP at balance_performance - not optimal for battery",
                    )
                    .current("balance_performance")
                    .recommended("balance_power")
                    .impact("~1-3W savings")
                    .path("cpu*/cpufreq/energy_performance_preference")
                    .weight(6),
                );
            }
            "balance_power" | "power" => {
                // Good, no finding needed
            }
            other => {
                findings.push(
                    Finding::new(
                        Severity::Info,
                        "CPU",
                        format!("Unusual EPP value: {}", other),
                    )
                    .current(other)
                    .recommended("balance_power")
                    .impact("Unknown")
                    .weight(1),
                );
            }
        }
    }

    // Check platform profile
    if let Some(ref profile) = hw.platform.platform_profile {
        match profile.as_str() {
            "performance" => {
                findings.push(
                    Finding::new(
                        Severity::High,
                        "CPU",
                        "Platform profile set to performance (TDP: 45W)",
                    )
                    .current("performance")
                    .recommended("low-power")
                    .impact("~1-2W savings at idle, lower TDP cap")
                    .path("/sys/firmware/acpi/platform_profile")
                    .weight(7),
                );
            }
            "balanced" => {
                findings.push(
                    Finding::new(
                        Severity::Low,
                        "CPU",
                        "Platform profile at balanced - could save more on battery",
                    )
                    .current("balanced")
                    .recommended("low-power")
                    .impact("~0.5-1W savings")
                    .path("/sys/firmware/acpi/platform_profile")
                    .weight(3),
                );
            }
            "low-power" => {
                // Optimal
            }
            _ => {}
        }
    }

    // Check governor
    if let Some(ref governor) = hw.cpu.governor
        && hw.cpu.is_amd_pstate() && governor != "powersave"
    {
        findings.push(
            Finding::new(
                Severity::Medium,
                "CPU",
                format!("Governor '{}' suboptimal with amd-pstate", governor),
            )
            .current(governor)
            .recommended("powersave")
            .impact("amd-pstate uses EPP for power/perf balance; powersave governor is correct")
            .path("cpu*/cpufreq/scaling_governor")
            .weight(4),
        );
    }

    findings
}
