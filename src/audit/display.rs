use crate::audit::{Finding, Severity};
use crate::detect::HardwareInfo;
use crate::sysfs::SysfsRoot;

pub fn check(hw: &HardwareInfo, sysfs: &SysfsRoot) -> Vec<Finding> {
    let mut findings = Vec::new();

    // Check backlight brightness level
    let bl_base = "sys/class/backlight";
    if let Ok(entries) = sysfs.list_dir(bl_base) {
        for entry in entries {
            let base = format!("{}/{}", bl_base, entry);
            let brightness: Option<u32> = sysfs
                .read_optional(format!("{}/brightness", base))
                .unwrap_or(None)
                .and_then(|v| v.parse().ok());
            let max_brightness: Option<u32> = sysfs
                .read_optional(format!("{}/max_brightness", base))
                .unwrap_or(None)
                .and_then(|v| v.parse().ok());

            if let (Some(cur), Some(max)) = (brightness, max_brightness)
                && max > 0
            {
                let pct = (cur as f64 / max as f64 * 100.0) as u32;
                if pct > 70 {
                    findings.push(
                        Finding::new(
                            Severity::Info,
                            "Display",
                            format!("Backlight at {}% - reducing saves significant power", pct),
                        )
                        .current(format!("{}%", pct))
                        .recommended("30-50% for indoor use")
                        .impact("Display is often the largest power consumer")
                        .path(format!("{}/brightness", base))
                        .weight(0), // Info only
                    );
                }
            }
        }
    }

    // Check for internal eDP display — suggest reducing refresh rate on battery
    if let Ok(entries) = sysfs.list_dir("sys/class/drm") {
        for entry in entries {
            if !entry.contains('-') || entry.starts_with("version") {
                continue;
            }
            let status_path = format!("sys/class/drm/{}/status", entry);
            if let Some(status) = sysfs.read_optional(&status_path).unwrap_or(None)
                && status == "connected"
                && entry.contains("eDP")
            {
                findings.push(
                    Finding::new(
                        Severity::Info,
                        "Display",
                        "Consider reducing display refresh rate to 60Hz on battery",
                    )
                    .impact("~1W savings (measured on Framework 16 with 165Hz panel)")
                    .path(status_path)
                    .weight(0),
                );
                break; // Only emit once for the first connected eDP
            }
        }
    }

    // Check if PSR (Panel Self-Refresh) is disabled via amdgpu.dcdebugmask
    if hw.gpu.is_amd() && hw.has_kernel_param("amdgpu.dcdebugmask") {
        let mask_value = hw
            .kernel_param_value("amdgpu.dcdebugmask")
            .unwrap_or_default();
        findings.push(
            Finding::new(
                Severity::Info,
                "Display",
                "Panel Self-Refresh may be disabled (amdgpu.dcdebugmask set)",
            )
            .current(&mask_value)
            .recommended("Remove amdgpu.dcdebugmask once PSR bugs are fixed")
            .impact("~0.5-1.5W potential savings when PSR works correctly")
            .weight(0),
        );
    }

    findings
}
