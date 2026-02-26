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

### 2. Create the profile file

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

### 3. Register it in the profile module

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

### 4. Optional: add DMI helper methods

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

Tests use a mock sysfs fixture so you don't need root or real hardware. See `tests/sysfs_mock.rs` for the full example.

### Create a fixture for your laptop

Add a fixture function in `tests/sysfs_mock.rs` (or a new test file):

```rust
fn create_thinkpad_x1c11_fixture(root: &Path) {
    // DMI
    let dmi = root.join("sys/class/dmi/id");
    fs::create_dir_all(&dmi).unwrap();
    fs::write(dmi.join("board_vendor"), "LENOVO\n").unwrap();
    fs::write(dmi.join("board_name"), "21HMCTO1WW\n").unwrap();
    fs::write(dmi.join("product_name"), "ThinkPad X1 Carbon Gen 11\n").unwrap();
    fs::write(dmi.join("product_family"), "ThinkPad X1 Carbon Gen 11\n").unwrap();
    fs::write(dmi.join("bios_version"), "1.20\n").unwrap();

    // CPU (Intel example)
    let cpu_base = root.join("sys/devices/system/cpu");
    fs::create_dir_all(cpu_base.join("cpufreq")).unwrap();

    let cpuinfo = "processor\t: 0\nvendor_id\t: GenuineIntel\ncpu family\t: 6\nmodel\t\t: 186\nmodel name\t: 13th Gen Intel(R) Core(TM) i7-1365U\n\n";
    fs::create_dir_all(root.join("proc")).unwrap();
    fs::write(root.join("proc/cpuinfo"), cpuinfo).unwrap();

    // ... add CPU entries, battery, PCI, etc. as needed
    // Use the Framework 16 fixture as a reference for what paths to populate.
}
```

### Write tests for detection and profile matching

```rust
#[test]
fn test_thinkpad_x1c11_detection() {
    let tmp = TempDir::new().unwrap();
    create_thinkpad_x1c11_fixture(tmp.path());

    let sysfs = SysfsRoot::new(tmp.path());
    let hw = HardwareInfo::detect(&sysfs);

    assert!(hw.dmi.board_vendor.as_deref().unwrap().contains("LENOVO"));
}

#[test]
fn test_thinkpad_x1c11_profile_matches() {
    let tmp = TempDir::new().unwrap();
    create_thinkpad_x1c11_fixture(tmp.path());

    let sysfs = SysfsRoot::new(tmp.path());
    let hw = HardwareInfo::detect(&sysfs);

    let profile = profile::detect_profile(&hw);
    assert!(profile.is_some());
    assert_eq!(profile.unwrap().name(), "Lenovo ThinkPad X1 Carbon Gen 11 (Intel)");
}
```

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
- [ ] Test fixture created with realistic sysfs data from your laptop
- [ ] Tests pass for detection, profile matching, and at least one audit check
- [ ] `cargo clippy` and `cargo fmt` pass cleanly
