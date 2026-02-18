use crate::error::{Error, Result};

/// Disable and stop a systemd service.
pub fn disable_service(service: &str) -> Result<()> {
    // Stop first
    let _ = std::process::Command::new("systemctl")
        .args(["stop", service])
        .status();

    // Then disable
    let status = std::process::Command::new("systemctl")
        .args(["disable", service])
        .status()
        .map_err(|e| Error::Other(format!("failed to disable {}: {}", service, e)))?;

    if !status.success() {
        // Mask it as a fallback (some services re-enable themselves)
        let _ = std::process::Command::new("systemctl")
            .args(["mask", service])
            .status();
    }

    Ok(())
}

/// Re-enable a previously disabled service.
pub fn enable_service(service: &str) -> Result<()> {
    // Unmask first in case we masked it
    let _ = std::process::Command::new("systemctl")
        .args(["unmask", service])
        .status();

    let status = std::process::Command::new("systemctl")
        .args(["enable", service])
        .status()
        .map_err(|e| Error::Other(format!("failed to enable {}: {}", service, e)))?;

    if !status.success() {
        return Err(Error::Other(format!("systemctl enable {} failed", service)));
    }

    Ok(())
}
