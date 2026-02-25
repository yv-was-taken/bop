use crate::sysfs::SysfsRoot;

#[derive(Debug, Clone, Default)]
pub struct BatteryInfo {
    pub present: bool,
    pub status: Option<String>,
    pub capacity_percent: Option<u32>,
    // Energy-based reporting (µWh / µW)
    pub energy_now_uwh: Option<u64>,
    pub energy_full_uwh: Option<u64>,
    pub energy_full_design_uwh: Option<u64>,
    pub power_now_uw: Option<u64>,
    // Charge-based reporting (µAh / µA) — used when energy fields are absent
    pub charge_now_uah: Option<u64>,
    pub charge_full_uah: Option<u64>,
    pub charge_full_design_uah: Option<u64>,
    pub current_now_ua: Option<u64>,
    pub voltage_now_uv: Option<u64>,
    pub cycle_count: Option<u32>,
    pub health_percent: Option<f64>,
    pub supply_name: Option<String>,
}

fn read_u64(sysfs: &SysfsRoot, path: String) -> Option<u64> {
    sysfs
        .read_optional(path)
        .unwrap_or(None)
        .and_then(|v| v.parse().ok())
}

impl BatteryInfo {
    pub fn detect(sysfs: &SysfsRoot) -> Self {
        let mut info = Self::default();

        let ps_base = "sys/class/power_supply";
        let entries = match sysfs.list_dir(ps_base) {
            Ok(e) => e,
            Err(_) => return info,
        };

        let bat_name = entries.iter().find(|e| e.starts_with("BAT"));
        let bat_name = match bat_name {
            Some(n) => n.clone(),
            None => return info,
        };

        info.supply_name = Some(bat_name.clone());
        let base = format!("{}/{}", ps_base, bat_name);

        if let Some(ptype) = sysfs
            .read_optional(format!("{}/type", base))
            .unwrap_or(None)
            && ptype != "Battery"
        {
            return info;
        }

        info.present = sysfs
            .read_optional(format!("{}/present", base))
            .unwrap_or(None)
            .as_deref()
            == Some("1");

        info.status = sysfs
            .read_optional(format!("{}/status", base))
            .unwrap_or(None);
        info.capacity_percent = read_u64(sysfs, format!("{}/capacity", base)).map(|v| v as u32);

        // Energy-based fields (some batteries report these directly)
        info.energy_now_uwh = read_u64(sysfs, format!("{}/energy_now", base));
        info.energy_full_uwh = read_u64(sysfs, format!("{}/energy_full", base));
        info.energy_full_design_uwh = read_u64(sysfs, format!("{}/energy_full_design", base));
        info.power_now_uw = read_u64(sysfs, format!("{}/power_now", base));

        // Charge-based fields (other batteries report µAh/µA instead)
        info.charge_now_uah = read_u64(sysfs, format!("{}/charge_now", base));
        info.charge_full_uah = read_u64(sysfs, format!("{}/charge_full", base));
        info.charge_full_design_uah = read_u64(sysfs, format!("{}/charge_full_design", base));
        info.current_now_ua = read_u64(sysfs, format!("{}/current_now", base));
        info.voltage_now_uv = read_u64(sysfs, format!("{}/voltage_now", base));

        info.cycle_count = read_u64(sysfs, format!("{}/cycle_count", base)).map(|v| v as u32);

        // Calculate health from whichever set of fields is available
        let (full, design) = match (info.energy_full_uwh, info.energy_full_design_uwh) {
            (Some(f), Some(d)) => (Some(f), Some(d)),
            _ => (info.charge_full_uah, info.charge_full_design_uah),
        };
        if let (Some(full), Some(design)) = (full, design)
            && design > 0
        {
            info.health_percent = Some((full as f64 / design as f64) * 100.0);
        }

        info
    }

    /// Current power draw in watts.
    /// Prefers direct `power_now`, falls back to `current_now * voltage_now`.
    pub fn power_watts(&self) -> Option<f64> {
        if let Some(uw) = self.power_now_uw {
            return Some(uw as f64 / 1_000_000.0);
        }
        if let (Some(ua), Some(uv)) = (self.current_now_ua, self.voltage_now_uv) {
            return Some(ua as f64 * uv as f64 / 1e12);
        }
        None
    }

    /// Remaining energy in Wh.
    /// Prefers direct `energy_now`, falls back to `charge_now * voltage_now`.
    pub fn energy_wh(&self) -> Option<f64> {
        if let Some(uwh) = self.energy_now_uwh {
            return Some(uwh as f64 / 1_000_000.0);
        }
        if let (Some(uah), Some(uv)) = (self.charge_now_uah, self.voltage_now_uv) {
            return Some(uah as f64 * uv as f64 / 1e12);
        }
        None
    }

    /// Full capacity in Wh.
    /// Prefers direct `energy_full`, falls back to `charge_full * voltage_now`.
    pub fn usable_capacity_wh(&self) -> Option<f64> {
        if let Some(uwh) = self.energy_full_uwh {
            return Some(uwh as f64 / 1_000_000.0);
        }
        if let (Some(uah), Some(uv)) = (self.charge_full_uah, self.voltage_now_uv) {
            return Some(uah as f64 * uv as f64 / 1e12);
        }
        None
    }

    pub fn is_discharging(&self) -> bool {
        self.status.as_deref() == Some("Discharging")
    }
}
