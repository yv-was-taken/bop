use crate::apply::{self, ApplyState};
use crate::error::{Error, Result};
use colored::Colorize;

pub fn revert() -> Result<()> {
    if !nix::unistd::geteuid().is_root() {
        return Err(Error::NotRoot {
            operation: "revert".to_string(),
        });
    }

    let state = match ApplyState::load()? {
        Some(s) => s,
        None => {
            println!("{}", "No saved state found. Nothing to revert.".yellow());
            return Ok(());
        }
    };

    println!(
        "{} (applied at {})",
        "Reverting changes".bold().underline(),
        state.timestamp
    );
    println!();

    let all_succeeded = revert_loaded_state(&state)?;

    if all_succeeded {
        println!("{}", "Revert complete.".green().bold());
        if !state.kernel_param_backups.is_empty() || !state.kernel_params_added.is_empty() {
            println!(
                "{}",
                "  Note: Kernel parameter changes require a reboot to take effect.".yellow()
            );
        }
    } else {
        eprintln!(
            "{}",
            format!(
                "Revert incomplete. Kept state file at {} so you can retry after resolving failures.",
                ApplyState::file_path().display()
            )
            .yellow()
        );
    }

    Ok(())
}

fn revert_loaded_state(state: &ApplyState) -> Result<bool> {
    let remaining = revert_steps(state);
    if has_pending_reverts(&remaining) {
        remaining.save()?;
        Ok(false)
    } else {
        ApplyState::remove_file()?;
        Ok(true)
    }
}

fn has_pending_reverts(state: &ApplyState) -> bool {
    !state.sysfs_changes.is_empty()
        || !state.acpi_wakeup_toggled.is_empty()
        || !state.kernel_params_added.is_empty()
        || !state.services_disabled.is_empty()
        || !state.systemd_units_created.is_empty()
}

fn revert_steps(state: &ApplyState) -> ApplyState {
    let mut remaining = ApplyState {
        timestamp: state.timestamp.clone(),
        ..Default::default()
    };

    // Revert sysfs changes
    if !state.sysfs_changes.is_empty() {
        println!("  {} Restoring sysfs values:", ">>".cyan());
        for change in &state.sysfs_changes {
            match std::fs::write(&change.path, &change.original_value) {
                Ok(()) => {
                    println!(
                        "     {} {} -> {}",
                        change.path.dimmed(),
                        change.new_value.red(),
                        change.original_value.green()
                    );
                }
                Err(e) => {
                    eprintln!(
                        "     {} Failed to restore {}: {}",
                        "!".red(),
                        change.path,
                        e
                    );
                    remaining.sysfs_changes.push(change.clone());
                }
            }
        }
        println!();
    }

    // Re-enable ACPI wakeup sources (toggle them back)
    if !state.acpi_wakeup_toggled.is_empty() {
        println!("  {} Re-enabling ACPI wakeup sources:", ">>".cyan());
        for device in &state.acpi_wakeup_toggled {
            match apply::sysfs_writer::toggle_acpi_wakeup(device) {
                Ok(()) => println!("     {} {}", "enabled".green(), device),
                Err(e) => {
                    eprintln!("     {} Failed to toggle {}: {}", "!".red(), device, e);
                    remaining.acpi_wakeup_toggled.push(device.clone());
                }
            }
        }
        println!();
    }

    // Restore kernel params
    if !state.kernel_param_backups.is_empty() {
        println!("  {} Restoring kernel parameter boot entries:", ">>".cyan());
        for backup in &state.kernel_param_backups {
            println!("     {}", backup.path);
        }
        match apply::kernel_params::restore_kernel_param_backups(&state.kernel_param_backups) {
            Ok(()) => println!("     {}", "(will take effect after reboot)".dimmed()),
            Err(e) => eprintln!("     {} Failed: {}", "!".red(), e),
        }
        println!();
    } else if !state.kernel_params_added.is_empty() {
        // Backward compatibility for state files created before backup support.
        println!("  {} Removing kernel parameters:", ">>".cyan());
        for param in &state.kernel_params_added {
            println!("     {}", param);
        }
        match apply::kernel_params::remove_kernel_params(&state.kernel_params_added) {
            Ok(()) => println!("     {}", "(will take effect after reboot)".dimmed()),
            Err(e) => {
                eprintln!("     {} Failed: {}", "!".red(), e);
                remaining.kernel_params_added = state.kernel_params_added.clone();
            }
        }
        println!();
    }

    // Re-enable services
    if !state.services_disabled.is_empty() {
        println!("  {} Re-enabling services:", ">>".cyan());
        for svc in &state.services_disabled {
            match apply::services::enable_service(svc) {
                Ok(()) => println!("     {} {}", "enabled".green(), svc),
                Err(e) => {
                    eprintln!("     {} Failed to enable {}: {}", "!".red(), svc, e);
                    remaining.services_disabled.push(svc.clone());
                }
            }
        }
        println!();
    }

    // Remove systemd units
    if !state.systemd_units_created.is_empty() {
        println!("  {} Removing systemd units:", ">>".cyan());
        match apply::systemd::remove_service() {
            Ok(()) => {
                for unit in &state.systemd_units_created {
                    println!("     {} {}", "removed".green(), unit);
                }
            }
            Err(e) => {
                eprintln!("     {} Failed: {}", "!".red(), e);
                remaining.systemd_units_created = state.systemd_units_created.clone();
            }
        }
        println!();
    }

    remaining
}

