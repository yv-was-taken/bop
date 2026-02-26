# `bop status` Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add a `bop status` command that loads saved ApplyState and verifies each change against live system values, detecting drift.

**Architecture:** New `src/status/mod.rs` module with a pure-data `StatusReport` struct built by `check()`. Rendering in `src/output/mod.rs`. CLI wiring in `src/cli.rs` and `src/main.rs`. Tests use temp dir mocks matching existing patterns.

**Tech Stack:** Rust 2024, serde, colored, clap, tempfile (tests)

**Design doc:** `docs/plans/2026-02-25-status-command-design.md`

---

### Task 1: Add Status variant to CLI

**Files:**
- Modify: `src/cli.rs:18-41` (Command enum)
- Modify: `src/main.rs:11-17` (match arm)

**Step 1: Add Status variant to Command enum**

In `src/cli.rs`, add after the `Revert` variant (line 34):

```rust
/// Show current optimization state and detect drift
Status,
```

**Step 2: Add match arm in main.rs**

In `src/main.rs:11-17`, add to the match block:

```rust
Command::Status => cmd_status(cli.json)?,
```

Add a stub function at the bottom of `main.rs`:

```rust
fn cmd_status(_json: bool) -> Result<()> {
    println!("status: not yet implemented");
    Ok(())
}
```

**Step 3: Verify it compiles**

Run: `cargo check`
Expected: exit 0

**Step 4: Commit**

```
cli: add Status command variant (stub)
```

---

### Task 2: Create status module with StatusReport types and sysfs check logic

**Files:**
- Create: `src/status/mod.rs`
- Modify: `src/lib.rs:1-11` (add `pub mod status;`)

**Step 1: Write the failing test for sysfs drift detection**

Create `src/status/mod.rs` with the types, the `check()` function signature, and tests:

```rust
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
                expected: change.new_value.clone(),
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
            // Lines look like: "XHC1	  S0	*disabled   pci:0000:00:08.1"
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
        assert!(!result[0].active, "XHC1 re-enabled should be detected as drift");
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
                SysfsStatus { path: "a".into(), expected: "x".into(), actual: Some("x".into()), active: true },
                SysfsStatus { path: "b".into(), expected: "y".into(), actual: Some("z".into()), active: false },
            ],
            acpi_wakeup: vec![
                WakeupStatus { device: "XHC1".into(), active: true },
            ],
            kernel_params: vec![
                KernelParamStatus { param: "foo=1".into(), in_cmdline: true },
                KernelParamStatus { param: "bar=1".into(), in_cmdline: false },
            ],
            services: vec![],
            systemd_unit: Some(UnitStatus { path: "/etc/systemd/system/bop.service".into(), exists: true }),
        };

        assert_eq!(report.total_count(), 6);
        assert_eq!(report.active_count(), 4);
        assert_eq!(report.drifted_count(), 2);
    }
}
```

**Step 2: Register the module in lib.rs**

In `src/lib.rs`, add `pub mod status;` (alphabetically, after `revert`):

```rust
pub mod status;
```

**Step 3: Run the tests**

Run: `cargo test status`
Expected: 8 tests pass (the 7 unit tests above plus any from module resolution)

**Step 4: Commit**

```
status: add module with StatusReport types and check logic
```

---

### Task 3: Add output rendering for status

**Files:**
- Modify: `src/output/mod.rs` (add `print_status` and `print_status_json`)

**Step 1: Add print_status function**

At the bottom of `src/output/mod.rs`, add:

