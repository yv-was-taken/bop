use crate::audit::{Finding, Severity};
use crate::sysfs::SysfsRoot;

pub fn check(sysfs: &SysfsRoot) -> Vec<Finding> {
    check_with_opts(sysfs, false)
}

pub fn check_aggressive(sysfs: &SysfsRoot) -> Vec<Finding> {
    check_with_opts(sysfs, true)
}

fn check_with_opts(sysfs: &SysfsRoot, aggressive: bool) -> Vec<Finding> {
    let mut findings = Vec::new();

    // Check USB autosuspend
    // Normal: skip HID input devices (keyboards, mice) and expansion cards.
    // Aggressive: autosuspend everything.
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
                    if aggressive {
                        no_autosuspend += 1;
                    } else {
                        let product = sysfs
                            .read_optional(format!("{}/{}/product", usb_base, device))
                            .unwrap_or(None)
                            .unwrap_or_default()
                            .to_lowercase();

                        let is_input = product.contains("keyboard")
                            || product.contains("mouse")
                            || product.contains("trackpad")
                            || product.contains("touchpad");
                        let is_expansion = product.contains("expansion")
                            || product.contains("displayport")
                            || product.contains("hdmi");

                        if !is_input && !is_expansion {
                            no_autosuspend += 1;
                        }
                    }
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
                .impact(if aggressive {
                    "Power savings from idle USB devices (may cause input latency)"
                } else {
                    "Minor power savings from idle USB devices"
                })
                .path("/sys/bus/usb/devices/*/power/control")
                .weight(2),
            );
        }
    }

    findings
}
