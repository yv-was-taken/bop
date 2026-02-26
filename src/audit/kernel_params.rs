use crate::audit::{Finding, Severity};
use crate::detect::HardwareInfo;

pub fn check(hw: &HardwareInfo) -> Vec<Finding> {
    let mut findings = Vec::new();

    // Check for acpi.ec_no_wakeup=1
    if !hw.has_kernel_param("acpi.ec_no_wakeup") {
        findings.push(
            Finding::new(
                Severity::High,
                "Kernel",
                "EC wakeup not disabled - causes high sleep drain",
            )
            .current("unset")
            .recommended("acpi.ec_no_wakeup=1")
            .impact("~5-7% sleep drain reduction")
            .path("/proc/cmdline")
            .weight(9),
        );
    } else if hw.kernel_param_value("acpi.ec_no_wakeup") != Some("1".to_string()) {
        findings.push(
            Finding::new(Severity::Medium, "Kernel", "acpi.ec_no_wakeup not set to 1")
                .current(
                    hw.kernel_param_value("acpi.ec_no_wakeup")
                        .unwrap_or_default(),
                )
                .recommended("acpi.ec_no_wakeup=1")
                .impact("~5-7% sleep drain reduction")
                .path("/proc/cmdline")
                .weight(7),
        );
    }

    // Check for rtc_cmos.use_acpi_alarm=1
    if !hw.has_kernel_param("rtc_cmos.use_acpi_alarm") {
        findings.push(
            Finding::new(
                Severity::Medium,
                "Kernel",
                "RTC ACPI alarm not enabled - prevents deepest sleep states",
            )
            .current("unset")
            .recommended("rtc_cmos.use_acpi_alarm=1")
            .impact("Enables deeper CPU sleep states")
            .path("/proc/cmdline")
            .weight(5),
        );
    }

    // Check for NVMe APST disabled
    if let Some(ref val) = hw.kernel_param_value("nvme_core.default_ps_max_latency_us")
        && val == "0"
    {
        findings.push(
            Finding::new(
                Severity::Medium,
                "Kernel",
                "NVMe APST disabled - drive stays in highest power state",
            )
            .current("nvme_core.default_ps_max_latency_us=0")
            .recommended("Remove parameter (let APST work normally)")
            .impact("~0.5-1W savings from NVMe power state transitions")
            .path("/proc/cmdline")
            .weight(5),
        );
    }

    // Check for amdgpu.abmlevel
    if hw.gpu.is_amd() {
        match hw.kernel_param_value("amdgpu.abmlevel") {
            None => {
                findings.push(
                    Finding::new(
                        Severity::Medium,
                        "Kernel",
                        "AMD Adaptive Backlight Management not enabled",
                    )
                    .current("unset (level 0)")
                    .recommended("amdgpu.abmlevel=3")
                    .impact("~0.5-1W display power saving")
                    .path("/proc/cmdline")
                    .weight(5),
                );
            }
            Some(ref val) if val.parse::<u32>().unwrap_or(0) < 3 => {
                findings.push(
                    Finding::new(Severity::Low, "Kernel", "ABM level below recommended")
                        .current(format!("amdgpu.abmlevel={}", val))
                        .recommended("amdgpu.abmlevel=3")
                        .impact("Higher levels save more display power")
                        .path("/proc/cmdline")
                        .weight(3),
                );
            }
            _ => {}
        }
    }

    findings
}
