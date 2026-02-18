use crate::sysfs::SysfsRoot;

/// RAPL (Running Average Power Limit) energy counters.
#[derive(Debug, Clone)]
pub struct RaplEnergy {
    pub cpu_uj: u64, // microjoules
    pub soc_uj: u64, // microjoules (package includes CPU + iGPU + IO)
}

pub struct RaplReader {
    cpu_path: Option<String>,
    soc_path: Option<String>,
}

impl RaplReader {
    pub fn new(sysfs: &SysfsRoot) -> Self {
        let rapl_base = "sys/class/powercap";
        let mut cpu_path = None;
        let mut soc_path = None;

        if let Ok(entries) = sysfs.list_dir(rapl_base) {
            for entry in &entries {
                let name_path = format!("{}/{}/name", rapl_base, entry);
                if let Some(name) = sysfs.read_optional(&name_path).unwrap_or(None) {
                    let energy_path = format!("{}/{}/energy_uj", rapl_base, entry);
                    match name.as_str() {
                        "core" => {
                            if sysfs.exists(&energy_path) {
                                cpu_path = Some(energy_path);
                            }
                        }
                        "package-0" => {
                            if sysfs.exists(&energy_path) {
                                soc_path = Some(energy_path);
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        Self { cpu_path, soc_path }
    }

    pub fn read_energy(&self) -> Option<RaplEnergy> {
        let sysfs = SysfsRoot::system();

        let cpu_uj = self
            .cpu_path
            .as_ref()
            .and_then(|p| sysfs.read_parse::<u64>(p).ok())
            .unwrap_or(0);

        let soc_uj = self
            .soc_path
            .as_ref()
            .and_then(|p| sysfs.read_parse::<u64>(p).ok())
            .unwrap_or(0);

        if cpu_uj == 0 && soc_uj == 0 {
            return None;
        }

        Some(RaplEnergy { cpu_uj, soc_uj })
    }
}
