pub mod kernel_params;
pub mod services;
pub mod sysfs_writer;
pub mod systemd;

use crate::detect::HardwareInfo;
use crate::error::{Error, Result};
use crate::sysfs::SysfsRoot;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
#[cfg(test)]
use std::sync::{LazyLock, Mutex};

const STATE_DIR: &str = "/var/lib/bop";
const STATE_FILE: &str = "/var/lib/bop/state.json";

#[cfg(test)]
static STATE_FILE_OVERRIDE: LazyLock<Mutex<Option<PathBuf>>> = LazyLock::new(|| Mutex::new(None));

fn state_file_path() -> PathBuf {
    #[cfg(test)]
    {
        if let Some(path) = STATE_FILE_OVERRIDE
            .lock()
            .expect("state file override lock poisoned")
            .clone()
        {
            return path;
        }
    }

    PathBuf::from(STATE_FILE)
}

fn state_dir_path() -> PathBuf {
    state_file_path()
        .parent()
        .map(std::path::Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from(STATE_DIR))
}

/// Represents all changes made by bop, for reverting.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ApplyState {
    pub timestamp: String,
    pub sysfs_changes: Vec<SysfsChange>,
    pub kernel_params_added: Vec<String>,
    #[serde(default)]
    pub kernel_param_backups: Vec<kernel_params::KernelParamBackup>,
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
    fn has_recorded_changes(&self) -> bool {
        !self.sysfs_changes.is_empty()
            || !self.kernel_params_added.is_empty()
            || !self.services_disabled.is_empty()
            || !self.systemd_units_created.is_empty()
            || !self.modprobe_files_created.is_empty()
            || !self.acpi_wakeup_toggled.is_empty()
    }

    pub(crate) fn file_path() -> PathBuf {
        state_file_path()
    }

    #[cfg(test)]
    pub(crate) fn set_file_path_override_for_tests(path: Option<PathBuf>) {
        *STATE_FILE_OVERRIDE
            .lock()
            .expect("state file override lock poisoned") = path;
    }

    pub fn load() -> Result<Option<Self>> {
        let path = state_file_path();
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
        std::fs::create_dir_all(state_dir_path())
            .map_err(|e| Error::State(format!("failed to create state dir: {}", e)))?;
        let data = serde_json::to_string_pretty(self)
            .map_err(|e| Error::State(format!("failed to serialize state: {}", e)))?;
        std::fs::write(state_file_path(), data)
            .map_err(|e| Error::State(format!("failed to write state file: {}", e)))?;
        Ok(())
    }

    pub fn remove_file() -> Result<()> {
        let path = state_file_path();
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
    if hw.cpu.epp.as_deref() != Some("balance_power") && hw.cpu.epp.as_deref() != Some("power")
        && let Ok(cpus) = sysfs.list_dir("sys/devices/system/cpu")
    {
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
    if hw.kernel_param_value("acpi.ec_no_wakeup").as_deref() != Some("1") {
        plan.kernel_params.push("acpi.ec_no_wakeup=1".to_string());
    }
    if hw.kernel_param_value("rtc_cmos.use_acpi_alarm").as_deref() != Some("1") {
        plan.kernel_params
            .push("rtc_cmos.use_acpi_alarm=1".to_string());
    }
    if hw.gpu.is_amd()
        && hw
            .kernel_param_value("amdgpu.abmlevel")
            .and_then(|v| v.parse::<u32>().ok())
            .is_none_or(|v| v < 3)
    {
        plan.kernel_params.push("amdgpu.abmlevel=3".to_string());
    }

    // Services to disable
    for svc in &["tlp.service", "power-profiles-daemon.service"] {
        if is_service_active_or_enabled(svc) {
            plan.services_to_disable.push(svc.to_string());
        }
    }

    // ACPI wakeup sources to disable:
    // only USB/XHCI controllers on PCI, excluding the essential XHC0 controller.
    for source in &hw.platform.acpi_wakeup_sources {
        if should_disable_acpi_wakeup_source(source, hw) {
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

trait ApplyOps {
    fn write_sysfs(&mut self, path: &str, value: &str) -> Result<()>;
    fn toggle_acpi_wakeup(&mut self, device: &str) -> Result<()>;
    fn add_kernel_params(&mut self, params: &[String]) -> Result<Vec<kernel_params::KernelParamBackup>>;
    fn disable_service(&mut self, service: &str) -> Result<()>;
    fn generate_service(&mut self, hw: &HardwareInfo, plan: &ApplyPlan) -> Result<PathBuf>;
    fn enable_systemd_service(&mut self) -> Result<()>;
    fn save_state(&mut self, state: &ApplyState) -> Result<()>;
}

struct RealApplyOps;

impl ApplyOps for RealApplyOps {
    fn write_sysfs(&mut self, path: &str, value: &str) -> Result<()> {
        sysfs_writer::write_sysfs(path, value)
    }

    fn toggle_acpi_wakeup(&mut self, device: &str) -> Result<()> {
        sysfs_writer::toggle_acpi_wakeup(device)
    }

    fn add_kernel_params(&mut self, params: &[String]) -> Result<Vec<kernel_params::KernelParamBackup>> {
        kernel_params::add_kernel_params(params)
    }

    fn disable_service(&mut self, service: &str) -> Result<()> {
        services::disable_service(service)
    }

    fn generate_service(&mut self, hw: &HardwareInfo, plan: &ApplyPlan) -> Result<PathBuf> {
        systemd::generate_service(hw, plan)
    }

    fn enable_systemd_service(&mut self) -> Result<()> {
        systemd::enable_service()
    }

    fn save_state(&mut self, state: &ApplyState) -> Result<()> {
        state.save()
    }
}

fn persist_state_checkpoint(
    ops: &mut impl ApplyOps,
    state: &ApplyState,
    dry_run: bool,
) -> Result<()> {
    if !dry_run && state.has_recorded_changes() {
        ops.save_state(state)?;
    }
    Ok(())
}

fn execute_plan_with_ops(
    plan: &ApplyPlan,
    hw: &HardwareInfo,
    dry_run: bool,
    ops: &mut impl ApplyOps,
) -> Result<ApplyState> {
    let mut state = ApplyState {
        timestamp: chrono::Utc::now().to_rfc3339(),
        ..Default::default()
    };

    let sysfs = SysfsRoot::system();

    // Apply runtime sysfs writes.
    for write in &plan.sysfs_writes {
        let relative = write.path.strip_prefix('/').unwrap_or(&write.path);
        let original = sysfs
            .read_optional(relative)
            .unwrap_or(None)
            .unwrap_or_default();

        if dry_run {
            println!(
                "  [dry-run] {} -> {} (was: {})",
                write.path, write.value, original
            );
        } else {
            ops.write_sysfs(&write.path, &write.value)?;
            state.sysfs_changes.push(SysfsChange {
                path: write.path.clone(),
                original_value: original,
                new_value: write.value.clone(),
            });
        }
    }

    // ACPI wakeup toggling.
    for device in &plan.acpi_wakeup_disable {
        if dry_run {
            println!("  [dry-run] Disable ACPI wakeup: {}", device);
        } else if is_wakeup_enabled(device, &sysfs) {
            // /proc/acpi/wakeup is a toggle - only flip currently enabled sources.
            ops.toggle_acpi_wakeup(device)?;
            state.acpi_wakeup_toggled.push(device.clone());
        }
    }
    persist_state_checkpoint(ops, &state, dry_run)?;

    // Kernel params.
    if !plan.kernel_params.is_empty() {
        if dry_run {
            println!(
                "  [dry-run] Add kernel params: {}",
                plan.kernel_params.join(" ")
            );
        } else {
            let backups = ops.add_kernel_params(&plan.kernel_params)?;
            let previous_state = ApplyState::load().unwrap_or(None);
            merge_kernel_param_state(
                &mut state,
                &plan.kernel_params,
                backups,
                previous_state.as_ref(),
            );
        }
    }
    persist_state_checkpoint(ops, &state, dry_run)?;

    // Service management.
    for svc in &plan.services_to_disable {
        if dry_run {
            println!("  [dry-run] Disable service: {}", svc);
        } else {
            ops.disable_service(svc)?;
            state.services_disabled.push(svc.clone());
        }
    }
    persist_state_checkpoint(ops, &state, dry_run)?;

    // Generate/enable persistence service.
    if plan.systemd_service && !plan.sysfs_writes.is_empty() {
        if dry_run {
            println!("  [dry-run] Generate bop-powersave.service");
        } else {
            let unit_path = ops.generate_service(hw, plan)?;
            state
                .systemd_units_created
                .push(unit_path.to_string_lossy().into_owned());
            // Persist immediately so a later enable failure can still be reverted.
            persist_state_checkpoint(ops, &state, dry_run)?;
            ops.enable_systemd_service()?;
        }
    }

    Ok(state)
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

    let mut ops = RealApplyOps;
    execute_plan_with_ops(plan, hw, dry_run, &mut ops)
}

fn merge_kernel_param_state(
    state: &mut ApplyState,
    planned_params: &[String],
    new_backups: Vec<kernel_params::KernelParamBackup>,
    previous_state: Option<&ApplyState>,
) {
    state.kernel_params_added = planned_params.to_vec();

    // Start from previous backups, then overlay new ones keyed by path.
    // New backups replace previous entries for the same file, but previous
    // entries for files not touched this run are preserved.
    let mut merged: std::collections::HashMap<&str, &kernel_params::KernelParamBackup> =
        std::collections::HashMap::new();

    if let Some(prev) = previous_state {
        for backup in &prev.kernel_param_backups {
            merged.insert(&backup.path, backup);
        }
    }
    for backup in &new_backups {
        merged.insert(&backup.path, backup);
    }

    state.kernel_param_backups = merged.into_values().cloned().collect();
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

fn should_disable_acpi_wakeup_source(
    source: &crate::detect::platform::AcpiWakeupSource,
    hw: &HardwareInfo,
) -> bool {
    if !source.enabled || source.device == "XHC0" {
        return false;
    }

    let Some(pci_address) = source
        .sysfs_node
        .as_deref()
        .and_then(|node| node.strip_prefix("pci:"))
    else {
        return false;
    };

    if source.device.starts_with("XHC") {
        return true;
    }

    hw.pci
        .devices
        .iter()
        .find(|device| device.address == pci_address)
        .is_some_and(is_usb_pci_device)
}

fn is_usb_pci_device(device: &crate::detect::pci::PciDevice) -> bool {
    let class_is_usb_host_controller = device.class.as_deref().is_some_and(|class| {
        let class = class.trim_start_matches("0x").to_ascii_lowercase();
        let class = if class.len() >= 6 {
            &class[..6]
        } else {
            class.as_str()
        };

        matches!(class, "0c0300" | "0c0310" | "0c0320" | "0c0330")
    });
    if class_is_usb_host_controller {
        return true;
    }

    device.driver.as_deref().is_some_and(|driver| {
        let driver = driver.to_ascii_lowercase();
        driver.contains("xhci")
            || driver.contains("ehci")
            || driver.contains("ohci")
            || driver.contains("uhci")
    })
}

pub fn print_plan(plan: &ApplyPlan) {
    use colored::Colorize;

    println!("{}", "Apply Plan".bold().underline());
    println!();

    if !plan.sysfs_writes.is_empty() {
        println!("  {} Runtime sysfs changes:", ">>".cyan());
        for write in &plan.sysfs_writes {
            println!(
                "     {} {}",
                write.description.dimmed(),
                write.path.dimmed()
            );
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use tempfile::TempDir;

    struct TestApplyOps {
        state_path: PathBuf,
        fail_add_kernel_params: bool,
        fail_generate_service: bool,
        fail_enable_service: bool,
        checkpoint_count: usize,
    }

    impl TestApplyOps {
        fn new(state_path: PathBuf) -> Self {
            Self {
                state_path,
                fail_add_kernel_params: false,
                fail_generate_service: false,
                fail_enable_service: false,
                checkpoint_count: 0,
            }
        }
    }

    impl ApplyOps for TestApplyOps {
        fn write_sysfs(&mut self, path: &str, value: &str) -> Result<()> {
            std::fs::write(path, value).map_err(|source| Error::SysfsWrite {
                path: PathBuf::from(path),
                source,
            })
        }

        fn toggle_acpi_wakeup(&mut self, _device: &str) -> Result<()> {
            Ok(())
        }

        fn add_kernel_params(&mut self, _params: &[String]) -> Result<Vec<kernel_params::KernelParamBackup>> {
            if self.fail_add_kernel_params {
                return Err(Error::Other("injected kernel params failure".to_string()));
            }
            Ok(Vec::new())
        }

        fn disable_service(&mut self, _service: &str) -> Result<()> {
            Ok(())
        }

        fn generate_service(&mut self, _hw: &HardwareInfo, _plan: &ApplyPlan) -> Result<PathBuf> {
            if self.fail_generate_service {
                return Err(Error::Other(
                    "injected systemd generation failure".to_string(),
                ));
            }
            Ok(PathBuf::from("/etc/systemd/system/bop-powersave.service"))
        }

        fn enable_systemd_service(&mut self) -> Result<()> {
            if self.fail_enable_service {
                return Err(Error::Other("injected systemd enable failure".to_string()));
            }
            Ok(())
        }

        fn save_state(&mut self, state: &ApplyState) -> Result<()> {
            self.checkpoint_count += 1;
            if let Some(parent) = self.state_path.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| Error::State(format!("failed to create state dir: {}", e)))?;
            }
            let data = serde_json::to_string_pretty(state)
                .map_err(|e| Error::State(format!("failed to serialize state: {}", e)))?;
            std::fs::write(&self.state_path, data)
                .map_err(|e| Error::State(format!("failed to write state file: {}", e)))?;
            Ok(())
        }
    }

    fn minimal_hw() -> HardwareInfo {
        let tmp = TempDir::new().unwrap();
        HardwareInfo::detect(&SysfsRoot::new(tmp.path()))
    }

    fn read_state(path: &Path) -> ApplyState {
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    fn basic_plan(sysfs_path: &Path) -> ApplyPlan {
        ApplyPlan {
            sysfs_writes: vec![PlannedSysfsWrite {
                path: sysfs_path.to_string_lossy().into_owned(),
                value: "new".to_string(),
                description: "test write".to_string(),
            }],
            kernel_params: Vec::new(),
            services_to_disable: Vec::new(),
            acpi_wakeup_disable: Vec::new(),
            systemd_service: true,
            modprobe_configs: Vec::new(),
        }
    }

    #[test]
    fn test_execute_plan_persists_sysfs_state_before_systemd_generation_failure() {
        let tmp = TempDir::new().unwrap();
        let state_path = tmp.path().join("state.json");
        let sysfs_path = tmp.path().join("sysfs-value");
        std::fs::write(&sysfs_path, "old").unwrap();

        let hw = minimal_hw();
        let plan = basic_plan(&sysfs_path);
        let mut ops = TestApplyOps::new(state_path.clone());
        ops.fail_generate_service = true;

        let result = execute_plan_with_ops(&plan, &hw, false, &mut ops);
        assert!(result.is_err());

        let persisted = read_state(&state_path);
        assert_eq!(persisted.sysfs_changes.len(), 1);
        assert_eq!(persisted.sysfs_changes[0].path, plan.sysfs_writes[0].path);
        assert_eq!(persisted.sysfs_changes[0].original_value, "old");
        assert_eq!(persisted.sysfs_changes[0].new_value, "new");
        assert!(persisted.systemd_units_created.is_empty());
    }

    #[test]
    fn test_execute_plan_persists_created_unit_before_systemd_enable_failure() {
        let tmp = TempDir::new().unwrap();
        let state_path = tmp.path().join("state.json");
        let sysfs_path = tmp.path().join("sysfs-value");
        std::fs::write(&sysfs_path, "old").unwrap();

        let hw = minimal_hw();
        let mut plan = basic_plan(&sysfs_path);
        plan.kernel_params = vec!["acpi.ec_no_wakeup=1".to_string()];
        plan.services_to_disable = vec!["dummy.service".to_string()];

        let mut ops = TestApplyOps::new(state_path.clone());
        ops.fail_enable_service = true;

        let result = execute_plan_with_ops(&plan, &hw, false, &mut ops);
        assert!(result.is_err());

        let persisted = read_state(&state_path);
        assert_eq!(persisted.sysfs_changes.len(), 1);
        assert_eq!(persisted.kernel_params_added, plan.kernel_params);
        assert_eq!(persisted.services_disabled, plan.services_to_disable);
        assert_eq!(
            persisted.systemd_units_created,
            vec!["/etc/systemd/system/bop-powersave.service".to_string()]
        );
        assert_eq!(ops.checkpoint_count, 4);
    }

    #[test]
    fn test_execute_plan_preserves_existing_state_when_failure_happens_before_first_change() {
        let tmp = TempDir::new().unwrap();
        let state_path = tmp.path().join("state.json");

        let previous_state = ApplyState {
            timestamp: "2026-01-01T00:00:00Z".to_string(),
            sysfs_changes: vec![SysfsChange {
                path: "/sys/devices/system/cpu/cpu0/cpufreq/energy_performance_preference"
                    .to_string(),
                original_value: "performance".to_string(),
                new_value: "balance_power".to_string(),
            }],
            ..Default::default()
        };
        std::fs::write(
            &state_path,
            serde_json::to_string_pretty(&previous_state).unwrap(),
        )
        .unwrap();

        let hw = minimal_hw();
        let plan = ApplyPlan {
            sysfs_writes: Vec::new(),
            kernel_params: vec!["acpi.ec_no_wakeup=1".to_string()],
            services_to_disable: Vec::new(),
            acpi_wakeup_disable: Vec::new(),
            systemd_service: false,
            modprobe_configs: Vec::new(),
        };

        let mut ops = TestApplyOps::new(state_path.clone());
        ops.fail_add_kernel_params = true;

        let result = execute_plan_with_ops(&plan, &hw, false, &mut ops);
        assert!(result.is_err());
        assert_eq!(ops.checkpoint_count, 0);

        let persisted = read_state(&state_path);
        assert_eq!(persisted.timestamp, previous_state.timestamp);
        assert_eq!(persisted.sysfs_changes.len(), 1);
        assert_eq!(
            persisted.sysfs_changes[0].path,
            previous_state.sysfs_changes[0].path
        );
        assert_eq!(
            persisted.sysfs_changes[0].original_value,
            previous_state.sysfs_changes[0].original_value
        );
        assert_eq!(
            persisted.sysfs_changes[0].new_value,
            previous_state.sysfs_changes[0].new_value
        );
        assert!(persisted.kernel_params_added.is_empty());
    }

    #[test]
    fn test_merge_kernel_param_state_uses_new_backups_when_present() {
        let mut state = ApplyState::default();
        let planned = vec!["acpi.ec_no_wakeup=1".to_string()];
        let backups = vec![kernel_params::KernelParamBackup {
            path: "/boot/loader/entries/test.conf".to_string(),
            original_content: "options quiet acpi.ec_no_wakeup=0\n".to_string(),
        }];

        merge_kernel_param_state(&mut state, &planned, backups.clone(), None);

        assert_eq!(state.kernel_params_added, planned);
        assert_eq!(state.kernel_param_backups, backups);
    }

    #[test]
    fn test_merge_kernel_param_state_preserves_previous_when_no_new_backups() {
        let mut state = ApplyState::default();
        let planned = vec!["acpi.ec_no_wakeup=1".to_string()];
        let previous = ApplyState {
            kernel_params_added: vec!["acpi.ec_no_wakeup=1".to_string()],
            kernel_param_backups: vec![kernel_params::KernelParamBackup {
                path: "/boot/loader/entries/test.conf".to_string(),
                original_content: "options quiet acpi.ec_no_wakeup=0\n".to_string(),
            }],
            ..Default::default()
        };

        merge_kernel_param_state(&mut state, &planned, Vec::new(), Some(&previous));

        assert_eq!(state.kernel_params_added, previous.kernel_params_added);
        assert_eq!(state.kernel_param_backups, previous.kernel_param_backups);
    }

    #[test]
    fn test_merge_kernel_param_state_empty_when_no_backups_and_no_previous() {
        let mut state = ApplyState::default();
        let planned = vec!["acpi.ec_no_wakeup=1".to_string()];

        merge_kernel_param_state(&mut state, &planned, Vec::new(), None);

        assert_eq!(state.kernel_params_added, planned);
        assert!(state.kernel_param_backups.is_empty());
    }

    #[test]
    fn test_merge_kernel_param_state_merges_new_and_previous_backups() {
        let mut state = ApplyState::default();
        let planned = vec![
            "acpi.ec_no_wakeup=1".to_string(),
            "rtc_cmos.use_acpi_alarm=1".to_string(),
        ];
        let prev_backup = kernel_params::KernelParamBackup {
            path: "/boot/loader/entries/old.conf".to_string(),
            original_content: "options quiet acpi.ec_no_wakeup=0\n".to_string(),
        };
        let new_backup = kernel_params::KernelParamBackup {
            path: "/boot/loader/entries/new.conf".to_string(),
            original_content: "options quiet rtc_cmos.use_acpi_alarm=0\n".to_string(),
        };
        let previous = ApplyState {
            kernel_param_backups: vec![prev_backup.clone()],
            ..Default::default()
        };

        merge_kernel_param_state(
            &mut state,
            &planned,
            vec![new_backup.clone()],
            Some(&previous),
        );

        assert_eq!(state.kernel_params_added, planned);
        assert_eq!(state.kernel_param_backups.len(), 2);
        assert!(state.kernel_param_backups.contains(&prev_backup));
        assert!(state.kernel_param_backups.contains(&new_backup));
    }

    #[test]
    fn test_merge_kernel_param_state_new_backup_overwrites_same_path() {
        let mut state = ApplyState::default();
        let planned = vec!["acpi.ec_no_wakeup=1".to_string()];
        let prev_backup = kernel_params::KernelParamBackup {
            path: "/boot/loader/entries/linux.conf".to_string(),
            original_content: "options quiet\n".to_string(),
        };
        let new_backup = kernel_params::KernelParamBackup {
            path: "/boot/loader/entries/linux.conf".to_string(),
            original_content: "options quiet acpi.ec_no_wakeup=0\n".to_string(),
        };
        let previous = ApplyState {
            kernel_param_backups: vec![prev_backup],
            ..Default::default()
        };

        merge_kernel_param_state(
            &mut state,
            &planned,
            vec![new_backup.clone()],
            Some(&previous),
        );

        assert_eq!(state.kernel_param_backups.len(), 1);
        assert_eq!(state.kernel_param_backups[0], new_backup);
    }
}
