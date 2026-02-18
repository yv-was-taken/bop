pub mod kernel_params;
pub mod services;
pub mod sysfs_writer;
pub mod systemd;

use crate::detect::HardwareInfo;
use crate::error::{Error, Result};
use crate::sysfs::SysfsRoot;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const STATE_DIR: &str = "/var/lib/bop";
const STATE_FILE: &str = "/var/lib/bop/state.json";

/// Represents all changes made by bop, for reverting.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ApplyState {
    pub timestamp: String,
    pub sysfs_changes: Vec<SysfsChange>,
    pub kernel_params_added: Vec<String>,
    pub services_disabled: Vec<String>,
    pub systemd_units_created: Vec<String>,
    pub modprobe_files_created: Vec<String>,
    pub acpi_wakeup_toggled: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SysfsChange {
    pub path: String,
    pub original_value: String,
    pub new_value: String,
}

impl ApplyState {
    pub fn load() -> Result<Option<Self>> {
        let path = PathBuf::from(STATE_FILE);
        if !path.exists() {
            return Ok(None);
        }
        let data = std::fs::read_to_string(&path)
            .map_err(|e| Error::State(format!("failed to read state file: {}", e)))?;
        let state: Self = serde_json::from_str(&data)
            .map_err(|e| Error::State(format!("failed to parse state file: {}", e)))?;
        Ok(Some(state))
    }

    pub fn save(&self) -> Result<()> {
        std::fs::create_dir_all(STATE_DIR)
            .map_err(|e| Error::State(format!("failed to create state dir: {}", e)))?;
        let data = serde_json::to_string_pretty(self)
            .map_err(|e| Error::State(format!("failed to serialize state: {}", e)))?;
        std::fs::write(STATE_FILE, data)
            .map_err(|e| Error::State(format!("failed to write state file: {}", e)))?;
        Ok(())
    }

    pub fn remove_file() -> Result<()> {
        let path = PathBuf::from(STATE_FILE);
        if path.exists() {
            std::fs::remove_file(&path)
                .map_err(|e| Error::State(format!("failed to remove state file: {}", e)))?;
        }
        Ok(())
    }
}

/// Plan of changes to apply.
#[derive(Debug, Clone)]
pub struct ApplyPlan {
    pub sysfs_writes: Vec<PlannedSysfsWrite>,
    pub kernel_params: Vec<String>,
    pub services_to_disable: Vec<String>,
    pub acpi_wakeup_disable: Vec<String>,
    pub systemd_service: bool,
    pub modprobe_configs: Vec<ModprobeConfig>,
}

#[derive(Debug, Clone)]
pub struct PlannedSysfsWrite {
    pub path: String,
    pub value: String,
    pub description: String,
}

#[derive(Debug, Clone)]
pub struct ModprobeConfig {
    pub filename: String,
    pub content: String,
}

/// Build the plan of changes based on audit findings.
pub fn build_plan(hw: &HardwareInfo, sysfs: &SysfsRoot) -> ApplyPlan {
    let mut plan = ApplyPlan {
        sysfs_writes: Vec::new(),
        kernel_params: Vec::new(),
        services_to_disable: Vec::new(),
        acpi_wakeup_disable: Vec::new(),
        systemd_service: true,
        modprobe_configs: Vec::new(),
    };

    // CPU: EPP -> balance_power
    if hw.cpu.epp.as_deref() != Some("balance_power") && hw.cpu.epp.as_deref() != Some("power") {
        if let Ok(cpus) = sysfs.list_dir("sys/devices/system/cpu") {
            for cpu in cpus {
                if cpu.starts_with("cpu") && cpu[3..].chars().all(|c| c.is_ascii_digit()) {
                    let path = format!(
                        "sys/devices/system/cpu/{}/cpufreq/energy_performance_preference",
                        cpu
                    );
                    if sysfs.exists(&path) {
                        plan.sysfs_writes.push(PlannedSysfsWrite {
                            path: format!("/{}", path),
                            value: "balance_power".to_string(),
                            description: format!("Set {} EPP to balance_power", cpu),
                        });
                    }
                }
            }
        }
    }

    // Platform profile -> low-power
    if hw.platform.platform_profile.as_deref() != Some("low-power") {
        plan.sysfs_writes.push(PlannedSysfsWrite {
            path: "/sys/firmware/acpi/platform_profile".to_string(),
            value: "low-power".to_string(),
            description: "Set platform profile to low-power".to_string(),
        });
    }

    // ASPM -> powersupersave
    if hw.pci.aspm_policy.as_deref() != Some("powersupersave") {
        plan.sysfs_writes.push(PlannedSysfsWrite {
            path: "/sys/module/pcie_aspm/parameters/policy".to_string(),
            value: "powersupersave".to_string(),
            description: "Set PCIe ASPM policy to powersupersave".to_string(),
        });
    }

    // PCI runtime PM -> auto
    for dev in &hw.pci.devices {
        if dev.runtime_pm.as_deref() != Some("auto") {
            plan.sysfs_writes.push(PlannedSysfsWrite {
                path: format!("/sys/bus/pci/devices/{}/power/control", dev.address),
                value: "auto".to_string(),
                description: format!("Enable runtime PM for PCI {}", dev.address),
            });
        }
    }

    // Kernel params
    if !hw.has_kernel_param("acpi.ec_no_wakeup") {
        plan.kernel_params.push("acpi.ec_no_wakeup=1".to_string());
    }
    if !hw.has_kernel_param("rtc_cmos.use_acpi_alarm") {
        plan.kernel_params
            .push("rtc_cmos.use_acpi_alarm=1".to_string());
    }
    if hw.gpu.is_amd() && !hw.has_kernel_param("amdgpu.abmlevel") {
        plan.kernel_params.push("amdgpu.abmlevel=3".to_string());
    }

    // Services to disable
    for svc in &["tlp.service", "power-profiles-daemon.service"] {
        if is_service_active_or_enabled(svc) {
            plan.services_to_disable.push(svc.to_string());
        }
    }

    // ACPI wakeup sources to disable
    for source in &hw.platform.acpi_wakeup_sources {
        if source.enabled && source.device != "XHC0" {
            // Check if it has devices (keep enabled if so)
            // For the plan, we'll mark it for disabling; the execution will double-check
            plan.acpi_wakeup_disable.push(source.device.clone());
        }
    }

    plan
}

