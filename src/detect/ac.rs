use crate::sysfs::SysfsRoot;

/// AC adapter (mains power) detection.
#[derive(Debug, Clone, Default)]
pub struct AcInfo {
    pub found: bool,
    pub supply_name: Option<String>,
    pub online: bool,
}

impl AcInfo {
    pub fn detect(sysfs: &SysfsRoot) -> Self {
        let mut info = Self::default();

        let ps_base = "sys/class/power_supply";
        let entries = match sysfs.list_dir(ps_base) {
            Ok(e) => e,
            Err(_) => return info,
        };

        for name in &entries {
            let base = format!("{}/{}", ps_base, name);

            let ptype = sysfs
                .read_optional(format!("{}/type", base))
                .unwrap_or(None);
            if ptype.as_deref() != Some("Mains") {
                continue;
            }

            info.found = true;
            info.supply_name = Some(name.clone());
            info.online = sysfs
                .read_optional(format!("{}/online", base))
                .unwrap_or(None)
                .as_deref()
                == Some("1");
            break;
        }

        info
    }

    pub fn is_on_ac(&self) -> bool {
        self.found && self.online
    }

    pub fn is_on_battery(&self) -> bool {
        self.found && !self.online
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn make_sysfs(root: &std::path::Path, name: &str, ptype: &str, online: &str) {
        let dir = root.join(format!("sys/class/power_supply/{}", name));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("type"), format!("{}\n", ptype)).unwrap();
        fs::write(dir.join("online"), format!("{}\n", online)).unwrap();
    }

    #[test]
    fn test_mains_online() {
        let tmp = TempDir::new().unwrap();
        make_sysfs(tmp.path(), "ACAD", "Mains", "1");
        let sysfs = SysfsRoot::new(tmp.path());
        let ac = AcInfo::detect(&sysfs);
        assert!(ac.found);
        assert!(ac.online);
        assert!(ac.is_on_ac());
        assert!(!ac.is_on_battery());
        assert_eq!(ac.supply_name.as_deref(), Some("ACAD"));
    }

    #[test]
    fn test_mains_offline() {
        let tmp = TempDir::new().unwrap();
        make_sysfs(tmp.path(), "ACAD", "Mains", "0");
        let sysfs = SysfsRoot::new(tmp.path());
        let ac = AcInfo::detect(&sysfs);
        assert!(ac.found);
        assert!(!ac.online);
        assert!(!ac.is_on_ac());
        assert!(ac.is_on_battery());
    }

    #[test]
    fn test_no_adapter() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir_all(tmp.path().join("sys/class/power_supply")).unwrap();
        let sysfs = SysfsRoot::new(tmp.path());
        let ac = AcInfo::detect(&sysfs);
        assert!(!ac.found);
        assert!(!ac.is_on_ac());
        assert!(!ac.is_on_battery());
    }

    #[test]
    fn test_alternate_names() {
        for name in &["ADP1", "AC0", "AC"] {
            let tmp = TempDir::new().unwrap();
            make_sysfs(tmp.path(), name, "Mains", "1");
            let sysfs = SysfsRoot::new(tmp.path());
            let ac = AcInfo::detect(&sysfs);
            assert!(ac.found, "should detect adapter named {}", name);
            assert!(ac.is_on_ac());
            assert_eq!(ac.supply_name.as_deref(), Some(*name));
        }
    }

    #[test]
    fn test_battery_only_no_mains() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("sys/class/power_supply/BAT0");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("type"), "Battery\n").unwrap();
        let sysfs = SysfsRoot::new(tmp.path());
        let ac = AcInfo::detect(&sysfs);
        assert!(!ac.found);
        assert!(!ac.is_on_ac());
        assert!(!ac.is_on_battery());
    }
}
