use crate::sysfs::SysfsRoot;

#[derive(Debug, Clone, Default)]
pub struct CpuInfo {
    pub model_name: Option<String>,
    pub vendor: Option<String>,
    pub family: Option<u32>,
    pub model: Option<u32>,
    pub scaling_driver: Option<String>,
    pub governor: Option<String>,
    pub epp: Option<String>,
    pub epp_available: Vec<String>,
    pub online_cpus: u32,
    pub has_boost: bool,
    pub boost_enabled: bool,
}

impl CpuInfo {
    pub fn detect(sysfs: &SysfsRoot) -> Self {
        let mut info = Self::default();

        // Parse /proc/cpuinfo for model name and vendor
        if let Ok(cpuinfo) = sysfs.read("proc/cpuinfo") {
            for line in cpuinfo.lines() {
                if let Some((key, value)) = line.split_once(':') {
                    let key = key.trim();
                    let value = value.trim();
                    match key {
                        "model name" if info.model_name.is_none() => {
                            info.model_name = Some(value.to_string());
                        }
                        "vendor_id" if info.vendor.is_none() => {
                            info.vendor = Some(value.to_string());
                        }
                        "cpu family" if info.family.is_none() => {
                            info.family = value.parse().ok();
                        }
                        "model" if info.model.is_none() => {
                            info.model = value.parse().ok();
                        }
                        _ => {}
                    }
                }
            }
        }

        // Scaling driver from cpu0
        info.scaling_driver = sysfs
            .read_optional("sys/devices/system/cpu/cpu0/cpufreq/scaling_driver")
            .unwrap_or(None);

        // Governor from cpu0
        info.governor = sysfs
            .read_optional("sys/devices/system/cpu/cpu0/cpufreq/scaling_governor")
            .unwrap_or(None);

        // Energy Performance Preference
        info.epp = sysfs
            .read_optional(
                "sys/devices/system/cpu/cpu0/cpufreq/energy_performance_preference",
            )
            .unwrap_or(None);

        // Available EPP values
        if let Some(avail) = sysfs
            .read_optional(
                "sys/devices/system/cpu/cpu0/cpufreq/energy_performance_available_preferences",
            )
            .unwrap_or(None)
        {
            info.epp_available = avail.split_whitespace().map(String::from).collect();
        }

        // Count online CPUs
        if let Ok(entries) = sysfs.list_dir("sys/devices/system/cpu") {
            info.online_cpus = entries
                .iter()
                .filter(|e| e.starts_with("cpu") && e[3..].chars().all(|c| c.is_ascii_digit()))
                .count() as u32;
        }

        // Boost
        if let Some(val) = sysfs
            .read_optional("sys/devices/system/cpu/cpufreq/boost")
            .unwrap_or(None)
        {
            info.has_boost = true;
            info.boost_enabled = val == "1";
        }

        info
    }

    pub fn is_amd(&self) -> bool {
        self.vendor.as_deref() == Some("AuthenticAMD")
    }

    pub fn is_amd_pstate(&self) -> bool {
        self.scaling_driver
            .as_deref()
            .is_some_and(|d| d.starts_with("amd-pstate"))
    }

    pub fn is_zen4(&self) -> bool {
        // Zen 4: family 25 (0x19), models 0x60-0x7F (Phoenix/Ryzen 7040)
        self.is_amd() && self.family == Some(25) && self.model.is_some_and(|m| m >= 0x60)
    }
}