fn is_service_active_or_enabled(service: &str) -> bool {
    std::process::Command::new("systemctl")
        .args(["is-active", "--quiet", service])
        .status()
        .is_ok_and(|s| s.success())
        || std::process::Command::new("systemctl")
            .args(["is-enabled", "--quiet", service])
            .status()
            .is_ok_and(|s| s.success())
}

/// Execute the apply plan.
pub fn execute_plan(plan: &ApplyPlan, hw: &HardwareInfo, dry_run: bool) -> Result<ApplyState> {
    if !dry_run && !nix::unistd::geteuid().is_root() {
        return Err(Error::NotRoot {
            operation: "apply".to_string(),
        });
    }

    // Check for conflicts
    check_conflicts()?;

    let mut state = ApplyState {
        timestamp: chrono::Utc::now().to_rfc3339(),
        ..Default::default()
    };

    let sysfs = SysfsRoot::system();

    // Apply sysfs writes
    for write in &plan.sysfs_writes {
        let relative = write.path.strip_prefix('/').unwrap_or(&write.path);
        let original = sysfs.read_optional(relative).unwrap_or(None).unwrap_or_default();

        if dry_run {
            println!(
                "  [dry-run] {} -> {} (was: {})",
                write.path, write.value, original
            );
        } else {
            sysfs_writer::write_sysfs(&write.path, &write.value)?;
            state.sysfs_changes.push(SysfsChange {
                path: write.path.clone(),
                original_value: original,
                new_value: write.value.clone(),
            });
        }
    }

    // ACPI wakeup toggling
    for device in &plan.acpi_wakeup_disable {
        if dry_run {
            println!("  [dry-run] Disable ACPI wakeup: {}", device);
        } else {
            // /proc/acpi/wakeup is a toggle - check current state first
            if is_wakeup_enabled(device, &sysfs) {
                sysfs_writer::toggle_acpi_wakeup(device)?;
                state.acpi_wakeup_toggled.push(device.clone());
            }
        }
    }

    // Kernel params
    if !plan.kernel_params.is_empty() {
        if dry_run {
            println!(
                "  [dry-run] Add kernel params: {}",
                plan.kernel_params.join(" ")
            );
        } else {
            kernel_params::add_kernel_params(&plan.kernel_params)?;
            state.kernel_params_added = plan.kernel_params.clone();
        }
    }

    // Service management
    for svc in &plan.services_to_disable {
        if dry_run {
            println!("  [dry-run] Disable service: {}", svc);
        } else {
            services::disable_service(svc)?;
            state.services_disabled.push(svc.clone());
        }
    }

    // Generate systemd oneshot service
    if plan.systemd_service && !plan.sysfs_writes.is_empty() {
        if dry_run {
            println!("  [dry-run] Generate bop-powersave.service");
        } else {
            let unit_path = systemd::generate_service(hw, plan)?;
            state
                .systemd_units_created
                .push(unit_path.to_string_lossy().into_owned());
            systemd::enable_service()?;
        }
    }

    // Save state
    if !dry_run {
        state.save()?;
    }

    Ok(state)
}

fn check_conflicts() -> Result<()> {
    if std::process::Command::new("systemctl")
        .args(["is-active", "--quiet", "tlp.service"])
        .status()
        .is_ok_and(|s| s.success())
    {
        return Err(Error::ConflictingService(
            "TLP is currently running. Stop it first: sudo systemctl stop tlp && sudo systemctl disable tlp".to_string(),
        ));
    }
    Ok(())
}

fn is_wakeup_enabled(device: &str, sysfs: &SysfsRoot) -> bool {
    if let Ok(wakeup) = sysfs.read("proc/acpi/wakeup") {
        for line in wakeup.lines() {
            if line.starts_with(device) {
                return line.contains("*enabled");
            }
        }
    }
    false
}

pub fn print_plan(plan: &ApplyPlan) {
    use colored::Colorize;

    println!("{}", "Apply Plan".bold().underline());
    println!();

    if !plan.sysfs_writes.is_empty() {
        println!("  {} Runtime sysfs changes:", ">>".cyan());
        for write in &plan.sysfs_writes {
            println!("     {} {}", write.description.dimmed(), write.path.dimmed());
        }
        println!();
    }

    if !plan.kernel_params.is_empty() {
        println!("  {} Kernel parameters (requires reboot):", ">>".cyan());
        for param in &plan.kernel_params {
            println!("     {}", param);
        }
        println!();
    }

    if !plan.services_to_disable.is_empty() {
        println!("  {} Services to disable:", ">>".cyan());
        for svc in &plan.services_to_disable {
            println!("     {}", svc);
        }
        println!();
    }

    if !plan.acpi_wakeup_disable.is_empty() {
        println!(
            "  {} ACPI wakeup sources to disable (volatile, resets on reboot):",
            ">>".cyan()
        );
        for dev in &plan.acpi_wakeup_disable {
            println!("     {}", dev);
        }
        println!();
    }

    if plan.systemd_service {
        println!(
            "  {} Will generate bop-powersave.service for boot persistence",
            ">>".cyan()
        );
        println!();
    }
}
