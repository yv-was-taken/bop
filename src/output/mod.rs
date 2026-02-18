use crate::audit::{Finding, Severity};
use crate::detect::HardwareInfo;
use colored::Colorize;
use comfy_table::{modifiers::UTF8_ROUND_CORNERS, presets::UTF8_FULL, Cell, Color, Table};

pub fn print_hardware_summary(hw: &HardwareInfo) {
    println!("{}", "Hardware Detection".bold().underline());
    println!();

    let mut table = Table::new();
    table.load_preset(UTF8_FULL);
    table.apply_modifier(UTF8_ROUND_CORNERS);

    table.set_header(vec![
        Cell::new("Component").fg(Color::Cyan),
        Cell::new("Detected").fg(Color::Cyan),
    ]);

    // DMI
    table.add_row(vec![
        "Board",
        &format!(
            "{} {}",
            hw.dmi.board_vendor.as_deref().unwrap_or("Unknown"),
            hw.dmi.board_name.as_deref().unwrap_or("")
        ),
    ]);

    table.add_row(vec![
        "Product",
        hw.dmi.product_name.as_deref().unwrap_or("Unknown"),
    ]);

    // CPU
    table.add_row(vec![
        "CPU",
        hw.cpu.model_name.as_deref().unwrap_or("Unknown"),
    ]);
    table.add_row(vec![
        "CPU Driver",
        hw.cpu.scaling_driver.as_deref().unwrap_or("Unknown"),
    ]);
    table.add_row(vec![
        "Governor",
        hw.cpu.governor.as_deref().unwrap_or("Unknown"),
    ]);
    table.add_row(vec![
        "EPP",
        hw.cpu.epp.as_deref().unwrap_or("Unknown"),
    ]);

    // GPU
    table.add_row(vec![
        "GPU Driver",
        hw.gpu.driver.as_deref().unwrap_or("Unknown"),
    ]);

    // Battery
    if hw.battery.present {
        if let Some(cap) = hw.battery.usable_capacity_wh() {
            table.add_row(vec!["Battery Capacity", &format!("{:.1} Wh", cap)]);
        }
        if let Some(health) = hw.battery.health_percent {
            table.add_row(vec!["Battery Health", &format!("{:.1}%", health)]);
        }
        if let Some(power) = hw.battery.power_watts() {
            table.add_row(vec!["Current Power Draw", &format!("{:.1} W", power)]);
        }
    }

    // Platform
    table.add_row(vec![
        "Platform Profile",
        hw.platform.platform_profile.as_deref().unwrap_or("N/A"),
    ]);
    table.add_row(vec![
        "Sleep Mode",
        hw.platform.mem_sleep.as_deref().unwrap_or("N/A"),
    ]);

    // PCI
    table.add_row(vec![
        "ASPM Policy",
        hw.pci.aspm_policy.as_deref().unwrap_or("N/A"),
    ]);
    table.add_row(vec![
        "PCI Devices",
        &format!("{}", hw.pci.devices.len()),
    ]);

    // Network
    table.add_row(vec![
        "WiFi",
        &format!(
            "{} ({})",
            hw.network.wifi_interface.as_deref().unwrap_or("None"),
            hw.network.wifi_driver.as_deref().unwrap_or("unknown")
        ),
    ]);

    println!("{table}");
    println!();
}

pub fn print_audit_findings(findings: &[Finding], score: u32) {
    println!("{}", "Audit Findings".bold().underline());
    println!();

    if findings.is_empty() {
        println!("{}", "  No issues found! System is well optimized.".green());
        println!();
        return;
    }

    let mut table = Table::new();
    table.load_preset(UTF8_FULL);
    table.apply_modifier(UTF8_ROUND_CORNERS);

    table.set_header(vec![
        Cell::new("Sev").fg(Color::Cyan),
        Cell::new("Category").fg(Color::Cyan),
        Cell::new("Finding").fg(Color::Cyan),
        Cell::new("Current").fg(Color::Cyan),
        Cell::new("Recommended").fg(Color::Cyan),
        Cell::new("Impact").fg(Color::Cyan),
    ]);

    // Sort by severity (high first)
    let mut sorted: Vec<&Finding> = findings.iter().collect();
    sorted.sort_by_key(|f| std::cmp::Reverse(f.severity));

    for finding in sorted {
        let (sev_str, sev_color) = match finding.severity {
            Severity::High => ("HIGH", Color::Red),
            Severity::Medium => ("MED", Color::Yellow),
            Severity::Low => ("LOW", Color::Blue),
            Severity::Info => ("INFO", Color::DarkGrey),
        };

        table.add_row(vec![
            Cell::new(sev_str).fg(sev_color),
            Cell::new(&finding.category),
            Cell::new(&finding.description),
            Cell::new(&finding.current_value),
            Cell::new(&finding.recommended_value),
            Cell::new(&finding.impact),
        ]);
    }

    println!("{table}");
    println!();

    // Score
    let score_color = if score >= 80 {
        "green"
    } else if score >= 50 {
        "yellow"
    } else {
        "red"
    };

    let score_str = format!("Power Optimization Score: {}/100", score);
    match score_color {
        "green" => println!("  {}", score_str.green().bold()),
        "yellow" => println!("  {}", score_str.yellow().bold()),
        _ => println!("  {}", score_str.red().bold()),
    }
    println!();
}

pub fn print_audit_json(hw: &HardwareInfo, findings: &[Finding], score: u32, profile_name: &str) {
    let output = serde_json::json!({
        "profile": profile_name,
        "score": score,
        "hardware": {
            "board_vendor": hw.dmi.board_vendor,
            "board_name": hw.dmi.board_name,
            "cpu": hw.cpu.model_name,
            "gpu_driver": hw.gpu.driver,
            "battery_health": hw.battery.health_percent,
            "platform_profile": hw.platform.platform_profile,
        },
        "findings": findings.iter().map(|f| serde_json::json!({
            "severity": format!("{:?}", f.severity),
            "category": f.category,
            "description": f.description,
            "current": f.current_value,
            "recommended": f.recommended_value,
            "impact": f.impact,
        })).collect::<Vec<_>>(),
    });

    println!("{}", serde_json::to_string_pretty(&output).unwrap());
}
