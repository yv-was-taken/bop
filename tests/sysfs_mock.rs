use bop::apply;
use bop::audit;
use bop::detect::HardwareInfo;
use bop::profile;
use bop::sysfs::SysfsRoot;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

/// Create a mock sysfs tree that simulates a Framework 16 AMD system
/// with suboptimal power settings (the "before" state).
fn create_framework16_fixture(root: &Path) {
    // DMI
    let dmi = root.join("sys/class/dmi/id");
    fs::create_dir_all(&dmi).unwrap();
    fs::write(dmi.join("board_vendor"), "Framework\n").unwrap();
    fs::write(dmi.join("board_name"), "FRANMDCP16\n").unwrap();
    fs::write(
        dmi.join("product_name"),
        "Laptop 16 (AMD Ryzen 7040 Series)\n",
    )
    .unwrap();
    fs::write(dmi.join("product_family"), "Framework Laptop\n").unwrap();
    fs::write(dmi.join("bios_version"), "03.05\n").unwrap();

    // CPU
    let cpu_base = root.join("sys/devices/system/cpu");
    fs::create_dir_all(cpu_base.join("cpufreq")).unwrap();
    fs::write(cpu_base.join("cpufreq/boost"), "1\n").unwrap();

    let cpuinfo = "processor\t: 0\nvendor_id\t: AuthenticAMD\ncpu family\t: 25\nmodel\t\t: 116\nmodel name\t: AMD Ryzen 9 7940HS w/ Radeon 780M Graphics\n\n";
    fs::create_dir_all(root.join("proc")).unwrap();
    fs::write(root.join("proc/cpuinfo"), cpuinfo).unwrap();

    // Create CPU entries with suboptimal EPP
    for i in 0..16 {
        let cpu_dir = cpu_base.join(format!("cpu{}/cpufreq", i));
        fs::create_dir_all(&cpu_dir).unwrap();
        fs::write(cpu_dir.join("scaling_driver"), "amd-pstate-epp\n").unwrap();
        fs::write(cpu_dir.join("scaling_governor"), "powersave\n").unwrap();
        fs::write(
            cpu_dir.join("energy_performance_preference"),
            "balance_performance\n",
        )
        .unwrap();
        fs::write(
            cpu_dir.join("energy_performance_available_preferences"),
            "default performance balance_performance balance_power power\n",
        )
        .unwrap();
    }

    // GPU (AMD iGPU)
    let drm = root.join("sys/class/drm/card0/device");
    fs::create_dir_all(&drm).unwrap();
    fs::write(drm.join("vendor"), "0x1002\n").unwrap();
    fs::write(drm.join("device"), "0x15bf\n").unwrap();
    fs::write(drm.join("power_dpm_force_performance_level"), "auto\n").unwrap();

    // ABM module parameter
    let amdgpu_params = root.join("sys/module/amdgpu/parameters");
    fs::create_dir_all(&amdgpu_params).unwrap();
    fs::write(amdgpu_params.join("abmlevel"), "0\n").unwrap();

    // Kernel cmdline (missing power params)
    fs::write(
        root.join("proc/cmdline"),
        "initrd=\\initramfs-linux.img root=UUID=abc123 rw\n",
    )
    .unwrap();

    // Platform profile - set to performance (suboptimal)
    let acpi = root.join("sys/firmware/acpi");
    fs::create_dir_all(&acpi).unwrap();
    fs::write(acpi.join("platform_profile"), "performance\n").unwrap();
    fs::write(
        acpi.join("platform_profile_choices"),
        "low-power balanced performance\n",
    )
    .unwrap();

    // Sleep state
    let power = root.join("sys/power");
    fs::create_dir_all(&power).unwrap();
    fs::write(power.join("state"), "mem disk\n").unwrap();
    fs::write(power.join("mem_sleep"), "[s2idle] deep\n").unwrap();

    // Battery
    let bat = root.join("sys/class/power_supply/BAT0");
    fs::create_dir_all(&bat).unwrap();
    fs::write(bat.join("type"), "Battery\n").unwrap();
    fs::write(bat.join("present"), "1\n").unwrap();
    fs::write(bat.join("status"), "Discharging\n").unwrap();
    fs::write(bat.join("capacity"), "75\n").unwrap();
    fs::write(bat.join("energy_now"), "41000000\n").unwrap();
    fs::write(bat.join("energy_full"), "54600000\n").unwrap();
    fs::write(bat.join("energy_full_design"), "61000000\n").unwrap();
    fs::write(bat.join("power_now"), "7500000\n").unwrap();
    fs::write(bat.join("voltage_now"), "15800000\n").unwrap();
    fs::write(bat.join("cycle_count"), "120\n").unwrap();

    // PCI devices (a few with suboptimal runtime PM)
    let pci_base = root.join("sys/bus/pci/devices");
    let aspm = root.join("sys/module/pcie_aspm/parameters");
    fs::create_dir_all(&aspm).unwrap();
    fs::write(
        aspm.join("policy"),
        "default [default] performance powersave powersupersave\n",
    )
    .unwrap();

    for (addr, control, class) in &[
        ("0000:00:00.0", "on", "0x060000"),
        ("0000:00:02.2", "on", "0x060400"),
        ("0000:c1:00.3", "auto", "0x0c0330"),
        ("0000:c1:00.4", "on", "0x0c0330"),
        ("0000:c3:00.3", "auto", "0x0c0330"),
    ] {
        let dev = pci_base.join(addr);
        fs::create_dir_all(dev.join("power")).unwrap();
        fs::write(dev.join("power/control"), format!("{}\n", control)).unwrap();
        fs::write(dev.join("power/runtime_status"), "active\n").unwrap();
        fs::write(dev.join("vendor"), "0x1022\n").unwrap();
        fs::write(dev.join("device"), "0x14e8\n").unwrap();
        fs::write(dev.join("class"), format!("{}\n", class)).unwrap();
    }

    // Network
    let net = root.join("sys/class/net/wlan0");
    fs::create_dir_all(net.join("wireless")).unwrap();
    fs::create_dir_all(net.join("device")).unwrap();

    // Audio
    let hda = root.join("sys/module/snd_hda_intel/parameters");
    fs::create_dir_all(&hda).unwrap();
    fs::write(hda.join("power_save"), "1\n").unwrap();
    fs::write(hda.join("power_save_controller"), "Y\n").unwrap();

    // ACPI wakeup (simulated - multiple unnecessary sources enabled)
    fs::create_dir_all(root.join("proc/acpi")).unwrap();
    let wakeup_content = "\
XHC0\tS3\t*enabled\tpci:0000:c1:00.3
XHC1\tS3\t*enabled\tpci:0000:c1:00.4
XHC3\tS3\t*enabled\tpci:0000:c3:00.3
GPP6\tS4\t*enabled\tpci:0000:00:02.2
NHI0\tS4\t*enabled\tpci:0000:c3:00.5
LID0\tS4\t*enabled\tplatform:PNP0C0D:00
PBTN\tS4\t*enabled\tplatform:PNP0C0C:00
SLPB\tS4\t*enabled\tplatform:PNP0C0E:00
";
    fs::write(root.join("proc/acpi/wakeup"), wakeup_content).unwrap();
}

