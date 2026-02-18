use crate::audit::{Finding, Severity};
use crate::detect::HardwareInfo;
use crate::sysfs::SysfsRoot;

/// Controllers that should keep wakeup enabled (internal devices).
const ESSENTIAL_WAKE_CONTROLLERS: &[&str] = &["XHC0"];

pub fn check(hw: &HardwareInfo, sysfs: &SysfsRoot) -> Vec<Finding> {
    let mut findings = Vec::new();

    // Check for unnecessary ACPI wakeup sources
    let mut unnecessary_enabled = Vec::new();
    for source in &hw.platform.acpi_wakeup_sources {
        if source.enabled && !ESSENTIAL_WAKE_CONTROLLERS.contains(&source.device.as_str()) {
            // Check if this controller has any real devices attached
            let has_devices = controller_has_devices(&source.device, sysfs);
            if !has_devices {
                unnecessary_enabled.push(source.device.clone());
            }
        }
    }

    if !unnecessary_enabled.is_empty() {
        findings.push(
            Finding::new(
                Severity::Medium,
                "Sleep",
                format!(
                    "{} unnecessary ACPI wakeup sources enabled",
                    unnecessary_enabled.len()
                ),
            )
            .current(format!("Enabled: {}", unnecessary_enabled.join(", ")))
            .recommended("Disable all except XHC0 (internal keyboard/BT)")
            .impact("Reduces spurious wakeups during sleep")
            .path("/proc/acpi/wakeup")
            .weight(6),
        );
    }

    // Check sleep state
    if hw.platform.mem_sleep.as_deref() != Some("s2idle") {
        if let Some(ref mem_sleep) = hw.platform.mem_sleep {
            findings.push(
                Finding::new(
                    Severity::Info,
                    "Sleep",
                    "System using deep sleep instead of s2idle",
                )
                .current(mem_sleep)
                .recommended("s2idle (for AMD platforms)")
                .impact("s2idle is recommended for modern AMD; deep may work but has less testing")
                .path("/sys/power/mem_sleep")
                .weight(2),
            );
        }
    }

    findings
}

/// Check if a USB controller (e.g., XHC1) has actual USB devices connected.
/// This traces through the PCI device -> USB root hub -> USB device chain.
fn controller_has_devices(controller_name: &str, sysfs: &SysfsRoot) -> bool {
    // Map controller names to PCI search patterns
    // We look through USB buses to find devices beyond root hubs
    let usb_base = "sys/bus/usb/devices";
    let Ok(usb_devices) = sysfs.list_dir(usb_base) else {
        return false;
    };

    // Find root hubs and their children for this controller
    // The controller name appears in the ACPI path -- we need to match via PCI address
    // For simplicity, we check if any non-root-hub USB devices exist on the buses
    // associated with this controller

    // First, find the PCI address of this controller from ACPI wakeup
    // The wakeup source format includes the PCI device path
    let proc_wakeup = match sysfs.read("proc/acpi/wakeup") {
        Ok(w) => w,
        Err(_) => return false,
    };

    let mut pci_addr = None;
    for line in proc_wakeup.lines() {
        if line.starts_with(controller_name) {
            // Extract PCI address from the line (last field, format: pci:0000:XX:XX.X)
            if let Some(pci_part) = line.split_whitespace().find(|p| p.starts_with("pci:")) {
                pci_addr = Some(pci_part.trim_start_matches("pci:").to_string());
            }
            break;
        }
    }

    let Some(pci_addr) = pci_addr else {
        return false;
    };

    // Now find USB buses associated with this PCI device
    // USB root hubs are at /sys/bus/usb/devices/usbN, and they link back to the PCI device
    for usb_dev in &usb_devices {
        if !usb_dev.starts_with("usb") {
            continue;
        }

        // Check if this root hub's parent PCI device matches
        let dev_path = format!("{}/{}", usb_base, usb_dev);
        let resolved = sysfs.path(&dev_path);
        if let Ok(canonical) = std::fs::canonicalize(&resolved) {
            let canonical_str = canonical.to_string_lossy();
            if canonical_str.contains(&pci_addr) {
                // This root hub belongs to our controller
                // Check if it has any child devices (beyond the root hub itself)
                let bus_num = usb_dev.trim_start_matches("usb");
                for other_dev in &usb_devices {
                    // Child devices have format: N-X or N-X.Y (where N is bus number)
                    if other_dev.starts_with(&format!("{}-", bus_num))
                        && !other_dev.contains(':')
                    {
                        return true;
                    }
                }
            }
        }
    }

    false
}
