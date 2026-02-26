use crate::apply::sysfs_writer;
use crate::error::{Error, Result};
use crate::sysfs::SysfsRoot;
use colored::Colorize;

#[derive(Debug, Clone)]
pub struct WakeController {
    pub name: String,
    pub pci_address: Option<String>,
    pub enabled: bool,
    pub has_devices: bool,
    pub device_descriptions: Vec<String>,
}

/// Framework 16 USB host controllers use the XHC* naming convention in ACPI.
/// Other ACPI wake sources (LID0, PBTN, GPP6, NHI0, etc.) are not USB
/// controllers and should not be toggled by wake scan.
fn is_usb_wakeup_source(name: &str) -> bool {
    name.starts_with("XHC")
}

/// Whether scan should disable this controller's wake capability.
/// XHC0 is exempt because it is the primary USB controller (keyboard/trackpad).
fn should_disable_in_scan(ctrl: &WakeController) -> bool {
    is_usb_wakeup_source(&ctrl.name) && !ctrl.has_devices && ctrl.enabled && ctrl.name != "XHC0"
}

/// Whether scan should enable this controller's wake capability.
fn should_enable_in_scan(ctrl: &WakeController) -> bool {
    is_usb_wakeup_source(&ctrl.name) && ctrl.has_devices && !ctrl.enabled
}

/// List all USB controllers and their wakeup status.
pub fn list() -> Result<()> {
    let sysfs = SysfsRoot::system();
    let controllers = scan_controllers(&sysfs)?;

    println!("{}", " Wake Sources".bold());
    println!();

    for ctrl in &controllers {
        let wake_badge = if ctrl.enabled {
            "enabled".green().to_string()
        } else {
            "disabled".dimmed().to_string()
        };

        let addr = ctrl.pci_address.as_deref().unwrap_or("N/A");

        print!(
            "  {} {}  {}",
            format!("{:<5}", ctrl.name).bold(),
            wake_badge,
            addr.dimmed()
        );

        if ctrl.has_devices {
            print!("  {}", ctrl.device_descriptions.join(", "));
        }

        println!();
    }

    println!();

    // Warn about disabled controllers with devices
    for ctrl in &controllers {
        if !ctrl.enabled && ctrl.has_devices {
            println!(
                "  {} {} has connected devices but wake is disabled!",
                "WARNING:".yellow().bold(),
                ctrl.name
            );
            println!(
                "    Run `bop wake enable {}` to allow these devices to wake the system.",
                ctrl.name
            );
        }
    }

    // Note about expansion cards
    let disabled_empty: Vec<_> = controllers
        .iter()
        .filter(|c| !c.enabled && !c.has_devices && is_usb_wakeup_source(&c.name))
        .collect();
    if !disabled_empty.is_empty() {
        let names: Vec<_> = disabled_empty.iter().map(|c| c.name.as_str()).collect();
        println!();
        println!(
            "  {} Wake disabled on {} expansion card USB controllers ({}).",
            "NOTICE:".cyan(),
            names.len(),
            names.join(", ")
        );
        println!(
            "    USB expansion cards plugged into these ports will NOT wake the system from sleep."
        );
        println!(
            "    Run `bop wake enable <controller>` to re-enable, or `bop wake scan` to auto-detect."
        );
    }

    Ok(())
}

/// Enable wakeup for a controller.
pub fn enable(controller: &str) -> Result<()> {
    if !nix::unistd::geteuid().is_root() {
        return Err(Error::NotRoot {
            operation: "wake enable".to_string(),
        });
    }

    let sysfs = SysfsRoot::system();

    // Check if controller exists
    let wakeup = sysfs.read("proc/acpi/wakeup")?;
    if !wakeup.lines().any(|l| l.starts_with(controller)) {
        return Err(Error::Other(format!(
            "Controller '{}' not found in /proc/acpi/wakeup",
            controller
        )));
    }

    // Check current state
    let is_enabled = wakeup
        .lines()
        .any(|l| l.starts_with(controller) && l.contains("*enabled"));

    if is_enabled {
        println!("{} is already enabled.", controller);
        return Ok(());
    }

    sysfs_writer::toggle_acpi_wakeup(controller)?;
    println!(
        "{} Wake {} for {}",
        "OK".green().bold(),
        "enabled".green(),
        controller
    );
    println!(
        "  {}",
        "Note: This is volatile and resets on reboot. Run `bop apply` to persist.".dimmed()
    );

    Ok(())
}

