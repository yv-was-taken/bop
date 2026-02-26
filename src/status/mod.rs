use crate::apply::ApplyState;
use serde::Serialize;

/// Status of a single sysfs value after apply.
#[derive(Debug, Clone, Serialize)]
pub struct SysfsStatus {
    pub path: String,
    pub expected: String,
    pub actual: Option<String>,
    pub active: bool,
}

/// Status of an ACPI wakeup source.
#[derive(Debug, Clone, Serialize)]
pub struct WakeupStatus {
    pub device: String,
    pub active: bool, // true = still disabled as intended
}

/// Status of a kernel parameter.
#[derive(Debug, Clone, Serialize)]
pub struct KernelParamStatus {
    pub param: String,
    pub in_cmdline: bool,
}

/// Status of a disabled service.
#[derive(Debug, Clone, Serialize)]
pub struct ServiceStatus {
    pub name: String,
    pub still_stopped: bool,
}

/// Status of the generated systemd unit.
#[derive(Debug, Clone, Serialize)]
pub struct UnitStatus {
    pub path: String,
    pub exists: bool,
}

/// Full status report.
#[derive(Debug, Clone, Serialize)]
pub struct StatusReport {
    pub timestamp: String,
    pub sysfs: Vec<SysfsStatus>,
    pub acpi_wakeup: Vec<WakeupStatus>,
    pub kernel_params: Vec<KernelParamStatus>,
    pub services: Vec<ServiceStatus>,
    pub systemd_unit: Option<UnitStatus>,
}

impl StatusReport {
    /// Count of all optimizations that are verified active.
    pub fn active_count(&self) -> usize {
        self.sysfs.iter().filter(|s| s.active).count()
            + self.acpi_wakeup.iter().filter(|w| w.active).count()
            + self.kernel_params.iter().filter(|k| k.in_cmdline).count()
            + self.services.iter().filter(|s| s.still_stopped).count()
            + self.systemd_unit.iter().filter(|u| u.exists).count()
    }

    /// Total number of tracked optimizations.
    pub fn total_count(&self) -> usize {
        self.sysfs.len()
            + self.acpi_wakeup.len()
            + self.kernel_params.len()
            + self.services.len()
            + self.systemd_unit.iter().count()
    }

    /// Count of drifted (inactive) optimizations.
    pub fn drifted_count(&self) -> usize {
        self.total_count() - self.active_count()
    }
}

/// Check sysfs values from the saved state against live filesystem.
fn check_sysfs(state: &ApplyState) -> Vec<SysfsStatus> {
    state
        .sysfs_changes
        .iter()
        .map(|change| {
            let actual = std::fs::read_to_string(&change.path)
                .ok()
                .map(|s| s.trim().to_string());
            let active = actual.as_deref() == Some(change.new_value.trim());
            SysfsStatus {
                path: change.path.clone(),
                expected: change.new_value.trim().to_string(),
                actual,
                active,
            }
        })
        .collect()
}

/// Check ACPI wakeup sources against /proc/acpi/wakeup.
fn check_acpi_wakeup(state: &ApplyState, acpi_wakeup_content: &str) -> Vec<WakeupStatus> {
    state
        .acpi_wakeup_toggled
        .iter()
        .map(|device| {
            // Parse /proc/acpi/wakeup to find this device's current state.
            // Lines look like: "XHC1      S0    *disabled   pci:0000:00:08.1"
            let actual_disabled = acpi_wakeup_content.lines().any(|line| {
                let parts: Vec<&str> = line.split_whitespace().collect();
                parts.first() == Some(&device.as_str()) && parts.contains(&"*disabled")
            });
            WakeupStatus {
                device: device.clone(),
                active: actual_disabled,
            }
        })
        .collect()
}

/// Check kernel parameters against /proc/cmdline content.
fn check_kernel_params(state: &ApplyState, cmdline: &str) -> Vec<KernelParamStatus> {
    state
        .kernel_params_added
        .iter()
        .map(|param| {
            let in_cmdline = cmdline.split_whitespace().any(|p| p == param);
            KernelParamStatus {
                param: param.clone(),
                in_cmdline,
            }
        })
        .collect()
}

/// Check whether disabled services are still stopped.
fn check_services(state: &ApplyState) -> Vec<ServiceStatus> {
    state
        .services_disabled
        .iter()
        .map(|svc| {
            let is_active = std::process::Command::new("systemctl")
                .args(["is-active", "--quiet", svc])
                .status()
                .is_ok_and(|s| s.success())
                || std::process::Command::new("systemctl")
                    .args(["is-enabled", "--quiet", svc])
                    .status()
                    .is_ok_and(|s| s.success());
            ServiceStatus {
                name: svc.clone(),
                still_stopped: !is_active,
            }
        })
        .collect()
}

/// Check whether generated systemd units still exist on disk.
fn check_systemd_units(state: &ApplyState) -> Option<UnitStatus> {
    state.systemd_units_created.first().map(|path| UnitStatus {
        path: path.clone(),
        exists: std::path::Path::new(path).exists(),
    })
}

