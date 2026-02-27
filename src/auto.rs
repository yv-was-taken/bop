use crate::apply::ApplyState;
use crate::detect::HardwareInfo;
use crate::error::{Error, Result};
use crate::sysfs::SysfsRoot;
use colored::Colorize;
use std::fs;
use std::path::{Path, PathBuf};

const UDEV_RULE_PATH: &str = "/etc/udev/rules.d/85-bop.rules";
const LOCK_DIR: &str = "/run/bop";
const LOCK_FILE: &str = "/run/bop/auto.lock";

fn udev_rule_content(aggressive: bool) -> String {
    let bin = if aggressive {
        "/usr/bin/bop --aggressive auto"
    } else {
        "/usr/bin/bop auto"
    };
    format!(
        r#"# Managed by bop — do not edit
ACTION=="change", SUBSYSTEM=="power_supply", KERNEL!="hidpp_battery*", RUN+="{}"
"#,
        bin
    )
}

/// Outcome of an auto-switching run.
#[derive(Debug, PartialEq, Eq)]
pub enum AutoOutcome {
    Applied,
    Reverted,
    NoOp,
    NoProfile,
    NoAcAdapter,
}

/// Lock guard that removes the lock file on drop.
struct LockGuard {
    path: PathBuf,
}

impl Drop for LockGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

/// Try to acquire the lock file. Returns None if another bop auto is running.
fn acquire_lock() -> Option<LockGuard> {
    let lock_dir = Path::new(LOCK_DIR);
    if !lock_dir.exists() && fs::create_dir_all(lock_dir).is_err() {
        return None;
    }

    let lock_path = PathBuf::from(LOCK_FILE);

    // Try to create the lock file atomically
    match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&lock_path)
    {
        Ok(file) => {
            use std::io::Write;
            let pid = std::process::id();
            let _ = write!(&file, "{}", pid);
            Some(LockGuard { path: lock_path })
        }
        Err(_) => {
            // Lock file exists — check if the PID is still alive
            if let Ok(contents) = fs::read_to_string(&lock_path)
                && let Ok(pid) = contents.trim().parse::<u32>()
            {
                let proc_path = format!("/proc/{}", pid);
                if !Path::new(&proc_path).exists() {
                    // Stale lock — remove and retry
                    let _ = fs::remove_file(&lock_path);
                    return acquire_lock();
                }
            }
            None
        }
    }
}

/// Log an auto-switching event to the systemd journal via `logger`.
fn log_to_journal(outcome: &AutoOutcome) {
    let (priority, message) = match outcome {
        AutoOutcome::Applied => ("info", "Battery detected — power optimizations applied"),
        AutoOutcome::Reverted => ("info", "AC power detected — optimizations reverted"),
        AutoOutcome::NoOp => (
            "debug",
            "No action needed (state already matches power source)",
        ),
        AutoOutcome::NoProfile => ("warning", "No hardware profile matched — skipping"),
        AutoOutcome::NoAcAdapter => ("debug", "No AC adapter detected"),
    };

    let _ = std::process::Command::new("logger")
        .args(["-t", "bop", "-p", &format!("user.{}", priority), message])
        .status();
}