#[cfg(test)]
mod tests {
    use super::revert_loaded_state;
    use crate::apply::{ApplyState, SysfsChange, sysfs_writer};
    use std::fs;
    use std::path::PathBuf;
    use std::sync::{LazyLock, Mutex};
    use tempfile::TempDir;

    static TEST_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

    struct StateFileOverrideGuard;

    impl Drop for StateFileOverrideGuard {
        fn drop(&mut self) {
            ApplyState::set_file_path_override_for_tests(None);
        }
    }

    fn set_state_file_override(path: PathBuf) -> StateFileOverrideGuard {
        ApplyState::set_file_path_override_for_tests(Some(path));
        StateFileOverrideGuard
    }

    struct AcpiWakeupPathOverrideGuard;

    impl Drop for AcpiWakeupPathOverrideGuard {
        fn drop(&mut self) {
            sysfs_writer::set_acpi_wakeup_path_override_for_tests(None);
        }
    }

    fn set_acpi_wakeup_path_override(path: PathBuf) -> AcpiWakeupPathOverrideGuard {
        sysfs_writer::set_acpi_wakeup_path_override_for_tests(Some(path));
        AcpiWakeupPathOverrideGuard
    }

    #[test]
    fn test_revert_keeps_state_when_a_restore_step_fails() {
        let _test_guard = TEST_LOCK.lock().expect("test lock poisoned");
        let tmp = TempDir::new().expect("failed to create temp dir");
        let state_path = tmp.path().join("state.json");
        let _state_override = set_state_file_override(state_path.clone());

        let ok_path = tmp.path().join("restore-ok");
        fs::write(&ok_path, "new-value").expect("failed to seed writable sysfs mock");

        let missing_parent = tmp.path().join("missing");
        let failing_path = missing_parent.join("restore-fail");
        let failing_path_str = failing_path.to_string_lossy().into_owned();

        let state = ApplyState {
            timestamp: "2026-02-18T00:00:00Z".to_string(),
            sysfs_changes: vec![
                SysfsChange {
                    path: ok_path.to_string_lossy().into_owned(),
                    original_value: "old-value".to_string(),
                    new_value: "new-value".to_string(),
                },
                SysfsChange {
                    path: failing_path_str.clone(),
                    original_value: "old-fail".to_string(),
                    new_value: "new-fail".to_string(),
                },
            ],
            ..Default::default()
        };

        state.save().expect("failed to save state");
        assert!(state_path.exists(), "state file should be created");

        let all_succeeded = revert_loaded_state(&state).expect("revert execution failed");
        assert!(
            !all_succeeded,
            "revert should report partial failure when one restore step fails"
        );
        assert!(
            state_path.exists(),
            "state file must be preserved when revert is partial"
        );
        let remaining = ApplyState::load()
            .expect("failed to load persisted partial state")
            .expect("persisted partial state should exist");
        assert_eq!(
            remaining.sysfs_changes.len(),
            1,
            "only failed sysfs entries should remain for retry"
        );
        assert_eq!(
            remaining.sysfs_changes[0].path, failing_path_str,
            "partial state should only retain the failed path"
        );
        assert_eq!(
            fs::read_to_string(&ok_path).expect("failed to read restored file"),
            "old-value"
        );
    }