```rust
use crate::status::StatusReport;

pub fn print_status(report: &StatusReport) {
    println!(
        "{} (applied {})",
        "bop status".bold(),
        report.timestamp.dimmed()
    );
    println!();

    // Sysfs
    if !report.sysfs.is_empty() {
        let active = report.sysfs.iter().filter(|s| s.active).count();
        let total = report.sysfs.len();
        println!(
            "  {} Sysfs Optimizations ({}/{})",
            ">>".cyan(),
            active,
            total
        );
        for s in &report.sysfs {
            if s.active {
                println!("     {} {}  {}", "✓".green(), s.path.dimmed(), s.expected);
            } else if let Some(actual) = &s.actual {
                println!("     {} {}", "✗".red(), s.path);
                println!(
                    "       expected: {}  actual: {}",
                    s.expected.green(),
                    actual.red()
                );
            } else {
                println!("     {} {}  (path not found)", "?".yellow(), s.path);
            }
        }
        println!();
    }

    // ACPI wakeup
    if !report.acpi_wakeup.is_empty() {
        let active = report.acpi_wakeup.iter().filter(|w| w.active).count();
        let total = report.acpi_wakeup.len();
        println!(
            "  {} ACPI Wakeup ({}/{} disabled)",
            ">>".cyan(),
            active,
            total
        );
        for w in &report.acpi_wakeup {
            if w.active {
                println!("     {} {} disabled", "✓".green(), w.device);
            } else {
                println!("     {} {} re-enabled (drifted)", "✗".red(), w.device);
            }
        }
        println!();
    }

    // Kernel params
    if !report.kernel_params.is_empty() {
        let active = report.kernel_params.iter().filter(|k| k.in_cmdline).count();
        let total = report.kernel_params.len();
        println!(
            "  {} Kernel Parameters ({}/{})",
            ">>".cyan(),
            active,
            total
        );
        for k in &report.kernel_params {
            if k.in_cmdline {
                println!("     {} {}", "✓".green(), k.param);
            } else {
                println!("     {} {} (pending reboot)", "⏳".yellow(), k.param);
            }
        }
        println!();
    }

    // Services
    if !report.services.is_empty() {
        let active = report.services.iter().filter(|s| s.still_stopped).count();
        let total = report.services.len();
        println!(
            "  {} Services ({}/{} stopped)",
            ">>".cyan(),
            active,
            total
        );
        for s in &report.services {
            if s.still_stopped {
                println!("     {} {} stopped", "✓".green(), s.name);
            } else {
                println!("     {} {} running (drifted)", "✗".red(), s.name);
            }
        }
        println!();
    }

    // Systemd unit
    if let Some(unit) = &report.systemd_unit {
        println!("  {} Systemd Persistence", ">>".cyan());
        if unit.exists {
            println!("     {} {} installed", "✓".green(), unit.path);
        } else {
            println!("     {} {} missing", "✗".red(), unit.path);
        }
        println!();
    }

    // Summary
    let active = report.active_count();
    let total = report.total_count();
    let drifted = report.drifted_count();
    if drifted == 0 {
        println!(
            "  {}",
            format!("All {total} optimizations active.").green().bold()
        );
    } else {
        println!(
            "  {}",
            format!("{active}/{total} optimizations active, {drifted} drifted")
                .yellow()
                .bold()
        );
    }
}

pub fn print_status_json(report: &StatusReport) {
    println!(
        "{}",
        serde_json::to_string_pretty(report).unwrap()
    );
}
```

**Step 2: Verify it compiles**

Run: `cargo check`
Expected: exit 0

**Step 3: Commit**

```
output: add status report rendering
```

---

### Task 4: Wire up cmd_status in main.rs

**Files:**
- Modify: `src/main.rs` (replace stub with real implementation)

**Step 1: Replace the stub cmd_status**

```rust
fn cmd_status(json: bool) -> Result<()> {
    let report = match bop::status::check()? {
        Some(r) => r,
        None => {
            println!(
                "{}",
                "No optimizations applied. Run `sudo bop apply` to get started."
                    .yellow()
            );
            return Ok(());
        }
    };

    if json {
        bop::output::print_status_json(&report);
    } else {
        bop::output::print_status(&report);
    }

    Ok(())
}
```

Add `use colored::Colorize;` if not already imported (it is, on line 6).

**Step 2: Verify everything compiles and tests pass**

Run: `cargo check && cargo test`
Expected: all pass

**Step 3: Commit**

```
status: wire up cmd_status with state loading and rendering
```

---

### Task 5: Run full verification and final commit

**Step 1: Full verification suite**

Run each:
- `cargo check`
- `cargo test`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo fmt --all -- --check`

Expected: all exit 0, all tests pass, no clippy warnings, no fmt diffs.

**Step 2: Fix any issues found**

If clippy or fmt report issues, fix them before proceeding.

**Step 3: Final commit if any fixups were needed**

```
status: fix clippy/fmt issues
```

---

### Task 6: Update README and CLI help

**Files:**
- Modify: `README.md:59-83` (Usage section)

**Step 1: Add status to the Usage section**

After the `bop audit` line in the usage block, add:

```bash
# Check if applied optimizations are still active
bop status
```

**Step 2: Commit**

```
docs: add bop status to README usage section
```
