use crate::apply::ApplyState;
use crate::detect::HardwareInfo;
use crate::error::{Error, Result};
use crate::preset::Preset;
use crate::sysfs::SysfsRoot;
use colored::Colorize;
use std::fs;
use std::path::{Path, PathBuf};

const UDEV_RULE_PATH: &str = "/etc/udev/rules.d/85-bop.rules";
const LOCK_DIR: &str = "/run/bop";
const LOCK_FILE: &str = "/run/bop/auto.lock";

fn udev_rule_content(cli_preset: Option<Preset>, config_path: Option<&Path>) -> String {
    let mut args = String::from("/usr/bin/bop");
    if let Some(path) = config_path {
        // Resolve to absolute path (udev runs from /) and quote for spaces.
        // Use canonicalize for existing files, fall back to joining with cwd.
        let abs = path.canonicalize().unwrap_or_else(|_| {
            if path.is_absolute() {
                path.to_path_buf()
            } else {
                std::env::current_dir()
                    .map(|cwd| cwd.join(path))
                    .unwrap_or_else(|_| path.to_path_buf())
            }
        });
        let path_str = abs.display().to_string();
        if path_str.contains(' ') || path_str.contains('\'') {
            // Shell-safe single quoting: replace ' with '\'' (end quote, literal ', start quote)
            let escaped = path_str.replace('\'', "'\\''");
            args.push_str(&format!(" --config '{}'", escaped));
        } else {
            args.push_str(&format!(" --config {}", path_str));
        }
    }
    if let Some(preset) = cli_preset {
        args.push_str(&format!(" --preset {}", preset));
    }
    args.push_str(" auto");
    let bin = args;
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
    /// On battery but nothing to change (system already matches preset).
    AlreadyOptimal,
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
        AutoOutcome::AlreadyOptimal => (
            "debug",
            "On battery — system already matches preset, no changes needed",
        ),
        AutoOutcome::NoProfile => ("warning", "No hardware profile matched — skipping"),
        AutoOutcome::NoAcAdapter => ("debug", "No AC adapter detected"),
    };

    let _ = std::process::Command::new("logger")
        .args(["-t", "bop", "-p", &format!("user.{}", priority), message])
        .status();
}

