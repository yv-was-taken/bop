pub mod battery;
pub mod cpu;
pub mod dmi;
pub mod gpu;
pub mod network;
pub mod pci;
pub mod platform;

use crate::sysfs::SysfsRoot;

/// All detected hardware information.
#[derive(Debug, Clone)]
pub struct HardwareInfo {
    pub dmi: dmi::DmiInfo,
    pub cpu: cpu::CpuInfo,
    pub gpu: gpu::GpuInfo,
    pub battery: battery::BatteryInfo,
    pub pci: pci::PciInfo,
    pub network: network::NetworkInfo,
    pub platform: platform::PlatformInfo,
    pub kernel_cmdline: String,
}

impl HardwareInfo {
    pub fn detect(sysfs: &SysfsRoot) -> Self {
        let kernel_cmdline = sysfs.read("proc/cmdline").unwrap_or_default();

        Self {
            dmi: dmi::DmiInfo::detect(sysfs),
            cpu: cpu::CpuInfo::detect(sysfs),
            gpu: gpu::GpuInfo::detect(sysfs),
            battery: battery::BatteryInfo::detect(sysfs),
            pci: pci::PciInfo::detect(sysfs),
            network: network::NetworkInfo::detect(sysfs),
            platform: platform::PlatformInfo::detect(sysfs),
            kernel_cmdline,
        }
    }

    pub fn has_kernel_param(&self, param: &str) -> bool {
        self.kernel_cmdline
            .split_whitespace()
            .any(|p| p == param || p.starts_with(&format!("{}=", param)))
    }

    pub fn kernel_param_value(&self, param: &str) -> Option<String> {
        let prefix = format!("{}=", param);
        self.kernel_cmdline
            .split_whitespace()
            .find(|p| p.starts_with(&prefix))
            .map(|p| p[prefix.len()..].to_string())
    }
}
