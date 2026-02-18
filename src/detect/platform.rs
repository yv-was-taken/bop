use crate::sysfs::SysfsRoot;

#[derive(Debug, Clone, Default)]
pub struct PlatformInfo {
    pub platform_profile: Option<String>,
    pub platform_profiles_available: Vec<String>,
    pub sleep_state: Option<String>,
    pub sleep_states_available: Vec<String>,
    pub mem_sleep: Option<String>,
    pub acpi_wakeup_sources: Vec<AcpiWakeupSource>,
}

#[derive(Debug, Clone)]
pub struct AcpiWakeupSource {
    pub device: String,
    pub sysfs_node: Option<String>,
    pub status: String, // "enabled" or "disabled"
    pub enabled: bool,
}

impl PlatformInfo {
    pub fn detect(sysfs: &SysfsRoot) -> Self {
        let mut info = Self::default();

        // Platform profile
        info.platform_profile = sysfs
            .read_optional("sys/firmware/acpi/platform_profile")
            .unwrap_or(None);

        if let Some(avail) = sysfs
            .read_optional("sys/firmware/acpi/platform_profile_choices")
            .unwrap_or(None)
        {
            info.platform_profiles_available =
                avail.split_whitespace().map(String::from).collect();
        }

        // Sleep states
        if let Some(sleep) = sysfs.read_optional("sys/power/state").unwrap_or(None) {
            info.sleep_states_available = sleep.split_whitespace().map(String::from).collect();
        }

        if let Some(mem_sleep) = sysfs.read_optional("sys/power/mem_sleep").unwrap_or(None) {
            // Format: s2idle [deep]
            for word in mem_sleep.split_whitespace() {
                if word.starts_with('[') && word.ends_with(']') {
                    info.mem_sleep = Some(word[1..word.len() - 1].to_string());
                }
            }
            if info.mem_sleep.is_none() {
                // If no brackets, first entry is current
                info.mem_sleep = mem_sleep
                    .split_whitespace()
                    .next()
                    .map(String::from);
            }
        }

        // ACPI wakeup sources
        if let Ok(wakeup) = sysfs.read("proc/acpi/wakeup") {
            for line in wakeup.lines() {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 3 {
                    let device = parts[0].to_string();
                    // Format: DEVICE  S-STATE  STATUS  SYSFS_NODE
                    let status = if parts.iter().any(|p| *p == "*enabled") {
                        "enabled".to_string()
                    } else if parts.iter().any(|p| *p == "*disabled") {
                        "disabled".to_string()
                    } else {
                        // Try to find enabled/disabled in the parts
                        parts
                            .iter()
                            .find(|p| **p == "enabled" || **p == "disabled")
                            .map(|s| s.to_string())
                            .unwrap_or_default()
                    };

                    let sysfs_node = parts.last().and_then(|p| {
                        if p.starts_with("pci:") || p.starts_with("platform:") {
                            Some(p.to_string())
                        } else {
                            None
                        }
                    });

                    let enabled = status.contains("enabled");

                    info.acpi_wakeup_sources.push(AcpiWakeupSource {
                        device,
                        sysfs_node,
                        status,
                        enabled,
                    });
                }
            }
        }

        info
    }

    pub fn has_s2idle(&self) -> bool {
        self.sleep_states_available.iter().any(|s| s == "mem")
            && self.mem_sleep.as_deref() == Some("s2idle")
    }
}
