use crate::audit::{Finding, Severity};
use crate::detect::HardwareInfo;

pub fn check(hw: &HardwareInfo) -> Vec<Finding> {
    let mut findings = Vec::new();

    if !hw.gpu.is_amd() {
        return findings;
    }

    // Check DPM level
    if let Some(ref dpm) = hw.gpu.dpm_level {
        if dpm != "auto" {
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
    }

    // ABM is checked via kernel_params module since it's a kernel parameter
    // Here we just note if ABM is available but not configured
    if hw.gpu.has_abm && hw.gpu.abm_level.unwrap_or(0) == 0 {
        // Only report if kernel_params didn't already catch it
        // (kernel_params checks the cmdline; this checks the live module param)
        if hw.has_kernel_param("amdgpu.abmlevel") {
            // kernel_params module will handle this
        } else {
            // No finding here - kernel_params.rs handles the ABM check
        }
    }

    findings
}
