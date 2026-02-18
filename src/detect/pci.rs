use crate::sysfs::SysfsRoot;

#[derive(Debug, Clone)]
pub struct PciDevice {
    pub address: String,
    pub class: Option<String>,
    pub vendor: Option<String>,
    pub device: Option<String>,
    pub driver: Option<String>,
    pub runtime_pm: Option<String>,
    pub runtime_status: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct PciInfo {
    pub devices: Vec<PciDevice>,
    pub aspm_policy: Option<String>,
    pub aspm_policies_available: Vec<String>,
}

impl PciInfo {
    pub fn detect(sysfs: &SysfsRoot) -> Self {
        let mut info = Self::default();

        // ASPM policy
        if let Some(policy_str) = sysfs
            .read_optional("sys/module/pcie_aspm/parameters/policy")
            .unwrap_or(None)
        {
            // Format is like: default performance [powersave] powersupersave
            // where the active one is in brackets
            for word in policy_str.split_whitespace() {
                if word.starts_with('[') && word.ends_with(']') {
                    info.aspm_policy = Some(word[1..word.len() - 1].to_string());
                    info.aspm_policies_available
                        .push(word[1..word.len() - 1].to_string());
                } else {
                    info.aspm_policies_available.push(word.to_string());
                }
            }
        }

        // Enumerate PCI devices
        let pci_base = "sys/bus/pci/devices";
        if let Ok(entries) = sysfs.list_dir(pci_base) {
            for addr in entries {
                let base = format!("{}/{}", pci_base, addr);
                let class = sysfs
                    .read_optional(format!("{}/class", base))
                    .unwrap_or(None);
                let vendor = sysfs
                    .read_optional(format!("{}/vendor", base))
                    .unwrap_or(None);
                let device = sysfs
                    .read_optional(format!("{}/device", base))
                    .unwrap_or(None);
                let runtime_pm = sysfs
                    .read_optional(format!("{}/power/control", base))
                    .unwrap_or(None);
                let runtime_status = sysfs
                    .read_optional(format!("{}/power/runtime_status", base))
                    .unwrap_or(None);

                // Read driver by following symlink
                let driver_path = sysfs.path(format!("{}/driver", base));
                let driver = std::fs::read_link(&driver_path)
                    .ok()
                    .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()));

                info.devices.push(PciDevice {
                    address: addr,
                    class,
                    vendor,
                    device,
                    driver,
                    runtime_pm,
                    runtime_status,
                });
            }
        }

        info
    }

    /// Count devices not using runtime power management
    pub fn devices_without_runtime_pm(&self) -> Vec<&PciDevice> {
        self.devices
            .iter()
            .filter(|d| d.runtime_pm.as_deref() != Some("auto"))
            .collect()
    }
}
