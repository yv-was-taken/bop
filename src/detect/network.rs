use crate::sysfs::SysfsRoot;

#[derive(Debug, Clone, Default)]
pub struct NetworkInfo {
    pub wifi_interface: Option<String>,
    pub wifi_driver: Option<String>,
    pub wifi_power_save: Option<bool>,
}

impl NetworkInfo {
    pub fn detect(sysfs: &SysfsRoot) -> Self {
        let mut info = Self::default();

        // Find wireless interface
        let net_base = "sys/class/net";
        if let Ok(entries) = sysfs.list_dir(net_base) {
            for iface in entries {
                // Check if it's wireless by looking for the wireless/ subdir
                let wireless_path = format!("{}/{}/wireless", net_base, iface);
                if sysfs.exists(&wireless_path) {
                    info.wifi_interface = Some(iface.clone());

                    // Read driver
                    let driver_path = sysfs.path(format!("{}/{}/device/driver", net_base, iface));
                    if let Ok(target) = std::fs::read_link(&driver_path)
                        && let Some(name) = target.file_name()
                    {
                        info.wifi_driver = name.to_str().map(String::from);
                    }

                    break;
                }
            }
        }

        // WiFi power save status requires `iw` -- we'll check it at runtime
        // during audit rather than detection, since it requires a subprocess call

        info
    }

    pub fn is_mediatek(&self) -> bool {
        self.wifi_driver
            .as_deref()
            .is_some_and(|d| d.starts_with("mt7"))
    }
}
