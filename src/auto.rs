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

/// Core auto-switching logic. Called by udev or `bop auto`.
pub fn run(aggressive: bool, _config: &crate::config::BopConfig) -> Result<AutoOutcome> {
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
        return Ok(AutoOutcome::NoAcAdapter);
    }

    let profile = crate::profile::detect_profile(&hw);
    if profile.is_none() {
        return Ok(AutoOutcome::NoProfile);
    }

    let state_exists = ApplyState::load()?.is_some();

    if hw.ac.is_on_battery() && !state_exists {
        // On battery, no optimizations applied — apply them
        let plan = if aggressive {
            crate::apply::build_plan_aggressive(&hw, &sysfs)
        } else {
            crate::apply::build_plan(&hw, &sysfs)
        };
        crate::apply::execute_plan(&plan, &hw, false)?;
        Ok(AutoOutcome::Applied)
    } else if hw.ac.is_on_ac() && state_exists {
        // On AC, optimizations applied — revert them
        crate::revert::revert()?;
        Ok(AutoOutcome::Reverted)
    } else {
        Ok(AutoOutcome::NoOp)
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

/// Show status of auto-switching.
pub fn status() -> Result<()> {
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
}
