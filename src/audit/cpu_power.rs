use crate::audit::{Finding, Severity};
use crate::detect::HardwareInfo;
use crate::preset::{PlatformProfilePolicy, Preset, PresetKnobs};

pub fn check(hw: &HardwareInfo) -> Vec<Finding> {
    check_with_knobs(hw, &Preset::Moderate.knobs())
}

pub fn check_aggressive(hw: &HardwareInfo) -> Vec<Finding> {
    check_with_knobs(hw, &Preset::Supersaver.knobs())
}

pub fn check_with_preset(hw: &HardwareInfo, preset: Preset) -> Vec<Finding> {
    check_with_knobs(hw, &preset.knobs())
}

/// Rank EPP values from least power-saving (0) to most (3).
fn epp_rank(epp: &str) -> u8 {
    match epp {
        "performance" => 0,
        "balance_performance" => 1,
        "balance_power" => 2,
        "power" => 3,
        _ => 0,
    }
}

pub fn check_with_knobs(hw: &HardwareInfo, knobs: &PresetKnobs) -> Vec<Finding> {
    let force_low_power = knobs.platform_profile == PlatformProfilePolicy::ForceLowPower;
    let mut findings = Vec::new();

    // Check if amd-pstate driver is active — only relevant when EPP knob is active
    if knobs.epp.is_some() && hw.cpu.is_amd() && !hw.cpu.is_amd_pstate() {
        let driver = hw.cpu.scaling_driver.as_deref().unwrap_or("unknown");
        findings.push(
            Finding::new(
                Severity::High,
                "CPU",
                format!("Using '{}' instead of amd-pstate - EPP unavailable", driver),
            )
            .current(driver)
            .recommended("amd-pstate-epp")
            .impact("~2-5W savings; enables fine-grained energy/performance tuning")
            .path("cpu0/cpufreq/scaling_driver")
            .weight(9),
        );
    }

    // Check amd_pstate mode — only relevant when EPP knob is active
    if knobs.epp.is_some()
        && hw.cpu.is_amd_pstate()
        && let Some(ref mode) = hw.cpu.amd_pstate_mode
        && mode == "active"
    {
        findings.push(
            Finding::new(
                Severity::Info,
                "CPU",
                "amd-pstate in active mode — guided or passive may improve idle power",
            )
            .current("active")
            .recommended("Experiment with guided mode (kernel param amd_pstate=guided)")
            .impact("Potentially 1-2W better idle power (varies by workload)")
            .path("sys/devices/system/cpu/amd_pstate/status")
            .weight(0),
        );
    }

    // Check EPP — only when the knob would change it
    if let Some(ref target_epp) = knobs.epp
        && let Some(ref epp) = hw.cpu.epp
    {
        let target: &str = target_epp;
        let current_rank = epp_rank(epp);
        let target_rank = epp_rank(target);

        // Flag EPP drift — use string comparison to match build_plan's logic.
        // Skip when current is "power" since build_plan won't overwrite it
        // (power is already the most power-saving value), unless EPP is locked.
        if epp.as_str() != target && (knobs.epp_locked || epp != "power") {
            let (severity, weight, impact) = if current_rank < target_rank {
                // Moving to more power-saving
                match epp.as_str() {
                    "performance" => (Severity::High, 8, "~2-3W savings"),
                    "balance_performance" => (Severity::Medium, 6, "~1-3W savings"),
                    _ => (Severity::Low, 3, "~0.5-1W savings"),
                }
            } else {
                // Moving to more performant (e.g. adaptive high-battery)
                (Severity::Info, 1, "EPP will be adjusted for current conditions")
            };
            findings.push(
                Finding::new(
                    severity,
                    "CPU",
                    format!("EPP at '{}' — target is '{}'", epp, target),
                )
                .current(epp.as_str())
                .recommended(target)
                .impact(impact)
                .path("cpu*/cpufreq/energy_performance_preference")
                .weight(weight),
            );
        }
    }

    // Check platform profile — only when the knob would change it
    if knobs.platform_profile != PlatformProfilePolicy::NoChange
        && let Some(ref profile) = hw.platform.platform_profile
    {
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
                if force_low_power {
                    findings.push(
                        Finding::new(
                            Severity::Low,
                            "CPU",
                            "Platform profile at balanced — low-power reduces TDP for battery savings",
                        )
                        .current("balanced")
                        .recommended("low-power")
                        .impact("~0.5-1W savings with lower TDP cap (reduced sustained performance)")
                        .path("/sys/firmware/acpi/platform_profile")
                        .weight(3),
                    );
                } else {
                    findings.push(
                        Finding::new(
                            Severity::Info,
                            "CPU",
                            "Platform profile at balanced — low-power saves ~0.5-1W but throttles more",
                        )
                        .current("balanced")
                        .recommended("low-power (trades sustained performance for battery)")
                        .impact("~0.5-1W savings with lower TDP cap")
                        .path("/sys/firmware/acpi/platform_profile")
                        .weight(0),
                    );
                }
            }
            "low-power" => {
                // Optimal
            }
            _ => {}
        }
    }

    // Check governor — only relevant when EPP knob is active
    if knobs.epp.is_some()
        && let Some(ref governor) = hw.cpu.governor
        && hw.cpu.is_amd_pstate()
        && governor != "powersave"
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

    // Flag turbo when knobs would change it
    if let Some(desired) = knobs.turbo_boost
        && hw.cpu.has_boost
        && hw.cpu.boost_enabled != desired
    {
        let (message, current, recommended, impact, weight) = if desired {
            (
                "Turbo boost disabled — knobs request re-enabling it",
                "disabled",
                "enabled",
                "Restores peak single-thread performance",
                2,
            )
        } else {
            (
                "Turbo boost enabled — disabling saves power under bursty loads",
                "enabled",
                "disabled (significant single-thread performance loss)",
                "~2-5W savings under load at cost of peak performance",
                4,
            )
        };
        findings.push(
            Finding::new(Severity::Low, "CPU", message)
                .current(current)
                .recommended(recommended)
                .impact(impact)
                .path("sys/devices/system/cpu/cpufreq/boost")
                .weight(weight),
        );
    }

    findings
}
