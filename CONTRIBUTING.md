# Contributing to bop

This guide walks you through adding support for a new laptop. If your laptop isn't supported yet, a single profile file and a few lines of registration code are all you need.

## How profiles work

bop uses a pipeline: **detect hardware -> match profile -> audit -> apply/revert**.

1. `HardwareInfo::detect()` reads sysfs/procfs to gather DMI strings, CPU info, GPU info, battery status, PCI devices, and more.
2. `detect_profile()` iterates a registry of `HardwareProfile` implementations and returns the first one whose `matches()` returns true.
3. The matched profile's `audit()` method runs a set of checks and returns `Vec<Finding>` -- each finding describes a suboptimal setting with a severity, current value, recommended value, and weight for scoring.

The trait lives in `src/profile/mod.rs`:

```rust
pub trait HardwareProfile: std::fmt::Debug {
    fn name(&self) -> &str;
    fn matches(&self, hw: &HardwareInfo) -> bool;
    fn audit(&self, hw: &HardwareInfo) -> Vec<Finding>;
}
```

## Step-by-step: adding a new profile

### 1. Identify your laptop's DMI strings

Run these on your laptop to get the values you'll match against:

```bash
cat /sys/class/dmi/id/board_vendor
cat /sys/class/dmi/id/board_name
cat /sys/class/dmi/id/product_name
cat /sys/class/dmi/id/product_family
```

Example output for a ThinkPad X1 Carbon Gen 11:

```
LENOVO
21HMCTO1WW
ThinkPad X1 Carbon Gen 11
ThinkPad X1 Carbon Gen 11
```

Write these down -- you'll use them in your `matches()` implementation and test fixture.

### 2. Capture a system snapshot

Run `bop snapshot` on your laptop to capture every sysfs/procfs path that bop reads (DMI, CPU, GPU, PCI, USB, battery, audio, network, kernel params, ACPI wakeup, sysctl):

```bash
sudo bop snapshot -o my-laptop.json
```

Include the resulting JSON file in your PR at `tests/fixtures/<name>.json` (e.g., `tests/fixtures/thinkpad-x1c11.json`). This snapshot replaces the need to hand-write a full fixture function -- it can be loaded in tests to recreate a complete mock sysfs tree automatically.

### 3. Create the profile file

Create `src/profile/<your_laptop>.rs`. Here's a minimal template:

```rust
use crate::audit::{self, Finding};
use crate::detect::HardwareInfo;
use crate::profile::HardwareProfile;
use crate::sysfs::SysfsRoot;

#[derive(Debug)]
pub struct ThinkpadX1Carbon11;

impl HardwareProfile for ThinkpadX1Carbon11 {
    fn name(&self) -> &str {
        "Lenovo ThinkPad X1 Carbon Gen 11 (Intel)"
    }

    fn matches(&self, hw: &HardwareInfo) -> bool {
        hw.dmi.board_vendor.as_deref().is_some_and(|v| v.contains("LENOVO"))
            && hw.dmi.product_name.as_deref().is_some_and(|n| n.contains("X1 Carbon Gen 11"))
    }

    fn audit(&self, hw: &HardwareInfo) -> Vec<Finding> {
        let sysfs = SysfsRoot::system();
        let mut findings = Vec::new();

        // Generic checks -- these work on any laptop
        findings.extend(audit::cpu_power::check(hw));
        findings.extend(audit::pci_power::check(hw));
        findings.extend(audit::usb_power::check(&sysfs));
        findings.extend(audit::audio::check(&sysfs));
        findings.extend(audit::network_power::check(hw));
        findings.extend(audit::sleep::check(hw, &sysfs));
        findings.extend(audit::services::check());
        findings.extend(audit::sysctl::check(&sysfs));

        // Include these if relevant to your hardware:
        // findings.extend(audit::kernel_params::check(hw));   // Laptop-specific kernel params
        // findings.extend(audit::gpu_power::check(hw));       // AMD/NVIDIA discrete GPU checks
        // findings.extend(audit::display::check(hw, &sysfs)); // eDP panel checks (AMD)

        findings
    }
}
```

### 4. Register it in the profile module

Edit `src/profile/mod.rs`:

```rust
pub mod framework16_amd;
pub mod thinkpad_x1_carbon_11;  // Add your module

// ...

pub fn all_profiles() -> Vec<Box<dyn HardwareProfile>> {
    vec![
        Box::new(framework16_amd::Framework16Amd),
        Box::new(thinkpad_x1_carbon_11::ThinkpadX1Carbon11),  // Add your profile
    ]
}
```

Profiles are checked in order. Put more specific profiles (e.g., a particular CPU variant) before more general ones.

### 5. Optional: add DMI helper methods

If you want reusable detection logic, add methods to `DmiInfo` in `src/detect/dmi.rs`:

```rust
pub fn is_thinkpad_x1_carbon_11(&self) -> bool {
    self.board_vendor.as_deref().is_some_and(|v| v.contains("LENOVO"))
        && self.product_name.as_deref().is_some_and(|n| n.contains("X1 Carbon Gen 11"))
}
```

