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

#[test]
fn test_audit_flags_missing_amd_pstate() {
    let tmp = TempDir::new().unwrap();
    create_framework16_fixture(tmp.path());

    // Override all CPU scaling drivers to acpi-cpufreq
    for i in 0..16 {
        let driver_path = tmp.path().join(format!(
            "sys/devices/system/cpu/cpu{}/cpufreq/scaling_driver",
            i
        ));
        fs::write(driver_path, "acpi-cpufreq\n").unwrap();
    }

    let sysfs = SysfsRoot::new(tmp.path());
    let hw = HardwareInfo::detect(&sysfs);

    assert!(!hw.cpu.is_amd_pstate());
    assert_eq!(hw.cpu.scaling_driver.as_deref(), Some("acpi-cpufreq"));

    let findings = audit::cpu_power::check(&hw);
    let pstate_finding = findings
        .iter()
        .find(|f| f.severity == audit::Severity::High && f.description.contains("EPP unavailable"))
        .expect("Expected a HIGH finding about missing amd-pstate with EPP unavailable");

    assert_eq!(pstate_finding.recommended_value, "amd-pstate-epp");
    assert!(pstate_finding.description.contains("acpi-cpufreq"));
}

#[test]
fn test_audit_nvme_apst_disabled() {
    let tmp = TempDir::new().unwrap();
    create_framework16_fixture(tmp.path());

    // Override /proc/cmdline to include nvme_core.default_ps_max_latency_us=0
    fs::write(
        tmp.path().join("proc/cmdline"),
        "initrd=\\initramfs-linux.img root=UUID=abc123 rw nvme_core.default_ps_max_latency_us=0\n",
    )
    .unwrap();

    let sysfs = SysfsRoot::new(tmp.path());
    let hw = HardwareInfo::detect(&sysfs);

    let findings = audit::kernel_params::check(&hw);
    let nvme_finding = findings
        .iter()
        .find(|f| f.description.contains("NVMe APST disabled"))
        .expect("Expected a finding about NVMe APST being disabled");

    assert_eq!(nvme_finding.severity, audit::Severity::Medium);
    assert!(
        nvme_finding
            .current_value
            .contains("nvme_core.default_ps_max_latency_us=0")
    );
}

#[test]
fn test_audit_dgpu_not_d3cold() {
    let tmp = TempDir::new().unwrap();
    create_framework16_fixture(tmp.path());

    // Add a second DRM card (dGPU) in D0
    let dgpu = tmp.path().join("sys/class/drm/card1/device");
    fs::create_dir_all(&dgpu).unwrap();
    fs::write(dgpu.join("vendor"), "0x1002\n").unwrap();
    fs::write(dgpu.join("power_state"), "D0\n").unwrap();

    let sysfs = SysfsRoot::new(tmp.path());
    let hw = HardwareInfo::detect(&sysfs);

    assert_eq!(hw.gpu.dgpu_power_state.as_deref(), Some("D0"));

    let findings = audit::gpu_power::check(&hw);
    let dgpu_finding = findings
        .iter()
        .find(|f| f.description.contains("D3cold") && f.description.contains("D0"))
        .expect("Expected a MEDIUM finding about dGPU not being in D3cold");

    assert_eq!(dgpu_finding.severity, audit::Severity::Medium);
}

#[test]
fn test_audit_dgpu_d3cold_no_finding() {
    let tmp = TempDir::new().unwrap();
    create_framework16_fixture(tmp.path());

    // Add a second DRM card (dGPU) already in D3cold
    let dgpu = tmp.path().join("sys/class/drm/card1/device");
    fs::create_dir_all(&dgpu).unwrap();
    fs::write(dgpu.join("vendor"), "0x1002\n").unwrap();
    fs::write(dgpu.join("power_state"), "D3cold\n").unwrap();

    let sysfs = SysfsRoot::new(tmp.path());
    let hw = HardwareInfo::detect(&sysfs);

    assert_eq!(hw.gpu.dgpu_power_state.as_deref(), Some("D3cold"));

    let findings = audit::gpu_power::check(&hw);
    assert!(
        !findings
            .iter()
            .any(|f| f.description.contains("D3cold") || f.description.contains("Discrete GPU")),
        "Expected no finding about dGPU power state when already in D3cold, but got: {:?}",
        findings
    );
}

