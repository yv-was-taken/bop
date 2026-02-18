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
        if !state.kernel_params_added.is_empty() {
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
    let all_succeeded = revert_steps(state);
    if all_succeeded {
        ApplyState::remove_file()?;
    }
    Ok(all_succeeded)
}

fn revert_steps(state: &ApplyState) -> bool {
    let mut all_succeeded = true;

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
                    all_succeeded = false;
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
                    all_succeeded = false;
                }
            }
        }
        println!();
    }

    // Remove kernel params
    if !state.kernel_params_added.is_empty() {
        println!("  {} Removing kernel parameters:", ">>".cyan());
        for param in &state.kernel_params_added {
            println!("     {}", param);
        }
        match apply::kernel_params::remove_kernel_params(&state.kernel_params_added) {
            Ok(()) => println!("     {}", "(will take effect after reboot)".dimmed()),
            Err(e) => {
                eprintln!("     {} Failed: {}", "!".red(), e);
                all_succeeded = false;
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
                    all_succeeded = false;
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
                all_succeeded = false;
            }
        }
        println!();
    }

    all_succeeded
}

#[cfg(test)]
mod tests {
    use super::revert_loaded_state;
    use crate::apply::{ApplyState, SysfsChange};
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

        let state = ApplyState {
            timestamp: "2026-02-18T00:00:00Z".to_string(),
            sysfs_changes: vec![
                SysfsChange {
                    path: ok_path.to_string_lossy().into_owned(),
                    original_value: "old-value".to_string(),
                    new_value: "new-value".to_string(),
                },
                SysfsChange {
                    path: failing_path.to_string_lossy().into_owned(),
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
        assert_eq!(
            fs::read_to_string(&ok_path).expect("failed to read restored file"),
            "old-value"
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