/// Disable wakeup for a controller.
pub fn disable(controller: &str) -> Result<()> {
    if !nix::unistd::geteuid().is_root() {
        return Err(Error::NotRoot {
            operation: "wake disable".to_string(),
        });
    }

    let sysfs = SysfsRoot::system();

    let wakeup = sysfs.read("proc/acpi/wakeup")?;
    if !wakeup.lines().any(|l| l.starts_with(controller)) {
        return Err(Error::Other(format!(
            "Controller '{}' not found in /proc/acpi/wakeup",
            controller
        )));
    }

    let is_enabled = wakeup
        .lines()
        .any(|l| l.starts_with(controller) && l.contains("*enabled"));

    if !is_enabled {
        println!("{} is already disabled.", controller);
        return Ok(());
    }

    sysfs_writer::toggle_acpi_wakeup(controller)?;
    println!(
        "{} Wake {} for {}",
        "OK".green().bold(),
        "disabled".yellow(),
        controller
    );

    Ok(())
}

/// Scan all controllers and auto-enable those with connected devices.
pub fn scan() -> Result<()> {
    if !nix::unistd::geteuid().is_root() {
        return Err(Error::NotRoot {
            operation: "wake scan".to_string(),
        });
    }

    let sysfs = SysfsRoot::system();
    let controllers = scan_controllers(&sysfs)?;

    println!("{}", "Scanning USB controllers...".bold());
    println!();

    let mut changes = 0;

    for ctrl in &controllers {
        if should_enable_in_scan(ctrl) {
            println!(
                "  {} has connected devices, enabling wake...",
                ctrl.name.bold()
            );
            sysfs_writer::toggle_acpi_wakeup(&ctrl.name)?;
            changes += 1;
        } else if should_disable_in_scan(ctrl) {
            println!(
                "  {} has no connected devices, disabling wake...",
                ctrl.name.bold()
            );
            sysfs_writer::toggle_acpi_wakeup(&ctrl.name)?;
            changes += 1;
        }
    }

    if changes == 0 {
        println!("  No changes needed.");
    } else {
        println!();
        println!("{} {} controllers updated.", "OK".green().bold(), changes);
    }

    Ok(())
}

/// Scan all controllers and detect connected devices.
fn scan_controllers(sysfs: &SysfsRoot) -> Result<Vec<WakeController>> {
    let wakeup_content = sysfs.read("proc/acpi/wakeup")?;
    let mut controllers = Vec::new();

    let usb_devices = sysfs.list_dir("sys/bus/usb/devices").unwrap_or_default();

    for line in wakeup_content.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 3 {
            continue;
        }

        let name = parts[0].to_string();

        // Only process USB host controllers (XHC*) and other notable sources
        let is_usb_controller = is_usb_wakeup_source(&name);

        let enabled = line.contains("*enabled");

        let pci_address = parts
            .iter()
            .find(|p| p.starts_with("pci:"))
            .map(|p| p.trim_start_matches("pci:").to_string());

        let (has_devices, device_descriptions) = if is_usb_controller {
            find_usb_devices_for_controller(&name, &pci_address, &usb_devices, sysfs)
        } else {
            (false, Vec::new())
        };

        controllers.push(WakeController {
            name,
            pci_address,
            enabled,
            has_devices,
            device_descriptions,
        });
    }

    Ok(controllers)
}

