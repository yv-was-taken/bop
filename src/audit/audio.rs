use crate::audit::{Finding, Severity};
use crate::sysfs::SysfsRoot;

pub fn check(sysfs: &SysfsRoot) -> Vec<Finding> {
    let mut findings = Vec::new();

    // Check HDA Intel power save
    let power_save_path = "sys/module/snd_hda_intel/parameters/power_save";
    if let Some(val) = sysfs.read_optional(power_save_path).unwrap_or(None) {
        match val.as_str() {
            "0" => {
                findings.push(
                    Finding::new(Severity::Low, "Audio", "HDA Intel power save disabled")
                        .current("0 (disabled)")
                        .recommended("1 (1 second timeout)")
                        .impact("~0.1-0.3W savings when audio idle")
                        .path(power_save_path)
                        .weight(2),
                );
            }
            "1" => {
                // Already optimal
            }
            _ => {
                // Non-standard value, just note it
                findings.push(
                    Finding::new(
                        Severity::Info,
                        "Audio",
                        format!("HDA power_save set to {} (non-standard)", val),
                    )
                    .current(&val)
                    .recommended("1")
                    .impact("Standard value is 1 second")
                    .path(power_save_path)
                    .weight(1),
                );
            }
        }
    }

    // Check power_save_controller
    let controller_path = "sys/module/snd_hda_intel/parameters/power_save_controller";
    if let Some(val) = sysfs.read_optional(controller_path).unwrap_or(None)
        && val == "N"
    {
        findings.push(
            Finding::new(Severity::Low, "Audio", "HDA controller power save disabled")
                .current("N (disabled)")
                .recommended("Y (enabled)")
                .impact("Controller stays powered when idle")
                .path(controller_path)
                .weight(2),
        );
    }

    findings
}