#[test]
fn test_framework16_detection() {
    let tmp = TempDir::new().unwrap();
    create_framework16_fixture(tmp.path());

    let sysfs = SysfsRoot::new(tmp.path());
    let hw = HardwareInfo::detect(&sysfs);

    assert!(hw.dmi.is_framework());
    assert!(hw.dmi.is_framework_16());
    assert!(hw.cpu.is_amd());
    assert!(hw.cpu.is_amd_pstate());
    assert_eq!(hw.cpu.family, Some(25));
    assert_eq!(hw.cpu.model, Some(116));
    assert_eq!(hw.cpu.online_cpus, 16);
    assert_eq!(hw.cpu.epp.as_deref(), Some("balance_performance"));
    assert_eq!(hw.platform.platform_profile.as_deref(), Some("performance"));
    assert!(hw.battery.present);
    assert!(hw.battery.is_discharging());
    assert!(hw.gpu.is_amd());
    assert_eq!(hw.pci.aspm_policy.as_deref(), Some("default"));
}

#[test]
fn test_framework16_profile_matches() {
    let tmp = TempDir::new().unwrap();
    create_framework16_fixture(tmp.path());

    let sysfs = SysfsRoot::new(tmp.path());
    let hw = HardwareInfo::detect(&sysfs);

    let profile = profile::detect_profile(&hw);
    assert!(profile.is_some());
    assert_eq!(
        profile.unwrap().name(),
        "Framework Laptop 16 (AMD Ryzen 7040 Series)"
    );
}

