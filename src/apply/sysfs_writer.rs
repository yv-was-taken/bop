use crate::error::{Error, Result};
use std::path::Path;

/// Write a value to a sysfs path (absolute path).
pub fn write_sysfs(path: &str, value: &str) -> Result<()> {
    std::fs::write(path, value).map_err(|e| Error::SysfsWrite {
        path: Path::new(path).to_path_buf(),
        source: e,
    })
}

/// Toggle an ACPI wakeup source.
/// /proc/acpi/wakeup uses a toggle interface -- writing the device name flips its state.
pub fn toggle_acpi_wakeup(device: &str) -> Result<()> {
    std::fs::write("/proc/acpi/wakeup", device).map_err(|e| Error::SysfsWrite {
        path: "/proc/acpi/wakeup".into(),
        source: e,
    })
}
