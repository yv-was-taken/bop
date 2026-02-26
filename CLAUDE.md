# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build & Test Commands

```bash
cargo check                    # Fast compile validation during development
cargo build                    # Build debug binary
cargo build --release          # Build optimized binary at target/release/bop
cargo test                     # Run all tests (unit + integration)
cargo test test_name           # Run a single test by name
cargo test --test sysfs_mock   # Run only integration tests
cargo clippy --all-targets --all-features -- -D warnings  # Lint (treat warnings as errors)
cargo fmt --all                # Format
cargo run -- audit             # Run audit on current system
cargo run -- audit --json      # JSON output
cargo run -- apply --dry-run   # Preview changes without applying
cargo run -- wake list         # List ACPI wakeup controllers
cargo install --path .         # Install locally for end-to-end testing
```

## Architecture

Pipeline: **CLI → detect → profile match → audit → apply/revert**

- **`sysfs.rs`** — `SysfsRoot` abstracts all filesystem I/O. Defaults to `/` in production, redirectable to a temp dir for testing. Every module that reads sysfs/procfs takes a `&SysfsRoot` parameter.

- **`detect/*`** — Each module (dmi, cpu, gpu, battery, pci, network, platform) reads sysfs and returns a typed struct. `HardwareInfo::detect()` composes all of them.

- **`profile/*`** — `HardwareProfile` trait with `matches()` and `audit()`. `detect_profile()` iterates the registry to find the first match. Currently only `Framework16Amd`. Adding a new laptop = new file implementing the trait.

- **`audit/*`** — Each file (cpu_power, gpu_power, pci_power, kernel_params, sleep, services, etc.) is a check function returning `Vec<Finding>`. The profile's `audit()` method calls all relevant checks. `Finding` uses a builder pattern with a weight-based scoring system (0-100).

- **`apply/*`** — Two-phase: `build_plan()` produces an `ApplyPlan` (pure data, no side effects), then `execute_plan()` applies it and saves an `ApplyState` to `/var/lib/bop/state.json` for reverting. Persistence layers: sysfs writes, kernel params (systemd-boot entries), systemd oneshot service generation, service management.

- **`revert/`** — Reads `ApplyState` from disk and undoes everything: restores sysfs values, removes kernel params, re-enables services, removes generated systemd units.

- **`wake/`** — Framework-specific ACPI wakeup source management. Traces PCI → USB root hub → USB device chains to determine which controllers have real devices attached.

- **`monitor/`** — Real-time RAPL energy counters + battery discharge rate.

## Testing Pattern

Tests use a mock sysfs fixture (see `tests/sysfs_mock.rs`):

```rust
let tmp = TempDir::new().unwrap();
create_framework16_fixture(tmp.path());  // Populates temp dir with realistic sysfs tree
let sysfs = SysfsRoot::new(tmp.path());
let hw = HardwareInfo::detect(&sysfs);   // Works against mock data
```

This enables testing detection, profile matching, audit checks, and scoring without root access or real hardware. When adding new audit checks or detection modules, extend the fixture and add corresponding test assertions.

## Volatile vs Persistent Changes

- **Volatile** (runtime, reset on reboot): EPP, platform profile, ASPM policy, PCI runtime PM, WiFi power save, ACPI wakeup toggles
- **Boot-persistent** (require reboot): Kernel params added to systemd-boot entries
- **Persistence bridge**: `bop-powersave.service` (generated systemd oneshot) re-applies volatile sysfs settings and ACPI wakeup config at every boot

## Git Workflow

- **Never merge PRs or push directly to master** unless the user explicitly says to merge. Always create PRs for review.
- When creating PRs, sort by code significance: correctness bugs first, then data-loss bugs, then behavioral fixes.
- Use `git worktree` for parallel fix branches. Worktrees live at `~/Projects/bop/<branch-name>/`.
- Commit subjects: descriptive, imperative, scoped when possible (e.g., `audit: flag disabled wifi powersave`).

## Key Details

- The real Framework 16 board name is `FRANMZCP09` (not containing "16"), so `is_framework_16()` checks `product_name` which contains "Laptop 16".
- `/proc/acpi/wakeup` uses a **toggle** interface — writing a device name flips its state. Code must check current state before writing to avoid toggling the wrong direction.
- ACPI wakeup changes are volatile (reset on reboot). The generated systemd oneshot service re-applies them at boot.
- `nix` crate requires the `user` feature for `geteuid()`.
- Some audit checks shell out to external tools (`iw` for WiFi power save, `systemctl` for service status).