/// Core auto-switching logic. Called by udev or `bop auto`.
pub fn run(cli_preset: Option<Preset>, config: &crate::config::BopConfig) -> Result<AutoOutcome> {
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

        let effective_preset = crate::config::resolve_preset(config, cli_preset);
        let mut knobs = crate::config::resolve_knobs(config, effective_preset);
        let plan = match scope {
            crate::inhibitors::ApplyScope::Reduced => {
                knobs.clamp_for_reduced();
                crate::apply::build_plan_reduced(&hw, &sysfs, &knobs, Some(config))
            }
            _ => crate::apply::build_plan(&hw, &sysfs, &knobs, Some(config)),
        };

        if plan.is_empty() {
            // Dim backlight even for empty plans (e.g. already-optimized system)
            let mut dimmed = false;
            if config.brightness.auto_dim {
                match crate::brightness::dim(&config.brightness, &sysfs) {
                    Ok(Some(original)) => {
                        let state = ApplyState {
                            timestamp: chrono::Utc::now().to_rfc3339(),
                            brightness_original: Some(original),
                            ..Default::default()
                        };
                        state.save()?;
                        dimmed = true;
                    }
                    Ok(None) => {}
                    Err(e) => {
                        eprintln!("{} Failed to dim backlight: {}", "!".yellow(), e);
                    }
                }
            }
            let outcome = if dimmed {
                AutoOutcome::Applied
            } else {
                AutoOutcome::AlreadyOptimal
            };
            log_to_journal(&outcome);

            if dimmed && config.notifications.enabled && config.notifications.on_apply {
                let _ = crate::notify::send("bop", "Power optimizations applied (on battery)");
            }

            return Ok(outcome);
        }

        // Apply optimizations first, then dim backlight only on success.
        // This avoids leaving the screen dimmed with no state to restore
        // if apply fails before any checkpoint.
        let mut state = crate::apply::execute_plan(&plan, &hw, false)?;

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
pub fn enable(
    cli_preset: Option<Preset>,
    config: &crate::config::BopConfig,
    config_path: Option<&Path>,
) -> Result<()> {
    if !nix::unistd::geteuid().is_root() {
        return Err(Error::NotRoot {
            operation: "auto enable".to_string(),
        });
    }

    let effective_preset = crate::config::resolve_preset(config, cli_preset);
    let rule = udev_rule_content(cli_preset, config_path);
    fs::write(UDEV_RULE_PATH, &rule)
        .map_err(|e| Error::Other(format!("failed to write udev rule: {}", e)))?;

    reload_udevd();

    let preset_label = match cli_preset {
        Some(p) => p.to_string(),
        None => format!("{} (from config)", effective_preset),
    };
    println!(
        "{} Auto-switching enabled (preset: {})",
        ">>".green(),
        preset_label.bold()
    );
    println!("  Rule installed at {}", UDEV_RULE_PATH);

    // Apply immediately if currently on battery
    match run(cli_preset, config)? {
        AutoOutcome::Applied => {
            println!("  {} On battery — optimizations applied.", ">>".green());
        }
        AutoOutcome::NoOp => {
            println!("  On AC power — optimizations will apply when unplugged.");
        }
        AutoOutcome::AlreadyOptimal => {
            println!("  On battery — system already matches preset, no changes needed.");
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
    preset: Option<String>,
    ac_online: bool,
    optimizations_applied: bool,
}

/// Show status of auto-switching.
pub fn status(json: bool) -> Result<()> {
    let rule_path = Path::new(UDEV_RULE_PATH);
    let enabled = rule_path.exists();

    let preset_name = if enabled {
        let content = fs::read_to_string(rule_path).unwrap_or_default();
        if content.contains("--aggressive") {
            "supersaver".to_string()
        } else if let Some(pos) = content.find("--preset ") {
            let rest = &content[pos + 9..];
            rest.split_whitespace()
                .next()
                .unwrap_or("moderate")
                .to_string()
        } else {
            // Legacy rule without --preset; actual preset comes from config resolution
            "config-defined".to_string()
        }
    } else {
        "n/a".to_string()
    };

    let sysfs = SysfsRoot::system();
    let hw = HardwareInfo::detect(&sysfs);
    let state_exists = ApplyState::load().ok().and_then(|s| s).is_some();

    if json {
        let status = AutoStatus {
            enabled,
            preset: if enabled {
                Some(preset_name.clone())
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
        println!("  {} {}", "Preset:".bold(), preset_name);
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
    fn test_udev_rule_with_preset() {
        let rule = udev_rule_content(Some(Preset::Moderate), None);
        assert!(rule.contains("--preset moderate"));
        assert!(rule.contains("KERNEL!=\"hidpp_battery*\""));
        assert!(rule.contains("SUBSYSTEM==\"power_supply\""));
    }

    #[test]
    fn test_udev_rule_supersaver() {
        let rule = udev_rule_content(Some(Preset::Supersaver), None);
        assert!(rule.contains("RUN+=\"/usr/bin/bop --preset supersaver auto\""));
        assert!(rule.contains("--preset supersaver"));
    }

    #[test]
    fn test_udev_rule_saver() {
        let rule = udev_rule_content(Some(Preset::Saver), None);
        assert!(rule.contains("--preset saver"));
    }

    #[test]
    fn test_udev_rule_no_preset() {
        let rule = udev_rule_content(None, None);
        assert!(!rule.contains("--preset"));
        assert!(rule.contains("RUN+=\"/usr/bin/bop auto\""));
        assert!(rule.contains("KERNEL!=\"hidpp_battery*\""));
    }

    #[test]
    fn test_udev_rule_with_config_path() {
        let path = Path::new("/etc/bop/custom.toml");
        let rule = udev_rule_content(Some(Preset::Moderate), Some(path));
        assert!(rule.contains("--config /etc/bop/custom.toml"));
        assert!(rule.contains("--preset moderate"));
        assert!(rule.contains(" auto"));
    }

    #[test]
    fn test_udev_rule_config_path_no_preset() {
        let path = Path::new("/etc/bop/custom.toml");
        let rule = udev_rule_content(None, Some(path));
        assert!(rule.contains("--config /etc/bop/custom.toml"));
        assert!(!rule.contains("--preset"));
        assert!(rule.contains(" auto"));
    }

    #[test]
    fn test_auto_status_json_serialization() {
        let status = AutoStatus {
            enabled: true,
            preset: Some("moderate".to_string()),
            ac_online: true,
            optimizations_applied: false,
        };
        let json = serde_json::to_string_pretty(&status).unwrap();
        assert!(json.contains("\"enabled\": true"));
        assert!(json.contains("\"preset\": \"moderate\""));
        assert!(json.contains("\"ac_online\": true"));
        assert!(json.contains("\"optimizations_applied\": false"));
    }
}