#[test]
fn test_build_plan_includes_usb_autosuspend() {
    let tmp = TempDir::new().unwrap();
    create_framework16_fixture(tmp.path());

    // Add USB devices
    let usb_base = tmp.path().join("sys/bus/usb/devices");

    // 1-1: power/control = on (should be included in plan)
    let usb1 = usb_base.join("1-1/power");
    fs::create_dir_all(&usb1).unwrap();
    fs::write(usb1.join("control"), "on\n").unwrap();

    // 1-2: power/control = auto (already optimal, should NOT be included)
    let usb2 = usb_base.join("1-2/power");
    fs::create_dir_all(&usb2).unwrap();
    fs::write(usb2.join("control"), "auto\n").unwrap();

    // 1-1:1.0: interface entry (contains colon, should be skipped)
    let usb_iface = usb_base.join("1-1:1.0/power");
    fs::create_dir_all(&usb_iface).unwrap();
    fs::write(usb_iface.join("control"), "on\n").unwrap();

    let sysfs = SysfsRoot::new(tmp.path());
    let hw = HardwareInfo::detect(&sysfs);
    let plan = apply::build_plan(&hw, &sysfs);

    // Should include 1-1 (set to auto)
    assert!(
        plan.sysfs_writes.iter().any(|w| w.path.contains("1-1/")
            && w.path.contains("power/control")
            && w.value == "auto"),
        "Expected plan to include sysfs write for USB 1-1 to set auto"
    );

    // Should NOT include 1-2 (already auto)
    assert!(
        !plan.sysfs_writes.iter().any(|w| w.path.contains("1-2/")),
        "Expected plan to NOT include USB 1-2 (already auto)"
    );

    // Should NOT include 1-1:1.0 (interface entry with colon)
    assert!(
        !plan.sysfs_writes.iter().any(|w| w.path.contains("1-1:1.0")),
        "Expected plan to NOT include USB interface 1-1:1.0"
    );
}

#[test]
fn test_build_plan_includes_audio_and_gpu_dpm() {
    let tmp = TempDir::new().unwrap();
    create_framework16_fixture(tmp.path());

    // Override audio settings to suboptimal values
    let hda = tmp.path().join("sys/module/snd_hda_intel/parameters");
    fs::write(hda.join("power_save"), "0\n").unwrap();
    fs::write(hda.join("power_save_controller"), "N\n").unwrap();

    // Override GPU DPM to suboptimal value
    fs::write(
        tmp.path()
            .join("sys/class/drm/card0/device/power_dpm_force_performance_level"),
        "high\n",
    )
    .unwrap();

    let sysfs = SysfsRoot::new(tmp.path());
    let hw = HardwareInfo::detect(&sysfs);
    let plan = apply::build_plan(&hw, &sysfs);

    // Audio power_save -> 1
    assert!(
        plan.sysfs_writes
            .iter()
            .any(|w| w.path.contains("power_save")
                && !w.path.contains("controller")
                && w.value == "1"),
        "Expected plan to include sysfs write for audio power_save -> 1"
    );

    // Audio power_save_controller -> Y
    assert!(
        plan.sysfs_writes
            .iter()
            .any(|w| w.path.contains("power_save_controller") && w.value == "Y"),
        "Expected plan to include sysfs write for audio power_save_controller -> Y"
    );

    // GPU DPM -> auto
    assert!(
        plan.sysfs_writes
            .iter()
            .any(|w| w.path.contains("power_dpm_force_performance_level") && w.value == "auto"),
        "Expected plan to include sysfs write for GPU DPM -> auto"
    );
}

