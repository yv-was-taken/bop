# bop status command — Design

## Goal

Add `bop status` to show what bop changed and whether those changes are still active. Detects drift where something has reset a value since last apply.

## Approach

State-first with drift detection. Load the saved `ApplyState` from `/var/lib/bop/state.json`, then verify each recorded change against the live system.

## Output

```
$ bop status

bop status (applied 2026-02-18T00:00:00Z)

Sysfs Optimizations (10/12 active)
  ✓ /sys/firmware/acpi/platform_profile          low-power
  ✗ /sys/devices/system/cpu/cpu0/cpufreq/energy_performance_preference
      expected: balance_power  actual: performance

ACPI Wakeup (3 sources disabled)
  ✓ XHC1 disabled

Kernel Parameters (3 added)
  ✓ acpi.ec_no_wakeup=1
  ⏳ rtc_cmos.use_acpi_alarm=1 (pending reboot)

Services (1 disabled)
  ✓ tlp.service

Systemd Persistence
  ✓ bop-powersave.service installed

Summary: 16/18 optimizations active, 2 drifted
```

No state file: `No optimizations applied. Run sudo bop apply to get started.`

Supports `--json` via the existing global flag.

## Architecture

### New module: `src/status/mod.rs`

Public function `check() -> Result<StatusReport>` builds a pure-data report. No printing.

```rust
pub struct StatusReport {
    pub timestamp: String,
    pub sysfs: Vec<SysfsStatus>,
    pub acpi_wakeup: Vec<WakeupStatus>,
    pub kernel_params: Vec<KernelParamStatus>,
    pub services: Vec<ServiceStatus>,
    pub systemd_unit: Option<UnitStatus>,
}

pub struct SysfsStatus {
    pub path: String,
    pub expected: String,
    pub actual: Option<String>,  // None if path doesn't exist
    pub active: bool,
}

pub struct WakeupStatus {
    pub device: String,
    pub expected_disabled: bool,
    pub actual_disabled: bool,
}

pub struct KernelParamStatus {
    pub param: String,           // e.g. "acpi.ec_no_wakeup=1"
    pub in_cmdline: bool,        // present in /proc/cmdline
}

pub struct ServiceStatus {
    pub name: String,
    pub still_stopped: bool,
}

pub struct UnitStatus {
    pub path: String,
    pub exists: bool,
}
```

### Verification methods

| Category | How verified |
|----------|-------------|
| Sysfs changes | `std::fs::read_to_string(path)`, compare to `new_value` |
| ACPI wakeup | Parse `/proc/acpi/wakeup`, check device is `*disabled` |
| Kernel params | Parse `/proc/cmdline` for param presence |
| Services | `systemctl is-active --quiet` |
| Systemd unit | `Path::exists()` on recorded path |

### Rendering

Add `print_status()` and `print_status_json()` to `src/output/mod.rs`.

### CLI

Add `Status` variant to `Command` enum in `src/cli.rs`. No extra flags.

### Edge cases

- **No state file**: message + exit 0
- **Corrupt state file**: warn and exit gracefully
- **Sysfs path gone** (device removed): mark "unknown", not "drifted"
- **Kernel param in boot entry but not in `/proc/cmdline`**: show "pending reboot"
- **Service re-enabled**: caught by `systemctl` check

## Testing

Unit tests with temp dir mock:
- Build an `ApplyState`, write matching/mismatching sysfs files, verify `SysfsStatus.active`
- Kernel param check against a mock `/proc/cmdline` file (needs `SysfsRoot` or direct path param)
- ACPI wakeup check against a mock `/proc/acpi/wakeup` file
