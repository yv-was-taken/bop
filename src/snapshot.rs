use crate::sysfs::SysfsRoot;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

/// A complete capture of all sysfs/procfs paths that bop reads.
/// Can be serialized to JSON and used to recreate a mock sysfs tree for testing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    /// bop version that created this snapshot
    pub version: String,
    /// When this snapshot was taken
    pub timestamp: String,
    /// All captured file paths and their contents (relative paths, trimmed values)
    pub files: BTreeMap<String, String>,
    /// Directories that exist but are empty (needed for list_dir to work)
    pub dirs: Vec<String>,
}

/// Paths that detect modules and audit checks read.
/// Organized by subsystem for clarity.
const SINGLE_FILE_PATHS: &[&str] = &[
    // DMI
    "sys/class/dmi/id/board_vendor",
    "sys/class/dmi/id/board_name",
    "sys/class/dmi/id/product_name",
    "sys/class/dmi/id/product_family",
    "sys/class/dmi/id/bios_version",
    // CPU (global)
    "sys/devices/system/cpu/cpufreq/boost",
    "sys/devices/system/cpu/amd_pstate/status",
    // Platform / sleep
    "sys/firmware/acpi/platform_profile",
    "sys/firmware/acpi/platform_profile_choices",
    "sys/power/state",
    "sys/power/mem_sleep",
    // PCI ASPM
    "sys/module/pcie_aspm/parameters/policy",
    // Audio
    "sys/module/snd_hda_intel/parameters/power_save",
    "sys/module/snd_hda_intel/parameters/power_save_controller",
    // AMD GPU module param
    "sys/module/amdgpu/parameters/abmlevel",
    // Sysctl
    "proc/sys/kernel/nmi_watchdog",
    "proc/sys/vm/dirty_writeback_centisecs",
    // Proc
    "proc/cpuinfo",
    "proc/cmdline",
    "proc/acpi/wakeup",
];

impl Snapshot {
    /// Capture a snapshot from the real system (or a mock sysfs root).
    pub fn capture(sysfs: &SysfsRoot) -> Self {
        let mut files = BTreeMap::new();
        let mut dirs = Vec::new();

        // Capture single-file paths
        for path in SINGLE_FILE_PATHS {
            if let Some(val) = sysfs.read_optional(path).unwrap_or(None) {
                files.insert(path.to_string(), val);
            }
        }

        // Capture per-CPU entries
        capture_per_cpu(sysfs, &mut files);

        // Capture PCI devices
        capture_pci_devices(sysfs, &mut files, &mut dirs);

        // Capture USB devices
        capture_usb_devices(sysfs, &mut files, &mut dirs);

        // Capture DRM/GPU cards
        capture_drm(sysfs, &mut files, &mut dirs);

        // Capture backlight
        capture_backlight(sysfs, &mut files, &mut dirs);

        // Capture network interfaces
        capture_network(sysfs, &mut files, &mut dirs);

        // Capture battery/power supply
        capture_power_supply(sysfs, &mut files, &mut dirs);

        Self {
            version: env!("CARGO_PKG_VERSION").to_string(),
            timestamp: chrono_now(),
            files,
            dirs,
        }
    }

    /// Write this snapshot to a JSON file.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let json = serde_json::to_string_pretty(self).expect("snapshot serialization");
        fs::write(path, json)
    }

    /// Load a snapshot from a JSON file.
    pub fn load(path: &Path) -> std::io::Result<Self> {
        let json = fs::read_to_string(path)?;
        serde_json::from_str(&json)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }

    /// Materialize this snapshot as a mock sysfs tree in the given directory.
    /// Returns a `SysfsRoot` pointing at it.
    pub fn materialize(&self, root: &Path) -> std::io::Result<SysfsRoot> {
        for dir in &self.dirs {
            fs::create_dir_all(root.join(dir))?;
        }
        for (path, content) in &self.files {
            let full_path = root.join(path);
            if let Some(parent) = full_path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&full_path, format!("{}\n", content))?;
        }
        Ok(SysfsRoot::new(root))
    }
}

