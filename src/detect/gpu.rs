use crate::sysfs::SysfsRoot;

#[derive(Debug, Clone, Default)]
pub struct GpuInfo {
    pub vendor: Option<String>,
    pub driver: Option<String>,
    pub card_path: Option<String>,
    pub dpm_level: Option<String>,
    pub abm_level: Option<u32>,
    pub has_abm: bool,
}

impl GpuInfo {
    pub fn detect(sysfs: &SysfsRoot) -> Self {
        let mut info = Self::default();

        // Find the first DRM card
        if let Ok(entries) = sysfs.list_dir("sys/class/drm") {
            for entry in &entries {
                if entry.starts_with("card") && !entry.contains('-') {
                    let card_path = format!("sys/class/drm/{}/device", entry);
                    if sysfs.exists(&card_path) {
                        info.card_path = Some(card_path.clone());

                        // Read vendor
                        if let Some(vendor) = sysfs
                            .read_optional(format!("{}/vendor", card_path))
                            .unwrap_or(None)
                        {
                            info.vendor = Some(vendor);
                        }

                        // Read driver (follow the symlink)
                        let driver_path = sysfs.path(format!("{}/driver", card_path));
                        if let Ok(target) = std::fs::read_link(&driver_path) {
                            if let Some(name) = target.file_name() {
                                info.driver = name.to_str().map(String::from);
                            }
                        }

                        break;
                    }
                }
            }
        }

        // AMD GPU specific: DPM level and ABM
        if info.is_amd() {
            if let Some(ref card_path) = info.card_path {
                info.dpm_level = sysfs
                    .read_optional(format!("{}/power_dpm_force_performance_level", card_path))
                    .unwrap_or(None);
            }

            // Check kernel cmdline for amdgpu.abmlevel
            if let Ok(cmdline) = sysfs.read("proc/cmdline") {
                for param in cmdline.split_whitespace() {
                    if let Some(val) = param.strip_prefix("amdgpu.abmlevel=") {
                        info.abm_level = val.parse().ok();
                        info.has_abm = true;
                    }
                }
            }

            // Also check module parameter
            if let Some(val) = sysfs
                .read_optional("sys/module/amdgpu/parameters/abmlevel")
                .unwrap_or(None)
            {
                if !info.has_abm {
                    info.abm_level = val.parse().ok();
                    // ABM is available if the module parameter file exists
                    info.has_abm = true;
                }
            }
        }

        info
    }

    pub fn is_amd(&self) -> bool {
        self.vendor.as_deref() == Some("0x1002") || self.driver.as_deref() == Some("amdgpu")
    }
}
