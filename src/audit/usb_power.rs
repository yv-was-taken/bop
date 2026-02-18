use crate::audit::{Finding, Severity};
use crate::sysfs::SysfsRoot;

pub fn check(sysfs: &SysfsRoot) -> Vec<Finding> {
    let mut findings = Vec::new();

    // Check USB autosuspend
    let usb_base = "sys/bus/usb/devices";
    if let Ok(devices) = sysfs.list_dir(usb_base) {
        let mut no_autosuspend = 0;
        let mut total = 0;

        for device in &devices {
            // Skip interfaces (contain ':')
            if device.contains(':') {
                continue;
            }

            let control_path = format!("{}/{}/power/control", usb_base, device);
            if let Some(control) = sysfs.read_optional(&control_path).unwrap_or(None) {
                total += 1;
                if control != "auto" {
                    no_autosuspend += 1;
                }
            }
        }

        if no_autosuspend > 0 {
            findings.push(
                Finding::new(
                    Severity::Low,
                    "USB",
                    format!(
                        "{}/{} USB devices not using autosuspend",
                        no_autosuspend, total
                    ),
                )
                .current(format!("{} devices set to 'on'", no_autosuspend))
                .recommended("All devices set to 'auto'")
                .impact("Minor power savings from idle USB devices")
                .path("/sys/bus/usb/devices/*/power/control")
                .weight(2),
            );
        }
    }

    findings
}