fn capture_per_cpu(sysfs: &SysfsRoot, files: &mut BTreeMap<String, String>) {
    let cpu_base = "sys/devices/system/cpu";
    let entries = match sysfs.list_dir(cpu_base) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in &entries {
        if !entry.starts_with("cpu") || !entry[3..].chars().all(|c| c.is_ascii_digit()) {
            continue;
        }

        let cpufreq = format!("{}/{}/cpufreq", cpu_base, entry);
        for file in &[
            "scaling_driver",
            "scaling_governor",
            "energy_performance_preference",
            "energy_performance_available_preferences",
        ] {
            let path = format!("{}/{}", cpufreq, file);
            if let Some(val) = sysfs.read_optional(&path).unwrap_or(None) {
                files.insert(path, val);
            }
        }
    }
}

fn capture_pci_devices(
    sysfs: &SysfsRoot,
    files: &mut BTreeMap<String, String>,
    dirs: &mut Vec<String>,
) {
    let pci_base = "sys/bus/pci/devices";
    let entries = match sysfs.list_dir(pci_base) {
        Ok(e) => e,
        Err(_) => return,
    };

    for addr in &entries {
        let base = format!("{}/{}", pci_base, addr);
        dirs.push(format!("{}/power", base));

        for file in &[
            "class",
            "vendor",
            "device",
            "power/control",
            "power/runtime_status",
        ] {
            let path = format!("{}/{}", base, file);
            if let Some(val) = sysfs.read_optional(&path).unwrap_or(None) {
                files.insert(path, val);
            }
        }

        // Capture driver name from symlink
        let driver_path = sysfs.path(format!("{}/driver", base));
        if let Ok(target) = fs::read_link(&driver_path)
            && let Some(name) = target.file_name()
        {
            files.insert(
                format!("{}/__driver_name", base),
                name.to_string_lossy().to_string(),
            );
        }
    }
}

fn capture_usb_devices(
    sysfs: &SysfsRoot,
    files: &mut BTreeMap<String, String>,
    dirs: &mut Vec<String>,
) {
    let usb_base = "sys/bus/usb/devices";
    let entries = match sysfs.list_dir(usb_base) {
        Ok(e) => e,
        Err(_) => return,
    };

    for device in &entries {
        if device.contains(':') {
            continue; // skip interfaces
        }
        let base = format!("{}/{}", usb_base, device);
        dirs.push(format!("{}/power", base));

        for file in &[
            "power/control",
            "product",
            "manufacturer",
            "idVendor",
            "idProduct",
        ] {
            let path = format!("{}/{}", base, file);
            if let Some(val) = sysfs.read_optional(&path).unwrap_or(None) {
                files.insert(path, val);
            }
        }
    }
}

fn capture_drm(sysfs: &SysfsRoot, files: &mut BTreeMap<String, String>, dirs: &mut Vec<String>) {
    let drm_base = "sys/class/drm";
    let entries = match sysfs.list_dir(drm_base) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in &entries {
        let base = format!("{}/{}", drm_base, entry);

        if entry.starts_with("card") && !entry.contains('-') {
            // Card device — capture GPU info
            let device = format!("{}/device", base);
            dirs.push(device.clone());

            for file in &["vendor", "power_state", "power_dpm_force_performance_level"] {
                let path = format!("{}/{}", device, file);
                if let Some(val) = sysfs.read_optional(&path).unwrap_or(None) {
                    files.insert(path, val);
                }
            }

            // Driver name from symlink
            let driver_path = sysfs.path(format!("{}/driver", device));
            if let Ok(target) = fs::read_link(&driver_path)
                && let Some(name) = target.file_name()
            {
                files.insert(
                    format!("{}/__driver_name", device),
                    name.to_string_lossy().to_string(),
                );
            }
        } else if entry.contains('-') {
            // Connector — capture status
            let status_path = format!("{}/status", base);
            if let Some(val) = sysfs.read_optional(&status_path).unwrap_or(None) {
                files.insert(status_path, val);
            }
        }
    }
}

fn capture_backlight(
    sysfs: &SysfsRoot,
    files: &mut BTreeMap<String, String>,
    dirs: &mut Vec<String>,
) {
    let bl_base = "sys/class/backlight";
    let entries = match sysfs.list_dir(bl_base) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in &entries {
        let base = format!("{}/{}", bl_base, entry);
        dirs.push(base.clone());

        for file in &["brightness", "max_brightness", "actual_brightness"] {
            let path = format!("{}/{}", base, file);
            if let Some(val) = sysfs.read_optional(&path).unwrap_or(None) {
                files.insert(path, val);
            }
        }
    }
}