Then use `hw.dmi.is_thinkpad_x1_carbon_11()` in your `matches()` implementation. This isn't required -- inline matching works fine for a single profile.

## Which audit checks to include

All checks live in `src/audit/` and have one of three signatures:

| Signature | Checks |
|---|---|
| `check(hw: &HardwareInfo) -> Vec<Finding>` | `cpu_power`, `gpu_power`, `pci_power`, `network_power`, `kernel_params` |
| `check(sysfs: &SysfsRoot) -> Vec<Finding>` | `audio`, `usb_power`, `sysctl` |
| `check(hw, sysfs) -> Vec<Finding>` | `sleep`, `display` |
| `check() -> Vec<Finding>` | `services` |

**Generic checks** (safe to include for any laptop):

- `cpu_power` -- EPP, platform profile, scaling driver
- `pci_power` -- ASPM policy, PCI runtime PM
- `usb_power` -- USB autosuspend
- `audio` -- HDA Intel power save
- `network_power` -- WiFi power save (shells out to `iw`)
- `sleep` -- sleep state (s2idle vs deep)
- `services` -- power-hungry systemd services (shells out to `systemctl`)
- `sysctl` -- NMI watchdog, dirty writeback interval

**Hardware-specific checks** (only include if relevant):

- `kernel_params` -- currently checks for Framework-specific params (`acpi.ec_no_wakeup`, `rtc_cmos.use_acpi_alarm`, `amdgpu.abmlevel`). You may need to add your own param checks here or create a new audit module.
- `gpu_power` -- AMD iGPU DPM level and dGPU D3cold state. Only relevant for AMD GPUs.
- `display` -- eDP refresh rate hints and PSR status. Currently AMD-specific.

## Testing

Tests use a mock sysfs tree so you don't need root or real hardware. The easiest way is to load the snapshot you captured in step 2.

### Using a snapshot (recommended)

Load your snapshot file and call `materialize()` to recreate the full sysfs tree in a temp directory:

```rust
use bop::snapshot::Snapshot;
use bop::sysfs::SysfsRoot;
use bop::detect::HardwareInfo;
use tempfile::TempDir;
use std::path::Path;

#[test]
fn test_thinkpad_x1c11_detection() {
    let snap = Snapshot::load(Path::new("tests/fixtures/thinkpad-x1c11.json")).unwrap();
    let tmp = TempDir::new().unwrap();
    let sysfs = snap.materialize(tmp.path()).unwrap();
    let hw = HardwareInfo::detect(&sysfs);

    assert!(hw.dmi.board_vendor.as_deref().unwrap().contains("LENOVO"));
}

#[test]
fn test_thinkpad_x1c11_profile_matches() {
    let snap = Snapshot::load(Path::new("tests/fixtures/thinkpad-x1c11.json")).unwrap();
    let tmp = TempDir::new().unwrap();
    let sysfs = snap.materialize(tmp.path()).unwrap();
    let hw = HardwareInfo::detect(&sysfs);

    let profile = bop::profile::detect_profile(&hw);
    assert!(profile.is_some());
    assert_eq!(profile.unwrap().name(), "Lenovo ThinkPad X1 Carbon Gen 11 (Intel)");
}
```

### Hand-written fixtures (alternative)

If you don't have the hardware and want a minimal, targeted fixture, you can write one manually. See `tests/sysfs_mock.rs` for a full example.

```rust
fn create_thinkpad_x1c11_fixture(root: &Path) {
    let dmi = root.join("sys/class/dmi/id");
    fs::create_dir_all(&dmi).unwrap();
    fs::write(dmi.join("board_vendor"), "LENOVO\n").unwrap();
    fs::write(dmi.join("board_name"), "21HMCTO1WW\n").unwrap();
    fs::write(dmi.join("product_name"), "ThinkPad X1 Carbon Gen 11\n").unwrap();
    // ... add CPU, battery, PCI entries as needed
}
```

This approach is useful when you only need to test specific detection or audit paths without a full system capture.

### Run tests

```bash
cargo test                          # Run all tests
cargo test test_thinkpad            # Run only your new tests by name
cargo test --test sysfs_mock        # Run only integration tests
```

## Build and lint

```bash
cargo check                                                    # Fast compile check
cargo build                                                    # Debug build
cargo clippy --all-targets --all-features -- -D warnings       # Lint (warnings are errors)
cargo fmt --all                                                # Format code
```

Run all four before submitting a PR. CI will reject code with clippy warnings or formatting issues.

## Checklist for your PR

- [ ] Profile file created at `src/profile/<name>.rs`
- [ ] Profile registered in `src/profile/mod.rs` (both `mod` declaration and `all_profiles()`)
- [ ] `matches()` uses DMI strings that uniquely identify your laptop
- [ ] `audit()` includes all generic checks plus any hardware-specific ones
- [ ] Snapshot file included at `tests/fixtures/<name>.json`
- [ ] Tests pass for detection, profile matching, and at least one audit check
- [ ] `cargo clippy` and `cargo fmt` pass cleanly
