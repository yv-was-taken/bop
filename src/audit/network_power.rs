use crate::audit::{Finding, Severity};
use crate::detect::HardwareInfo;

pub fn check(hw: &HardwareInfo) -> Vec<Finding> {
    let mut findings = Vec::new();

    // Check WiFi power save via iw command
    if let Some(ref iface) = hw.network.wifi_interface {
        match std::process::Command::new("iw")
            .args(["dev", iface, "get", "power_save"])
            .output()
        {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                if stdout.contains("off") {
                    findings.push(
                        Finding::new(
                            Severity::Medium,
                            "Network",
                            "WiFi power save disabled",
                        )
                        .current("off")
                        .recommended("on")
                        .impact("~0.5W savings")
                        .path(format!("iw dev {} set power_save on", iface))
                        .weight(5),
                    );
                }
                // "on" is optimal -- no finding
            }
            Err(_) => {
                findings.push(
                    Finding::new(
                        Severity::Info,
                        "Network",
                        "Could not check WiFi power save (iw not available)",
                    )
                    .current("unknown")
                    .recommended("on")
                    .impact("~0.5W if disabled")
                    .weight(1),
                );
            }
        }
    }

    findings
}
