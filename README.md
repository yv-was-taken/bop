# bop

Hardware-aware battery optimization for Linux laptops. Audits your system for power waste, applies fixes, and lets you undo everything.

Built for Framework Laptop 16 (AMD Ryzen 7040). Single binary, no daemon, no config files.

## Why not TLP?

Framework and AMD engineers [explicitly recommend against TLP on AMD systems](https://community.frame.work/t/tracking-ppd-v-tlp-for-amd-ryzen-7040/39423). TLP's default config is tuned for Intel and actively fights `amd-pstate`. bop is built for AMD first, applies settings as a one-shot rather than running a daemon, and knows about Framework-specific hardware (expansion card USB controllers, CrosEC, etc.).

## What it does

```
$ bop audit

Hardware Detection
╭──────────────────┬────────────────────────────────────────────╮
│ Component        ┆ Detected                                   │
╞══════════════════╪════════════════════════════════════════════╡
│ Board            ┆ Framework FRANMZCP09                       │
│ Product          ┆ Laptop 16 (AMD Ryzen 7040 Series)          │
│ CPU              ┆ AMD Ryzen 9 7940HS w/ Radeon 780M Graphics │
│ CPU Driver       ┆ amd-pstate-epp                             │
│ EPP              ┆ balance_performance                        │
│ GPU Driver       ┆ amdgpu                                     │
│ Platform Profile ┆ performance                                │
│ ASPM Policy      ┆ default                                    │
│ WiFi             ┆ wlan0 (mt7921e)                            │
╰──────────────────┴────────────────────────────────────────────╯

  Matched profile: Framework Laptop 16 (AMD Ryzen 7040 Series)

Audit Findings
╭──────┬──────────┬────────────────────────────────────────────────┬─────────────────────┬──────────────────────┬────────────────────────────────╮
│ Sev  ┆ Category ┆ Finding                                        ┆ Current             ┆ Recommended          ┆ Impact                         │
╞══════╪══════════╪════════════════════════════════════════════════╪═════════════════════╪══════════════════════╪════════════════════════════════╡
│ HIGH ┆ Kernel   ┆ EC wakeup not disabled - high sleep drain      ┆ unset               ┆ acpi.ec_no_wakeup=1  ┆ ~5-7% sleep drain reduction    │
│ HIGH ┆ CPU      ┆ Platform profile set to performance (TDP: 45W) ┆ performance         ┆ low-power            ┆ ~1-2W savings at idle          │
│ HIGH ┆ Services ┆ tlp.service is active - conflicts with AMD     ┆ active (running)    ┆ disable and stop     ┆ Actively harmful               │
│ MED  ┆ CPU      ┆ EPP at balance_performance                     ┆ balance_performance ┆ balance_power        ┆ ~1-3W savings                  │
│ MED  ┆ PCIe     ┆ ASPM policy not using deepest sleep states     ┆ default             ┆ powersupersave       ┆ ~0.5-1W savings                │
│ MED  ┆ PCIe     ┆ 36/40 PCI devices without runtime PM           ┆ 36 set to 'on'      ┆ All set to 'auto'    ┆ ~0.5W savings                  │
│ MED  ┆ Network  ┆ WiFi power save disabled                       ┆ off                 ┆ on                   ┆ ~0.5W savings                  │
│ MED  ┆ Sleep    ┆ 9 unnecessary ACPI wakeup sources enabled      ┆ 9 enabled           ┆ Disable all but XHC0 ┆ Reduces spurious wakeups       │
│ ...  ┆          ┆                                                ┆                     ┆                      ┆                                │
╰──────┴──────────┴────────────────────────────────────────────────┴─────────────────────┴──────────────────────┴────────────────────────────────╯

  Power Optimization Score: 51/100
```

Additional checks include: amd-pstate driver not active (~2-5W), NVMe APST disabled (~0.5-1W), and discrete GPU not in D3cold (~5-8W).

On a Framework 16 (7940HS, 61Wh battery, ~50% brightness, light browsing/coding), fixing these issues typically saves 4-8W, extending battery life from ~5-6 hours to ~8-12 hours. Your results will vary with workload, brightness, and expansion card configuration.

## Install

```bash
cargo install --path .
```

### Arch Linux (AUR)

```bash
# With an AUR helper
yay -S bop

# Manual
git clone https://aur.archlinux.org/bop.git
cd bop && makepkg -si
```

### Man page

```bash
man bop
```

Man pages are installed automatically via the AUR package. For manual installs, generate with:

```bash
cargo run --bin manpage
sudo install -Dm644 man/bop.1 /usr/share/man/man1/bop.1
```

## Usage

```bash
# Scan your system and see what's wrong
bop audit

# Check if applied optimizations are still active
bop status

# See exactly what would change (no root required)
bop apply --dry-run

# Apply all optimizations (interactive confirmation)
sudo bop apply

# Undo everything
sudo bop revert

# Real-time power monitoring (RAPL + battery)
bop monitor

# Manage Framework expansion card wakeup sources
bop wake list
sudo bop wake scan          # auto-detect and configure
sudo bop wake enable XHC1   # enable specific controller

# Generate shell completions (auto-detects shell)
bop completions

# Or specify: bash, zsh, fish, elvish, powershell
bop completions zsh
```

JSON output is available for all commands with `--json`.

## What it changes

### Runtime (immediate, reverted on reboot without the generated service)

| Tunable | Before | After | Tradeoff |
|---------|--------|-------|----------|
| EPP | `balance_performance` | `balance_power` | Imperceptible for browsing/coding. ~5% slower sustained compilation. |
| Platform profile | `performance` | `low-power` | TDP 45W→30W. No effect on light tasks. ~10-15% slower sustained heavy loads. |
| ASPM policy | `default` | `powersupersave` | Adds ~2-10us wake latency on first PCI access. Imperceptible. |
| PCI runtime PM | `on` (36 devices) | `auto` (all) | Idle devices enter low-power state. No practical downside. |
| WiFi power save | `off` | `on` | ~50-200ms latency on first packet after idle. |
| ACPI wakeup | 10 sources enabled | 1 (XHC0 only) | Volatile, resets on reboot. Keyboard/lid/power button still work. Run `bop wake list` to verify for your firmware/expansion card config. |
| USB autosuspend | `on` (per device) | `auto` (all) | Idle USB devices enter low-power state. No practical downside. |
| Audio power save | `0` (disabled) | `1` (1 second) | HDA codec powers down after 1s idle. May cause faint pop on wake. |
| GPU DPM | `high`/`manual` | `auto` | GPU dynamically scales power. No downside for desktop/light use. |

### Boot-persistent (require reboot)

| Parameter | Effect |
|-----------|--------|
| `acpi.ec_no_wakeup=1` | Prevents EC events from waking CPU during s2idle. Biggest single impact on sleep drain. |
| `rtc_cmos.use_acpi_alarm=1` | ACPI alarm instead of legacy RTC. Enables deepest sleep states. |
| `amdgpu.abmlevel=3` | Adaptive backlight management. ~0.5-1W display savings. Subtle change in deep blacks. |

### Services

| Service | Action | Why |
|---------|--------|-----|
| TLP | Disabled | Framework + AMD say don't use TLP on AMD. Default config fights amd-pstate. |
| Docker | Info only | Reports power impact but does not touch it. |

### Persistence

bop generates a `bop-powersave.service` (systemd oneshot) that re-applies runtime sysfs settings and ACPI wakeup configuration on every boot. Kernel parameters are written to systemd-boot loader entries. Only systemd-boot is currently supported; GRUB and rEFInd users must add kernel parameters manually.

All changes are recorded in `/var/lib/bop/state.json`. Running `sudo bop revert` restores everything to the original state.

## Supported hardware

| Laptop | Status |
|--------|--------|
| Framework Laptop 16 (AMD Ryzen 7040) | Full profile with all optimizations |
| Everything else | Hardware detection works, but no optimization profile. PRs welcome. |

Adding a new laptop is one Rust file implementing the `HardwareProfile` trait.

## License

MIT