    #[test]
    fn test_revert_does_not_retry_successful_acpi_toggles_after_partial_failure() {
        let _test_guard = TEST_LOCK.lock().expect("test lock poisoned");
        let tmp = TempDir::new().expect("failed to create temp dir");
        let state_path = tmp.path().join("state.json");
        let _state_override = set_state_file_override(state_path.clone());

        let acpi_wakeup_path = tmp.path().join("acpi-wakeup");
        let _acpi_override = set_acpi_wakeup_path_override(acpi_wakeup_path.clone());
        fs::write(&acpi_wakeup_path, "").expect("failed to seed acpi wakeup mock");

        let missing_parent = tmp.path().join("missing");
        let failing_path = missing_parent.join("restore-fail");
        let failing_path_str = failing_path.to_string_lossy().into_owned();

        let state = ApplyState {
            timestamp: "2026-02-18T00:00:00Z".to_string(),
            sysfs_changes: vec![SysfsChange {
                path: failing_path_str.clone(),
                original_value: "old-fail".to_string(),
                new_value: "new-fail".to_string(),
            }],
            acpi_wakeup_toggled: vec!["XHC0".to_string()],
            ..Default::default()
        };

        state.save().expect("failed to save state");
        assert!(state_path.exists(), "state file should be created");

        let all_succeeded = revert_loaded_state(&state).expect("revert execution failed");
        assert!(
            !all_succeeded,
            "revert should report partial failure when any restore step fails"
        );

        let remaining = ApplyState::load()
            .expect("failed to load persisted partial state")
            .expect("persisted partial state should exist");
        assert!(
            remaining.acpi_wakeup_toggled.is_empty(),
            "successful ACPI toggles must be removed to avoid re-toggling on retry"
        );
        assert_eq!(
            remaining.sysfs_changes.len(),
            1,
            "failed sysfs restore should remain for retry"
        );
        assert_eq!(
            remaining.sysfs_changes[0].path, failing_path_str,
            "the failed sysfs path should stay in persisted state"
        );
        assert_eq!(
            fs::read_to_string(&acpi_wakeup_path).expect("failed to read acpi wakeup mock"),
            "XHC0",
            "ACPI wakeup toggle should run once and not be retained in state"
        );
    }

    #[test]
    fn test_revert_removes_state_when_all_steps_succeed() {
        let _test_guard = TEST_LOCK.lock().expect("test lock poisoned");
        let tmp = TempDir::new().expect("failed to create temp dir");
        let state_path = tmp.path().join("state.json");
        let _state_override = set_state_file_override(state_path.clone());

        let restored_path = tmp.path().join("restore-ok");
        fs::write(&restored_path, "new-value").expect("failed to seed writable sysfs mock");

        let state = ApplyState {
            timestamp: "2026-02-18T00:00:00Z".to_string(),
            sysfs_changes: vec![SysfsChange {
                path: restored_path.to_string_lossy().into_owned(),
                original_value: "old-value".to_string(),
                new_value: "new-value".to_string(),
            }],
            ..Default::default()
        };

        state.save().expect("failed to save state");
        assert!(state_path.exists(), "state file should be created");

        let all_succeeded = revert_loaded_state(&state).expect("revert execution failed");
        assert!(
            all_succeeded,
            "revert should succeed when all steps succeed"
        );
        assert!(
            !state_path.exists(),
            "state file should be removed only when revert fully succeeds"
        );
        assert_eq!(
            fs::read_to_string(&restored_path).expect("failed to read restored file"),
            "old-value"
        );
    }
}