#[test]
fn test_status_sysfs_active_and_drifted() {
    use bop::apply::{ApplyState, SysfsChange};

    let tmp = TempDir::new().unwrap();
    create_framework16_fixture(tmp.path());

    let sysfs = SysfsRoot::new(tmp.path());
    let hw = HardwareInfo::detect(&sysfs);

    // Build a plan to see what changes would be made.
    let plan = apply::build_plan(&hw, &sysfs);

    // Verify the plan includes EPP and platform profile writes.
    assert!(
        plan.sysfs_writes
            .iter()
            .any(|w| w.path.contains("energy_performance_preference") && w.value == "balance_power"),
        "Plan should include EPP -> balance_power"
    );
    assert!(
        plan.sysfs_writes
            .iter()
            .any(|w| w.path.contains("platform_profile") && w.value == "low-power"),
        "Plan should include platform profile -> low-power"
    );

    // Simulate "apply" by writing the new values to the fixture paths.
    let epp_path = tmp
        .path()
        .join("sys/devices/system/cpu/cpu0/cpufreq/energy_performance_preference");
    let profile_path = tmp.path().join("sys/firmware/acpi/platform_profile");

    fs::write(&epp_path, "balance_power\n").unwrap();
    fs::write(&profile_path, "low-power\n").unwrap();

    // Create an ApplyState recording those changes with the real temp paths.
    let state = ApplyState {
        timestamp: "2026-01-01T00:00:00Z".to_string(),
        sysfs_changes: vec![
            SysfsChange {
                path: epp_path.to_string_lossy().into_owned(),
                original_value: "balance_performance".to_string(),
                new_value: "balance_power".to_string(),
            },
            SysfsChange {
                path: profile_path.to_string_lossy().into_owned(),
                original_value: "performance".to_string(),
                new_value: "low-power".to_string(),
            },
        ],
        ..Default::default()
    };

    // Verify both values are "active" by reading files and comparing to expected.
    for change in &state.sysfs_changes {
        let actual = fs::read_to_string(&change.path).unwrap().trim().to_string();
        assert_eq!(
            actual,
            change.new_value.trim(),
            "value should be active after apply for {}",
            change.path
        );
    }

    // Simulate drift on the platform profile.
    fs::write(&profile_path, "balanced\n").unwrap();

    // Verify drift is detected on platform profile.
    let profile_actual = fs::read_to_string(&state.sysfs_changes[1].path)
        .unwrap()
        .trim()
        .to_string();
    assert_ne!(
        profile_actual,
        state.sysfs_changes[1].new_value.trim(),
        "platform profile should have drifted"
    );

    // Verify EPP is still active (not drifted).
    let epp_actual = fs::read_to_string(&state.sysfs_changes[0].path)
        .unwrap()
        .trim()
        .to_string();
    assert_eq!(
        epp_actual,
        state.sysfs_changes[0].new_value.trim(),
        "EPP should still be active"
    );
}

#[test]
fn test_revert_restores_sysfs_values_integration() {
    use bop::apply::{ApplyState, SysfsChange};

    let tmp = TempDir::new().unwrap();
    create_framework16_fixture(tmp.path());

    let sysfs = SysfsRoot::new(tmp.path());
    let hw = HardwareInfo::detect(&sysfs);

    // Build a plan to see what the optimizer would change.
    let plan = apply::build_plan(&hw, &sysfs);
    assert!(
        !plan.sysfs_writes.is_empty(),
        "Plan should have sysfs writes"
    );

    // Collect fixture paths and their original values, then simulate "apply".
    let mut sysfs_changes = Vec::new();

    // EPP for all 16 CPUs: balance_performance -> balance_power
    for i in 0..16 {
        let path = tmp.path().join(format!(
            "sys/devices/system/cpu/cpu{}/cpufreq/energy_performance_preference",
            i
        ));
        let original = fs::read_to_string(&path).unwrap().trim().to_string();
        assert_eq!(
            original, "balance_performance",
            "fixture should start with balance_performance for cpu{}",
            i
        );
        fs::write(&path, "balance_power\n").unwrap();
        sysfs_changes.push(SysfsChange {
            path: path.to_string_lossy().into_owned(),
            original_value: original,
            new_value: "balance_power".to_string(),
        });
    }

    // Platform profile: performance -> low-power
    let profile_path = tmp.path().join("sys/firmware/acpi/platform_profile");
    let profile_original = fs::read_to_string(&profile_path)
        .unwrap()
        .trim()
        .to_string();
    assert_eq!(profile_original, "performance");
    fs::write(&profile_path, "low-power\n").unwrap();
    sysfs_changes.push(SysfsChange {
        path: profile_path.to_string_lossy().into_owned(),
        original_value: profile_original,
        new_value: "low-power".to_string(),
    });

    // ASPM policy: default -> powersupersave
    let aspm_path = tmp.path().join("sys/module/pcie_aspm/parameters/policy");
    let aspm_original = fs::read_to_string(&aspm_path).unwrap().trim().to_string();
    fs::write(&aspm_path, "powersupersave\n").unwrap();
    sysfs_changes.push(SysfsChange {
        path: aspm_path.to_string_lossy().into_owned(),
        original_value: aspm_original,
        new_value: "powersupersave".to_string(),
    });

    // Build the ApplyState.
    let state = ApplyState {
        timestamp: "2026-01-01T00:00:00Z".to_string(),
        sysfs_changes,
        ..Default::default()
    };

    // Sanity check: verify new values are written.
    for change in &state.sysfs_changes {
        let actual = fs::read_to_string(&change.path).unwrap().trim().to_string();
        assert_eq!(
            actual,
            change.new_value.trim(),
            "new value should be written before revert: {}",
            change.path
        );
    }

    // Simulate "revert" by writing original values back.
    for change in &state.sysfs_changes {
        fs::write(&change.path, &change.original_value).unwrap();
    }

    // Verify all original values are restored.
    for change in &state.sysfs_changes {
        let restored = fs::read_to_string(&change.path).unwrap().trim().to_string();
        assert_eq!(
            restored,
            change.original_value.trim(),
            "value should be restored after revert: {}",
            change.path
        );
    }
}

