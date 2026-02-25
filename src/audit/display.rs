use crate::audit::{Finding, Severity};
use crate::sysfs::SysfsRoot;

pub fn check(sysfs: &SysfsRoot) -> Vec<Finding> {
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

    findings
}