/// Build a full status report from saved state.
/// Returns None if no state file exists.
pub fn check() -> crate::error::Result<Option<StatusReport>> {
    let state = match ApplyState::load()? {
        Some(s) => s,
        None => return Ok(None),
    };

    let acpi_content = std::fs::read_to_string("/proc/acpi/wakeup").unwrap_or_default();
    let cmdline = std::fs::read_to_string("/proc/cmdline").unwrap_or_default();

    Ok(Some(StatusReport {
        timestamp: state.timestamp.clone(),
        sysfs: check_sysfs(&state),
        acpi_wakeup: check_acpi_wakeup(&state, &acpi_content),
        kernel_params: check_kernel_params(&state, &cmdline),
        services: check_services(&state),
        systemd_unit: check_systemd_units(&state),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apply::SysfsChange;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_check_sysfs_active_value() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("profile");
        fs::write(&path, "low-power\n").unwrap();

        let state = ApplyState {
            sysfs_changes: vec![SysfsChange {
                path: path.to_string_lossy().into_owned(),
                original_value: "performance".to_string(),
                new_value: "low-power".to_string(),
            }],
            ..Default::default()
        };

        let result = check_sysfs(&state);
        assert_eq!(result.len(), 1);
        assert!(result[0].active);
        assert_eq!(result[0].actual.as_deref(), Some("low-power"));
    }

    #[test]
    fn test_check_sysfs_drifted_value() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("profile");
        fs::write(&path, "performance\n").unwrap();

        let state = ApplyState {
            sysfs_changes: vec![SysfsChange {
                path: path.to_string_lossy().into_owned(),
                original_value: "performance".to_string(),
                new_value: "low-power".to_string(),
            }],
            ..Default::default()
        };

        let result = check_sysfs(&state);
        assert_eq!(result.len(), 1);
        assert!(!result[0].active);
        assert_eq!(result[0].actual.as_deref(), Some("performance"));
    }

    #[test]
    fn test_check_sysfs_missing_path() {
        let state = ApplyState {
            sysfs_changes: vec![SysfsChange {
                path: "/nonexistent/path/does/not/exist".to_string(),
                original_value: "old".to_string(),
                new_value: "new".to_string(),
            }],
            ..Default::default()
        };

        let result = check_sysfs(&state);
        assert_eq!(result.len(), 1);
        assert!(!result[0].active);
        assert!(result[0].actual.is_none());
    }

    #[test]
    fn test_check_acpi_wakeup_disabled() {
        let content = "\
XHC0\t  S0\t*enabled   pci:0000:c4:00.3
XHC1\t  S0\t*disabled  pci:0000:c4:00.4
XHC2\t  S0\t*disabled  pci:0000:c2:00.0";

        let state = ApplyState {
            acpi_wakeup_toggled: vec!["XHC1".to_string(), "XHC2".to_string()],
            ..Default::default()
        };

        let result = check_acpi_wakeup(&state, content);
        assert_eq!(result.len(), 2);
        assert!(result[0].active);
        assert!(result[1].active);
    }

    #[test]
    fn test_check_acpi_wakeup_drifted() {
        let content = "\
XHC1\t  S0\t*enabled   pci:0000:c4:00.4";

        let state = ApplyState {
            acpi_wakeup_toggled: vec!["XHC1".to_string()],
            ..Default::default()
        };

        let result = check_acpi_wakeup(&state, content);
        assert_eq!(result.len(), 1);
        assert!(
            !result[0].active,
            "XHC1 re-enabled should be detected as drift"
        );
    }

    #[test]
    fn test_check_kernel_params_present() {
        let cmdline = "BOOT_IMAGE=/vmlinuz-linux root=UUID=abc ro acpi.ec_no_wakeup=1 rtc_cmos.use_acpi_alarm=1";

        let state = ApplyState {
            kernel_params_added: vec![
                "acpi.ec_no_wakeup=1".to_string(),
                "rtc_cmos.use_acpi_alarm=1".to_string(),
            ],
            ..Default::default()
        };

        let result = check_kernel_params(&state, cmdline);
        assert_eq!(result.len(), 2);
        assert!(result[0].in_cmdline);
        assert!(result[1].in_cmdline);
    }

    #[test]
    fn test_check_kernel_params_pending_reboot() {
        let cmdline = "BOOT_IMAGE=/vmlinuz-linux root=UUID=abc ro";

        let state = ApplyState {
            kernel_params_added: vec!["acpi.ec_no_wakeup=1".to_string()],
            ..Default::default()
        };

        let result = check_kernel_params(&state, cmdline);
        assert_eq!(result.len(), 1);
        assert!(!result[0].in_cmdline);
    }

    #[test]
    fn test_report_counts() {
        let report = StatusReport {
            timestamp: "2026-02-18T00:00:00Z".to_string(),
            sysfs: vec![
                SysfsStatus {
                    path: "a".into(),
                    expected: "x".into(),
                    actual: Some("x".into()),
                    active: true,
                },
                SysfsStatus {
                    path: "b".into(),
                    expected: "y".into(),
                    actual: Some("z".into()),
                    active: false,
                },
            ],
            acpi_wakeup: vec![WakeupStatus {
                device: "XHC1".into(),
                active: true,
            }],
            kernel_params: vec![
                KernelParamStatus {
                    param: "foo=1".into(),
                    in_cmdline: true,
                },
                KernelParamStatus {
                    param: "bar=1".into(),
                    in_cmdline: false,
                },
            ],
            services: vec![],
            systemd_unit: Some(UnitStatus {
                path: "/etc/systemd/system/bop.service".into(),
                exists: true,
            }),
        };

        assert_eq!(report.total_count(), 6);
        assert_eq!(report.active_count(), 4);
        assert_eq!(report.drifted_count(), 2);
    }
}
