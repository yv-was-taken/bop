use crate::sysfs::SysfsRoot;

#[derive(Debug, Clone, Default)]
pub struct BatteryInfo {
    pub present: bool,
    pub status: Option<String>,
    pub capacity_percent: Option<u32>,
    pub energy_now_uwh: Option<u64>,
    pub energy_full_uwh: Option<u64>,
    pub energy_full_design_uwh: Option<u64>,
    pub power_now_uw: Option<u64>,
    pub voltage_now_uv: Option<u64>,
    pub cycle_count: Option<u32>,
    pub health_percent: Option<f64>,
    pub supply_name: Option<String>,
}

impl BatteryInfo {
    pub fn detect(sysfs: &SysfsRoot) -> Self {
        let mut info = Self::default();

        // Find power supply
        let ps_base = "sys/class/power_supply";
        let entries = match sysfs.list_dir(ps_base) {
            Ok(e) => e,
            Err(_) => return info,
        };

        // Look for a battery (BAT0, BAT1, etc.)
        let bat_name = entries.iter().find(|e| e.starts_with("BAT"));
        let bat_name = match bat_name {
            Some(n) => n.clone(),
            None => return info,
        };

        info.supply_name = Some(bat_name.clone());
        let base = format!("{}/{}", ps_base, bat_name);

        // Check type
        if let Some(ptype) = sysfs.read_optional(format!("{}/type", base)).unwrap_or(None) {
            if ptype != "Battery" {
                return info;
            }
        }

        info.present = sysfs
            .read_optional(format!("{}/present", base))
            .unwrap_or(None)
            .as_deref()
            == Some("1");

        info.status = sysfs.read_optional(format!("{}/status", base)).unwrap_or(None);
        info.capacity_percent = sysfs
            .read_optional(format!("{}/capacity", base))
            .unwrap_or(None)
            .and_then(|v| v.parse().ok());

        info.energy_now_uwh = sysfs
            .read_optional(format!("{}/energy_now", base))
            .unwrap_or(None)
            .and_then(|v| v.parse().ok());

        info.energy_full_uwh = sysfs
            .read_optional(format!("{}/energy_full", base))
            .unwrap_or(None)
            .and_then(|v| v.parse().ok());

        info.energy_full_design_uwh = sysfs
            .read_optional(format!("{}/energy_full_design", base))
            .unwrap_or(None)
            .and_then(|v| v.parse().ok());

        info.power_now_uw = sysfs
            .read_optional(format!("{}/power_now", base))
            .unwrap_or(None)
            .and_then(|v| v.parse().ok());

        info.voltage_now_uv = sysfs
            .read_optional(format!("{}/voltage_now", base))
            .unwrap_or(None)
            .and_then(|v| v.parse().ok());

        info.cycle_count = sysfs
            .read_optional(format!("{}/cycle_count", base))
            .unwrap_or(None)
            .and_then(|v| v.parse().ok());

        // Calculate health
        if let (Some(full), Some(design)) = (info.energy_full_uwh, info.energy_full_design_uwh) {
            if design > 0 {
                info.health_percent = Some((full as f64 / design as f64) * 100.0);
            }
        }

        info
    }

    /// Current power draw in watts (if available)
    pub fn power_watts(&self) -> Option<f64> {
        self.power_now_uw.map(|uw| uw as f64 / 1_000_000.0)
    }

    /// Remaining energy in Wh
    pub fn energy_wh(&self) -> Option<f64> {
        self.energy_now_uwh.map(|uw| uw as f64 / 1_000_000.0)
    }

    /// Usable capacity in Wh (accounting for health)
    pub fn usable_capacity_wh(&self) -> Option<f64> {
        self.energy_full_uwh.map(|uw| uw as f64 / 1_000_000.0)
    }

    pub fn is_discharging(&self) -> bool {
        self.status.as_deref() == Some("Discharging")
    }
}