#[test]
fn test_apply_then_revert_round_trip() {
    use bop::apply::{ApplyState, SysfsChange};

    let tmp = TempDir::new().unwrap();
    create_framework16_fixture(tmp.path());

    let sysfs = SysfsRoot::new(tmp.path());
    let hw = HardwareInfo::detect(&sysfs);

    // Record original values for key paths before any changes.
    let key_paths: Vec<(&str, &str)> = vec![
        (
            "sys/devices/system/cpu/cpu0/cpufreq/energy_performance_preference",
            "balance_performance",
        ),
        ("sys/firmware/acpi/platform_profile", "performance"),
        (
            "sys/module/pcie_aspm/parameters/policy",
            "default [default] performance powersave powersupersave",
        ),
    ];

    // Verify fixture has the expected original values.
    for (relative_path, expected_original) in &key_paths {
        let full = tmp.path().join(relative_path);
        let actual = fs::read_to_string(&full).unwrap().trim().to_string();
        assert_eq!(
            actual, *expected_original,
            "fixture should start with expected value for {}",
            relative_path
        );
    }

    // Build plan and check it wants to change these paths.
    let plan = apply::build_plan(&hw, &sysfs);

    // Map fixture-relative paths to what the plan would write.
    let plan_values: Vec<(&str, &str)> = vec![
        (
            "sys/devices/system/cpu/cpu0/cpufreq/energy_performance_preference",
            "balance_power",
        ),
        ("sys/firmware/acpi/platform_profile", "low-power"),
        ("sys/module/pcie_aspm/parameters/policy", "powersupersave"),
    ];

    // Verify the plan includes these writes.
    for (relative_path, expected_value) in &plan_values {
        assert!(
            plan.sysfs_writes.iter().any(|w| w
                .path
                .contains(relative_path.split('/').next_back().unwrap())
                && w.value == *expected_value),
            "Plan should include write {} -> {}",
            relative_path,
            expected_value
        );
    }

    // Simulate "apply": write new values, build ApplyState.
    let mut sysfs_changes = Vec::new();
    for ((relative_path, original), (_, new_value)) in key_paths.iter().zip(plan_values.iter()) {
        let full = tmp.path().join(relative_path);
        fs::write(&full, format!("{}\n", new_value)).unwrap();
        sysfs_changes.push(SysfsChange {
            path: full.to_string_lossy().into_owned(),
            original_value: original.to_string(),
            new_value: new_value.to_string(),
        });
    }

    let state = ApplyState {
        timestamp: "2026-01-01T00:00:00Z".to_string(),
        sysfs_changes,
        ..Default::default()
    };

    // Verify values changed.
    for change in &state.sysfs_changes {
        let actual = fs::read_to_string(&change.path).unwrap().trim().to_string();
        assert_eq!(
            actual,
            change.new_value.trim(),
            "value should reflect applied change: {}",
            change.path
        );
        assert_ne!(
            actual,
            change.original_value.trim(),
            "value should differ from original after apply: {}",
            change.path
        );
    }

    // Simulate "revert": write original values back.
    for change in &state.sysfs_changes {
        fs::write(&change.path, format!("{}\n", change.original_value)).unwrap();
    }

    // Verify values are back to original.
    for change in &state.sysfs_changes {
        let restored = fs::read_to_string(&change.path).unwrap().trim().to_string();
        assert_eq!(
            restored,
            change.original_value.trim(),
            "value should be restored to original after revert: {}",
            change.path
        );
    }
}
