use crate::error::{Error, Result};
use std::path::{Path, PathBuf};
#[cfg(test)]
use std::sync::{LazyLock, Mutex};

const ACPI_WAKEUP_PATH: &str = "/proc/acpi/wakeup";

#[cfg(test)]
static ACPI_WAKEUP_PATH_OVERRIDE: LazyLock<Mutex<Option<PathBuf>>> =
    LazyLock::new(|| Mutex::new(None));

fn acpi_wakeup_path() -> PathBuf {
    #[cfg(test)]
    {
        if let Some(path) = ACPI_WAKEUP_PATH_OVERRIDE
            .lock()
            .expect("acpi wakeup path override lock poisoned")
            .clone()
        {
            return path;
        }
    }

    PathBuf::from(ACPI_WAKEUP_PATH)
}

#[cfg(test)]
pub(crate) struct AcpiWakeupPathGuard {
    _guard: std::marker::PhantomData<()>,
}

#[cfg(test)]
impl Drop for AcpiWakeupPathGuard {
    fn drop(&mut self) {
        *ACPI_WAKEUP_PATH_OVERRIDE
            .lock()
            .expect("acpi wakeup path override lock poisoned") = None;
    }
}

#[cfg(test)]
pub(crate) fn set_acpi_wakeup_path_override_for_tests(path: PathBuf) -> AcpiWakeupPathGuard {
    let mut guard = ACPI_WAKEUP_PATH_OVERRIDE
        .lock()
        .expect("acpi wakeup path override lock poisoned");
    *guard = Some(path);
    AcpiWakeupPathGuard {
        _guard: std::marker::PhantomData,
    }
}

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
    let path = acpi_wakeup_path();
    std::fs::write(&path, device).map_err(|e| Error::SysfsWrite { path, source: e })
}