#[test]
fn test_kernel_param_detection() {
    let tmp = TempDir::new().unwrap();
    create_framework16_fixture(tmp.path());

    let sysfs = SysfsRoot::new(tmp.path());
    let hw = HardwareInfo::detect(&sysfs);

    assert!(!hw.has_kernel_param("acpi.ec_no_wakeup"));
    assert!(!hw.has_kernel_param("rtc_cmos.use_acpi_alarm"));
    assert!(!hw.has_kernel_param("amdgpu.abmlevel"));
    assert!(hw.has_kernel_param("root"));
    assert_eq!(
        hw.kernel_param_value("root"),
        Some("UUID=abc123".to_string())
    );
}

#[test]
fn test_build_plan_updates_wrong_kernel_param_values() {
    let tmp = TempDir::new().unwrap();
    create_framework16_fixture(tmp.path());

    fs::write(
        tmp.path().join("proc/cmdline"),
        "initrd=\\initramfs-linux.img root=UUID=abc123 rw acpi.ec_no_wakeup=0 rtc_cmos.use_acpi_alarm=0 amdgpu.abmlevel=1\n",
    )
    .unwrap();

    let sysfs = SysfsRoot::new(tmp.path());
    let hw = HardwareInfo::detect(&sysfs);
    let plan = apply::build_plan(&hw, &sysfs);

    assert!(
        plan.kernel_params
            .contains(&"acpi.ec_no_wakeup=1".to_string())
    );
    assert!(
        plan.kernel_params
            .contains(&"rtc_cmos.use_acpi_alarm=1".to_string())
    );
    assert!(
        plan.kernel_params
            .contains(&"amdgpu.abmlevel=3".to_string())
    );
}

#[test]
fn test_build_plan_skips_abmlevel_at_or_above_3() {
    let tmp = TempDir::new().unwrap();
    create_framework16_fixture(tmp.path());

    // abmlevel=3 is at the threshold — apply should leave it alone (matching audit)
    fs::write(
        tmp.path().join("proc/cmdline"),
        "root=UUID=abc rw acpi.ec_no_wakeup=1 rtc_cmos.use_acpi_alarm=1 amdgpu.abmlevel=3\n",
    )
    .unwrap();

    let sysfs = SysfsRoot::new(tmp.path());
    let hw = HardwareInfo::detect(&sysfs);
    let plan = apply::build_plan(&hw, &sysfs);

    assert!(
        !plan
            .kernel_params
            .iter()
            .any(|p| p.starts_with("amdgpu.abmlevel")),
        "apply should not touch abmlevel when it is already >= 3"
    );
}

#[test]
fn test_battery_info() {
    let tmp = TempDir::new().unwrap();
    create_framework16_fixture(tmp.path());

    let sysfs = SysfsRoot::new(tmp.path());
    let hw = HardwareInfo::detect(&sysfs);

    assert_eq!(hw.battery.capacity_percent, Some(75));
    assert!((hw.battery.power_watts().unwrap() - 7.5).abs() < 0.01);
    assert!((hw.battery.energy_wh().unwrap() - 41.0).abs() < 0.01);
    assert!((hw.battery.usable_capacity_wh().unwrap() - 54.6).abs() < 0.01);

    let health = hw.battery.health_percent.unwrap();
    assert!((health - 89.5).abs() < 0.5);
}