/// Find USB devices connected through a specific controller.
fn find_usb_devices_for_controller(
    _controller_name: &str,
    pci_address: &Option<String>,
    usb_devices: &[String],
    sysfs: &SysfsRoot,
) -> (bool, Vec<String>) {
    let Some(pci_addr) = pci_address else {
        return (false, Vec::new());
    };

    let mut descriptions = Vec::new();

    // Find root hubs that belong to this PCI address
    for usb_dev in usb_devices {
        if !usb_dev.starts_with("usb") {
            continue;
        }

        let dev_path = format!("sys/bus/usb/devices/{}", usb_dev);
        let resolved = sysfs.path(&dev_path);
        let Ok(canonical) = std::fs::canonicalize(&resolved) else {
            continue;
        };

        if !canonical.to_string_lossy().contains(pci_addr.as_str()) {
            continue;
        }

        // This root hub belongs to our controller -- find child devices
        let bus_num = usb_dev.trim_start_matches("usb");
        for other_dev in usb_devices {
            if other_dev.starts_with(&format!("{}-", bus_num)) && !other_dev.contains(':') {
                // This is a real USB device
                let product = sysfs
                    .read_optional(format!("sys/bus/usb/devices/{}/product", other_dev))
                    .unwrap_or(None);
                let manufacturer = sysfs
                    .read_optional(format!("sys/bus/usb/devices/{}/manufacturer", other_dev))
                    .unwrap_or(None);

                let desc = match (manufacturer, product) {
                    (Some(mfg), Some(prod)) => format!("{} {}", mfg, prod),
                    (None, Some(prod)) => prod,
                    (Some(mfg), None) => mfg,
                    (None, None) => other_dev.clone(),
                };
                descriptions.push(desc);
            }
        }
    }

    let has_devices = !descriptions.is_empty();
    (has_devices, descriptions)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn create_wakeup_fixture(root: &Path) {
        fs::create_dir_all(root.join("proc/acpi")).unwrap();
        fs::create_dir_all(root.join("sys/bus/usb/devices")).unwrap();

        let wakeup_content = "\
XHC0\tS3\t*enabled\tpci:0000:c1:00.3
XHC1\tS3\t*enabled\tpci:0000:c1:00.4
GPP6\tS4\t*enabled\tpci:0000:00:02.2
NHI0\tS4\t*enabled\tpci:0000:c3:00.5
LID0\tS4\t*enabled\tplatform:PNP0C0D:00
PBTN\tS4\t*enabled\tplatform:PNP0C0C:00
SLPB\tS3\t*enabled\tplatform:PNP0C0E:00
";
        fs::write(root.join("proc/acpi/wakeup"), wakeup_content).unwrap();
    }

    fn create_mixed_state_fixture(root: &Path) {
        fs::create_dir_all(root.join("proc/acpi")).unwrap();
        fs::create_dir_all(root.join("sys/bus/usb/devices")).unwrap();

        // XHC2: disabled USB controller — should be enabled by scan since it has devices
        // GPP6: disabled non-USB source — must NOT be enabled by scan
        // PBTN: disabled non-USB source — must NOT be enabled by scan
        let wakeup_content = "\
XHC0\tS3\t*enabled\tpci:0000:c1:00.3
XHC2\tS3\t*disabled\tpci:0000:c1:00.5
GPP6\tS4\t*disabled\tpci:0000:00:02.2
PBTN\tS4\t*disabled\tplatform:PNP0C0C:00
";
        fs::write(root.join("proc/acpi/wakeup"), wakeup_content).unwrap();

        // Create a USB device on XHC2's bus so it has_devices = true
        let pci_dir = root.join("sys/devices/pci0000:00/0000:c1:00.5/usb3");
        fs::create_dir_all(&pci_dir).unwrap();
        let usb_link = root.join("sys/bus/usb/devices/usb3");
        std::os::unix::fs::symlink(&pci_dir, &usb_link).unwrap();

        let child_dev = root.join("sys/devices/pci0000:00/0000:c1:00.5/usb3/3-1");
        fs::create_dir_all(&child_dev).unwrap();
        fs::write(child_dev.join("product"), "Test Device\n").unwrap();
        let child_link = root.join("sys/bus/usb/devices/3-1");
        std::os::unix::fs::symlink(&child_dev, &child_link).unwrap();
    }

    #[test]
    fn scan_enable_filter_excludes_non_usb_wake_sources() {
        let tmp = TempDir::new().unwrap();
        create_mixed_state_fixture(tmp.path());

        let sysfs = SysfsRoot::new(tmp.path());
        let controllers = scan_controllers(&sysfs).unwrap();

        let enable_candidates: Vec<&str> = controllers
            .iter()
            .filter(|c| should_enable_in_scan(c))
            .map(|c| c.name.as_str())
            .collect();

        // Only XHC2 should be enabled (disabled USB controller with devices)
        assert_eq!(enable_candidates, vec!["XHC2"]);

        // Non-USB sources must not be enabled even though they are disabled
        for source in ["GPP6", "PBTN"] {
            let ctrl = controllers
                .iter()
                .find(|c| c.name == source)
                .expect("fixture source should be present");
            assert!(!ctrl.enabled);
            assert!(!should_enable_in_scan(ctrl));
        }
    }

    #[test]
    fn scan_disable_filter_excludes_non_usb_wake_sources() {
        let tmp = TempDir::new().unwrap();
        create_wakeup_fixture(tmp.path());

        let sysfs = SysfsRoot::new(tmp.path());
        let controllers = scan_controllers(&sysfs).unwrap();

        let disable_candidates: Vec<&str> = controllers
            .iter()
            .filter(|c| should_disable_in_scan(c))
            .map(|c| c.name.as_str())
            .collect();

        assert_eq!(disable_candidates, vec!["XHC1"]);

        for source in ["GPP6", "NHI0", "LID0", "PBTN", "SLPB"] {
            let ctrl = controllers
                .iter()
                .find(|c| c.name == source)
                .expect("fixture source should be present");
            assert!(ctrl.enabled);
            assert!(!should_disable_in_scan(ctrl));
        }
    }
}