fn capture_network(
    sysfs: &SysfsRoot,
    files: &mut BTreeMap<String, String>,
    dirs: &mut Vec<String>,
) {
    let net_base = "sys/class/net";
    let entries = match sysfs.list_dir(net_base) {
        Ok(e) => e,
        Err(_) => return,
    };

    for iface in &entries {
        let wireless_path = format!("{}/{}/wireless", net_base, iface);
        if sysfs.exists(&wireless_path) {
            dirs.push(wireless_path);

            // Driver name from symlink
            let driver_path = sysfs.path(format!("{}/{}/device/driver", net_base, iface));
            if let Ok(target) = fs::read_link(&driver_path)
                && let Some(name) = target.file_name()
            {
                files.insert(
                    format!("{}/{}/__wifi_driver", net_base, iface),
                    name.to_string_lossy().to_string(),
                );
            }
        }
    }
}

fn capture_power_supply(
    sysfs: &SysfsRoot,
    files: &mut BTreeMap<String, String>,
    dirs: &mut Vec<String>,
) {
    let ps_base = "sys/class/power_supply";
    let entries = match sysfs.list_dir(ps_base) {
        Ok(e) => e,
        Err(_) => return,
    };

    for supply in &entries {
        let base = format!("{}/{}", ps_base, supply);
        dirs.push(base.clone());

        for file in &[
            "type",
            "online",
            "present",
            "status",
            "capacity",
            "energy_now",
            "energy_full",
            "energy_full_design",
            "power_now",
            "charge_now",
            "charge_full",
            "charge_full_design",
            "current_now",
            "voltage_now",
            "cycle_count",
        ] {
            let path = format!("{}/{}", base, file);
            if let Some(val) = sysfs.read_optional(&path).unwrap_or(None) {
                files.insert(path, val);
            }
        }
    }
}

fn chrono_now() -> String {
    // Simple timestamp without requiring chrono crate
    let output = std::process::Command::new("date")
        .arg("--iso-8601=seconds")
        .output();
    match output {
        Ok(o) => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        Err(_) => "unknown".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_snapshot_round_trip() {
        // Create a minimal mock sysfs
        let src = TempDir::new().unwrap();
        let dmi = src.path().join("sys/class/dmi/id");
        fs::create_dir_all(&dmi).unwrap();
        fs::write(dmi.join("board_vendor"), "TestVendor\n").unwrap();
        fs::write(dmi.join("product_name"), "TestLaptop\n").unwrap();

        let bat = src.path().join("sys/class/power_supply/BAT0");
        fs::create_dir_all(&bat).unwrap();
        fs::write(bat.join("type"), "Battery\n").unwrap();
        fs::write(bat.join("present"), "1\n").unwrap();
        fs::write(bat.join("capacity"), "85\n").unwrap();

        let proc = src.path().join("proc");
        fs::create_dir_all(&proc).unwrap();
        fs::write(
            proc.join("cpuinfo"),
            "processor\t: 0\nvendor_id\t: GenuineIntel\n",
        )
        .unwrap();
        fs::write(proc.join("cmdline"), "\n").unwrap();

        // Capture snapshot
        let sysfs = SysfsRoot::new(src.path());
        let snap = Snapshot::capture(&sysfs);

        assert_eq!(
            snap.files.get("sys/class/dmi/id/board_vendor"),
            Some(&"TestVendor".to_string())
        );
        assert_eq!(
            snap.files.get("sys/class/power_supply/BAT0/capacity"),
            Some(&"85".to_string())
        );

        // Save to JSON
        let json_path = src.path().join("snapshot.json");
        snap.save(&json_path).unwrap();

        // Load back
        let loaded = Snapshot::load(&json_path).unwrap();
        assert_eq!(loaded.files, snap.files);

        // Materialize into a new directory
        let dst = TempDir::new().unwrap();
        let new_sysfs = loaded.materialize(dst.path()).unwrap();

        // Verify the materialized tree works with detect
        let hw = crate::detect::HardwareInfo::detect(&new_sysfs);
        assert_eq!(hw.dmi.board_vendor.as_deref(), Some("TestVendor"));
        assert_eq!(hw.dmi.product_name.as_deref(), Some("TestLaptop"));
        assert!(hw.battery.present);
        assert_eq!(hw.battery.capacity_percent, Some(85));
    }
}