#[test]
fn test_audit_finds_issues() {
    let tmp = TempDir::new().unwrap();
    create_framework16_fixture(tmp.path());

    let sysfs = SysfsRoot::new(tmp.path());
    let hw = HardwareInfo::detect(&sysfs);

    // Run individual audit checks that don't require external commands
    let kernel_findings = audit::kernel_params::check(&hw);
    assert!(
        kernel_findings.len() >= 3,
        "Expected at least 3 kernel param findings (ec_no_wakeup, rtc_cmos, abmlevel), got {}",
        kernel_findings.len()
    );

    let cpu_findings = audit::cpu_power::check(&hw);
    assert!(
        cpu_findings.len() >= 2,
        "Expected at least 2 CPU findings (EPP + platform profile), got {}",
        cpu_findings.len()
    );

    let pci_findings = audit::pci_power::check(&hw);
    assert!(
        !pci_findings.is_empty(),
        "Expected PCI findings (ASPM + runtime PM)"
    );

    // Check score is low (many issues)
    let mut all_findings = Vec::new();
    all_findings.extend(kernel_findings);
    all_findings.extend(cpu_findings);
    all_findings.extend(pci_findings);

    let score = audit::calculate_score(&all_findings);
    assert!(
        score < 70,
        "Expected low score with many issues, got {}",
        score
    );
}

#[test]
fn test_score_calculation() {
    // No findings = perfect score
    assert_eq!(audit::calculate_score(&[]), 100);

    // Single high-weight finding
    let findings = vec![audit::Finding::new(audit::Severity::High, "Test", "test").weight(10)];
    let score = audit::calculate_score(&findings);
    assert_eq!(score, 0); // 10/10 penalty = 100% penalty

    // Mixed findings
    let findings = vec![
        audit::Finding::new(audit::Severity::High, "Test", "test").weight(8),
        audit::Finding::new(audit::Severity::Low, "Test", "test").weight(2),
    ];
    let score = audit::calculate_score(&findings);
    assert_eq!(score, 50); // 10/20 = 50% penalty = score 50
}

#[test]
fn test_apply_plan_only_disables_usb_wake_sources() {
    let tmp = TempDir::new().unwrap();
    create_framework16_fixture(tmp.path());

    let sysfs = SysfsRoot::new(tmp.path());
    let hw = HardwareInfo::detect(&sysfs);
    let plan = apply::build_plan(&hw, &sysfs);

    assert!(plan.acpi_wakeup_disable.contains(&"XHC1".to_string()));
    assert!(plan.acpi_wakeup_disable.contains(&"XHC3".to_string()));
    assert!(!plan.acpi_wakeup_disable.contains(&"XHC0".to_string()));
    assert!(!plan.acpi_wakeup_disable.contains(&"GPP6".to_string()));
    assert!(!plan.acpi_wakeup_disable.contains(&"NHI0".to_string()));
    assert!(!plan.acpi_wakeup_disable.contains(&"LID0".to_string()));
    assert!(!plan.acpi_wakeup_disable.contains(&"PBTN".to_string()));
    assert!(!plan.acpi_wakeup_disable.contains(&"SLPB".to_string()));
}

#[test]
fn test_apply_plan_does_not_disable_usb4_nhi_wake_source() {
    let tmp = TempDir::new().unwrap();
    create_framework16_fixture(tmp.path());

    let nhi = tmp.path().join("sys/bus/pci/devices/0000:c3:00.5");
    fs::create_dir_all(nhi.join("power")).unwrap();
    fs::write(nhi.join("power/control"), "auto\n").unwrap();
    fs::write(nhi.join("power/runtime_status"), "active\n").unwrap();
    fs::write(nhi.join("vendor"), "0x8086\n").unwrap();
    fs::write(nhi.join("device"), "0x0b26\n").unwrap();
    fs::write(nhi.join("class"), "0x0c0340\n").unwrap();

    let sysfs = SysfsRoot::new(tmp.path());
    let hw = HardwareInfo::detect(&sysfs);
    let plan = apply::build_plan(&hw, &sysfs);

    assert!(plan.acpi_wakeup_disable.contains(&"XHC1".to_string()));
    assert!(!plan.acpi_wakeup_disable.contains(&"NHI0".to_string()));
}
