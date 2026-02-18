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
                Err(e) => eprintln!("     {} Failed to toggle {}: {}", "!".red(), device, e),
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
            Err(e) => eprintln!("     {} Failed: {}", "!".red(), e),
        }
        println!();
    }

    // Re-enable services
    if !state.services_disabled.is_empty() {
        println!("  {} Re-enabling services:", ">>".cyan());
        for svc in &state.services_disabled {
            match apply::services::enable_service(svc) {
                Ok(()) => println!("     {} {}", "enabled".green(), svc),
                Err(e) => eprintln!("     {} Failed to enable {}: {}", "!".red(), svc, e),
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
            Err(e) => eprintln!("     {} Failed: {}", "!".red(), e),
        }
        println!();
    }

    // Remove state file
    ApplyState::remove_file()?;

    println!("{}", "Revert complete.".green().bold());
    if !state.kernel_params_added.is_empty() {
        println!(
            "{}",
            "  Note: Kernel parameter changes require a reboot to take effect.".yellow()
        );
    }

    Ok(())
}