/// Core auto-switching logic. Called by udev or `bop auto`.
pub fn run(aggressive: bool, config: &crate::config::BopConfig) -> Result<AutoOutcome> {
    if !nix::unistd::geteuid().is_root() {
        return Err(Error::NotRoot {
            operation: "auto".to_string(),
        });
    }

    let _lock = match acquire_lock() {
        Some(guard) => guard,
        None => return Ok(AutoOutcome::NoOp),
    };

    let sysfs = SysfsRoot::system();
    let hw = HardwareInfo::detect(&sysfs);

    if !hw.ac.found {
        let outcome = AutoOutcome::NoAcAdapter;
        log_to_journal(&outcome);
        return Ok(outcome);
    }

    let profile = crate::profile::detect_profile(&hw);
    if profile.is_none() {
        let outcome = AutoOutcome::NoProfile;
        log_to_journal(&outcome);
        return Ok(outcome);
    }

    let existing_state = ApplyState::load()?;
    let state_exists = existing_state.is_some();

    if hw.ac.is_on_battery() && !state_exists {
        // Check inhibitors
        let inhibitors = crate::inhibitors::check_inhibitors().unwrap_or_default();
        let scope = crate::inhibitors::should_apply(&config.inhibitors.mode, &inhibitors);

        if scope == crate::inhibitors::ApplyScope::Skip {
            let outcome = AutoOutcome::NoOp;
            log_to_journal(&outcome);
            return Ok(outcome);
        }

        let plan = match scope {
            crate::inhibitors::ApplyScope::Reduced => crate::apply::build_plan_reduced(&hw, &sysfs),
            _ => {
                if aggressive {
                    crate::apply::build_plan_aggressive_with_config(&hw, &sysfs, config)
                } else {
                    crate::apply::build_plan_with_config(&hw, &sysfs, config)
                }
            }
        };

        let mut state = crate::apply::execute_plan(&plan, &hw, false)?;

        // Dim backlight after applying optimizations
        if config.brightness.auto_dim {
            match crate::brightness::dim(&config.brightness, &sysfs) {
                Ok(original) => {
                    if original.is_some() {
                        state.brightness_original = original;
                        state.save()?;
                    }
                }
                Err(e) => {
                    eprintln!("{} Failed to dim backlight: {}", "!".yellow(), e);
                }
            }
        }

        let outcome = AutoOutcome::Applied;
        log_to_journal(&outcome);

        if config.notifications.enabled && config.notifications.on_apply {
            let _ = crate::notify::send("bop", "Power optimizations applied (on battery)");
        }

        Ok(outcome)
    } else if hw.ac.is_on_ac() && state_exists {
        // Restore brightness before reverting other changes
        if let Some(ref state) = existing_state
            && let Some(original) = state.brightness_original
            && let Err(e) = crate::brightness::restore(original, &sysfs)
        {
            eprintln!("{} Failed to restore backlight: {}", "!".yellow(), e);
        }

        // On AC, optimizations applied — revert them
        crate::revert::revert()?;
        let outcome = AutoOutcome::Reverted;
        log_to_journal(&outcome);

        if config.notifications.enabled && config.notifications.on_revert {
            let _ = crate::notify::send("bop", "Power optimizations reverted (on AC)");
        }

        Ok(outcome)
    } else {
        let outcome = AutoOutcome::NoOp;
        log_to_journal(&outcome);
        Ok(outcome)
    }
}

/// Install udev rule and apply immediately if on battery.
pub fn enable(aggressive: bool) -> Result<()> {
    if !nix::unistd::geteuid().is_root() {
        return Err(Error::NotRoot {
            operation: "auto enable".to_string(),
        });
    }

    let rule = udev_rule_content(aggressive);
    fs::write(UDEV_RULE_PATH, &rule)
        .map_err(|e| Error::Other(format!("failed to write udev rule: {}", e)))?;

    reload_udevd();

    let mode = if aggressive { "aggressive" } else { "normal" };
    println!(
        "{} Auto-switching enabled (mode: {})",
        ">>".green(),
        mode.bold()
    );
    println!("  Rule installed at {}", UDEV_RULE_PATH);

    // Apply immediately if currently on battery
    let config = crate::config::load(None);
    match run(aggressive, &config)? {
        AutoOutcome::Applied => {
            println!("  {} On battery — optimizations applied.", ">>".green());
        }
        AutoOutcome::NoOp => {
            println!("  On AC power — optimizations will apply when unplugged.");
        }
        AutoOutcome::NoProfile => {
            println!(
                "  {} No hardware profile matched. Auto-switching enabled but no optimizations to apply.",
                "!".yellow()
            );
        }
        AutoOutcome::NoAcAdapter => {
            println!("  {} No AC adapter detected.", "!".yellow());
        }
        AutoOutcome::Reverted => {} // shouldn't happen on enable, but harmless
    }

    Ok(())
}

