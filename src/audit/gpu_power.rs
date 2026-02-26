use crate::audit::{Finding, Severity};
use crate::detect::HardwareInfo;

pub fn check(hw: &HardwareInfo) -> Vec<Finding> {
    let mut findings = Vec::new();

    if !hw.gpu.is_amd() {
        return findings;
    }

    // Check DPM level
    if let Some(ref dpm) = hw.gpu.dpm_level
        && dpm != "auto"
    {
        findings.push(
            Finding::new(
                Severity::Medium,
                "GPU",
                format!("GPU DPM level '{}' instead of auto", dpm),
            )
            .current(dpm)
            .recommended("auto")
            .impact("GPU may not enter low-power states")
            .path("power_dpm_force_performance_level")
            .weight(5),
        );
    }

    // Check dGPU power state (Framework 16 expansion bay GPU)
    if let Some(ref power_state) = hw.gpu.dgpu_power_state
        && power_state != "D3cold"
    {
        findings.push(
            Finding::new(
                Severity::Medium,
                "GPU",
                format!("Discrete GPU in {} instead of D3cold", power_state),
            )
            .current(power_state)
            .recommended("D3cold")
            .impact("~5-8W savings when dGPU is idle")
            .path("power_state")
            .weight(7),
        );
    }

    findings
}