/// Remove udev rule and reload.
pub fn disable() -> Result<()> {
    if !nix::unistd::geteuid().is_root() {
        return Err(Error::NotRoot {
            operation: "auto disable".to_string(),
        });
    }

    let path = Path::new(UDEV_RULE_PATH);
    if path.exists() {
        fs::remove_file(path)
            .map_err(|e| Error::Other(format!("failed to remove udev rule: {}", e)))?;
        reload_udevd();
        println!("{} Auto-switching disabled.", ">>".green());
        println!("  Removed {}", UDEV_RULE_PATH);
    } else {
        println!("Auto-switching is not enabled (no udev rule found).");
    }

    Ok(())
}

/// JSON-serializable representation of auto-switching status.
#[derive(serde::Serialize)]
struct AutoStatus {
    enabled: bool,
    mode: Option<String>,
    ac_online: bool,
    optimizations_applied: bool,
}

/// Show status of auto-switching.
pub fn status(json: bool) -> Result<()> {
    let rule_path = Path::new(UDEV_RULE_PATH);
    let enabled = rule_path.exists();

    let mode = if enabled {
        let content = fs::read_to_string(rule_path).unwrap_or_default();
        if content.contains("--aggressive") {
            "aggressive"
        } else {
            "normal"
        }
    } else {
        "n/a"
    };

    let sysfs = SysfsRoot::system();
    let hw = HardwareInfo::detect(&sysfs);
    let state_exists = ApplyState::load().ok().and_then(|s| s).is_some();

    if json {
        let status = AutoStatus {
            enabled,
            mode: if enabled {
                Some(mode.to_string())
            } else {
                None
            },
            ac_online: hw.ac.online,
            optimizations_applied: state_exists,
        };
        let json_str = serde_json::to_string_pretty(&status)
            .map_err(|e| Error::Other(format!("JSON serialization failed: {}", e)))?;
        println!("{}", json_str);
        return Ok(());
    }

    println!("{}", "Auto-switching status".bold().underline());
    println!();
    println!(
        "  {} {}",
        "Enabled:".bold(),
        if enabled {
            "yes".green().to_string()
        } else {
            "no".yellow().to_string()
        }
    );
    if enabled {
        println!("  {} {}", "Mode:".bold(), mode);
    }

    if hw.ac.found {
        let ac_state = if hw.ac.online {
            "AC (plugged in)"
        } else {
            "battery"
        };
        println!("  {} {}", "Power source:".bold(), ac_state);
    } else {
        println!(
            "  {} {}",
            "Power source:".bold(),
            "no AC adapter detected".dimmed()
        );
    }

    println!(
        "  {} {}",
        "Optimizations:".bold(),
        if state_exists {
            "applied".green().to_string()
        } else {
            "not applied".dimmed().to_string()
        }
    );

    Ok(())
}

fn reload_udevd() {
    let _ = std::process::Command::new("udevadm")
        .args(["control", "--reload-rules"])
        .status();
    let _ = std::process::Command::new("udevadm")
        .arg("trigger")
        .args(["--subsystem-match=power_supply"])
        .status();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_udev_rule_normal() {
        let rule = udev_rule_content(false);
        assert!(rule.contains("RUN+=\"/usr/bin/bop auto\""));
        assert!(!rule.contains("--aggressive"));
        assert!(rule.contains("KERNEL!=\"hidpp_battery*\""));
        assert!(rule.contains("SUBSYSTEM==\"power_supply\""));
    }

    #[test]
    fn test_udev_rule_aggressive() {
        let rule = udev_rule_content(true);
        assert!(rule.contains("RUN+=\"/usr/bin/bop --aggressive auto\""));
        assert!(rule.contains("--aggressive"));
    }

    #[test]
    fn test_auto_status_json_serialization() {
        let status = AutoStatus {
            enabled: true,
            mode: Some("normal".to_string()),
            ac_online: true,
            optimizations_applied: false,
        };
        let json = serde_json::to_string_pretty(&status).unwrap();
        assert!(json.contains("\"enabled\": true"));
        assert!(json.contains("\"mode\": \"normal\""));
        assert!(json.contains("\"ac_online\": true"));
        assert!(json.contains("\"optimizations_applied\": false"));
    }
}
