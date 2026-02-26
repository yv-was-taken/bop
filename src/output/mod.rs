use crate::audit::{Finding, Severity};
use crate::detect::HardwareInfo;
use crate::status::StatusReport;
use colored::Colorize;

const LABEL_W: usize = 18;

pub fn print_hardware_summary(hw: &HardwareInfo) {
    let mut rows: Vec<(&str, String)> = vec![
        (
            "Board",
            format!(
                "{} {}",
                hw.dmi.board_vendor.as_deref().unwrap_or("Unknown"),
                hw.dmi.board_name.as_deref().unwrap_or("")
            ),
        ),
        (
            "Product",
            hw.dmi
                .product_name
                .as_deref()
                .unwrap_or("Unknown")
                .to_string(),
        ),
        (
            "CPU",
            hw.cpu
                .model_name
                .as_deref()
                .unwrap_or("Unknown")
                .to_string(),
        ),
        (
            "CPU Driver",
            hw.cpu
                .scaling_driver
                .as_deref()
                .unwrap_or("Unknown")
                .to_string(),
        ),
        (
            "EPP",
            hw.cpu.epp.as_deref().unwrap_or("Unknown").to_string(),
        ),
        (
            "GPU Driver",
            hw.gpu.driver.as_deref().unwrap_or("Unknown").to_string(),
        ),
        (
            "Platform Profile",
            hw.platform
                .platform_profile
                .as_deref()
                .unwrap_or("N/A")
                .to_string(),
        ),
        (
            "ASPM Policy",
            hw.pci.aspm_policy.as_deref().unwrap_or("N/A").to_string(),
        ),
        (
            "WiFi",
            format!(
                "{} ({})",
                hw.network.wifi_interface.as_deref().unwrap_or("None"),
                hw.network.wifi_driver.as_deref().unwrap_or("unknown")
            ),
        ),
    ];

    if hw.battery.present {
        if let (Some(cap), Some(health)) =
            (hw.battery.usable_capacity_wh(), hw.battery.health_percent)
        {
            rows.push(("Battery", format!("{:.1} Wh ({:.0}% health)", cap, health)));
        }
        if let Some(power) = hw.battery.power_watts() {
            rows.push(("Power Draw", format!("{:.1} W", power)));
        }
    }

    // Box width from content
    let eff_label_w = rows
        .iter()
        .map(|(l, _)| l.len())
        .max()
        .unwrap_or(0)
        .max(LABEL_W);
    let inner_w = rows
        .iter()
        .map(|(_, v)| eff_label_w + 2 + v.len())
        .max()
        .unwrap_or(40);

    let title = "Hardware";
    let fill = inner_w.saturating_sub(1 + title.len());
    println!("╭─ {} {}╮", title.bold(), "─".repeat(fill));

    for (label, value) in &rows {
        let padded = format!("{:<w$}", label, w = eff_label_w);
        let pad = inner_w.saturating_sub(eff_label_w + 2 + value.len());
        println!("│ {}  {}{} │", padded.dimmed(), value, " ".repeat(pad));
    }

    println!("╰{}╯", "─".repeat(inner_w + 2));
}

pub fn print_audit_findings(findings: &[Finding], score: u32) {
    if findings.is_empty() {
        println!("{}", "  No issues found. System is well optimized.".green());
        return;
    }

    let mut sorted: Vec<&Finding> = findings.iter().collect();
    sorted.sort_by_key(|f| std::cmp::Reverse(f.severity));

    let count = findings.len();
    let title = format!("Findings ({})", count);
    let divider_w: usize = 64;
    let fill = divider_w.saturating_sub(4 + title.len());
    println!("── {} {}", title.bold(), "─".repeat(fill));

    let mut prev_severity: Option<Severity> = None;
    for finding in sorted {
        if prev_severity.is_some() && prev_severity != Some(finding.severity) {
            println!();
        }
        prev_severity = Some(finding.severity);

        let sev = match finding.severity {
            Severity::High => "HIGH".red().bold(),
            Severity::Medium => " MED".yellow().bold(),
            Severity::Low => " LOW".blue().bold(),
            Severity::Info => "INFO".dimmed().bold(),
        };

        println!("  {} {}", sev, finding.description);

        let mut detail_parts = Vec::new();
        if !finding.current_value.is_empty() && !finding.recommended_value.is_empty() {
            detail_parts.push(format!(
                "{} → {}",
                finding.current_value, finding.recommended_value
            ));
        } else if !finding.current_value.is_empty() {
            detail_parts.push(finding.current_value.clone());
        } else if !finding.recommended_value.is_empty() {
            detail_parts.push(finding.recommended_value.clone());
        }
        if !finding.impact.is_empty() {
            detail_parts.push(finding.impact.clone());
        }
        if !detail_parts.is_empty() {
            println!("       {}", detail_parts.join("  ·  ").dimmed());
        }
    }

    println!("{}", "─".repeat(divider_w));

    let score_str = format!("Score: {}/100", score);
    if score >= 80 {
        println!("  {}", score_str.green().bold());
    } else if score >= 50 {
        println!("  {}", score_str.yellow().bold());
    } else {
        println!("  {}", score_str.red().bold());
    }
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

pub fn print_status(report: &StatusReport) {
    println!(
        "{} (applied {})",
        "bop status".bold(),
        report.timestamp.dimmed()
    );
    println!();

    // Sysfs
    if !report.sysfs.is_empty() {
        let active = report.sysfs.iter().filter(|s| s.active).count();
        let total = report.sysfs.len();
        println!(
            "  {} Sysfs Optimizations ({}/{})",
            ">>".cyan(),
            active,
            total
        );
        for s in &report.sysfs {
            if s.active {
                println!("     {} {}  {}", "✓".green(), s.path.dimmed(), s.expected);
            } else if let Some(actual) = &s.actual {
                println!("     {} {}", "✗".red(), s.path);
                println!(
                    "       expected: {}  actual: {}",
                    s.expected.green(),
                    actual.red()
                );
            } else {
                println!("     {} {}  (path not found)", "?".yellow(), s.path);
            }
        }
        println!();
    }

    // ACPI wakeup
    if !report.acpi_wakeup.is_empty() {
        let active = report.acpi_wakeup.iter().filter(|w| w.active).count();
        let total = report.acpi_wakeup.len();
        println!(
            "  {} ACPI Wakeup ({}/{} disabled)",
            ">>".cyan(),
            active,
            total
        );
        for w in &report.acpi_wakeup {
            if w.active {
                println!("     {} {} disabled", "✓".green(), w.device);
            } else {
                println!("     {} {} re-enabled (drifted)", "✗".red(), w.device);
            }
        }
        println!();
    }

    // Kernel params
    if !report.kernel_params.is_empty() {
        let active = report.kernel_params.iter().filter(|k| k.in_cmdline).count();
        let total = report.kernel_params.len();
        println!(
            "  {} Kernel Parameters ({}/{})",
            ">>".cyan(),
            active,
            total
        );
        for k in &report.kernel_params {
            if k.in_cmdline {
                println!("     {} {}", "✓".green(), k.param);
            } else {
                println!("     {} {} (pending reboot)", "⏳".yellow(), k.param);
            }
        }
        println!();
    }

    // Services
    if !report.services.is_empty() {
        let active = report.services.iter().filter(|s| s.still_stopped).count();
        let total = report.services.len();
        println!(
            "  {} Services ({}/{} stopped)",
            ">>".cyan(),
            active,
            total
        );
        for s in &report.services {
            if s.still_stopped {
                println!("     {} {} stopped", "✓".green(), s.name);
            } else {
                println!("     {} {} running (drifted)", "✗".red(), s.name);
            }
        }
        println!();
    }

    // Systemd unit
    if let Some(unit) = &report.systemd_unit {
        println!("  {} Systemd Persistence", ">>".cyan());
        if unit.exists {
            println!("     {} {} installed", "✓".green(), unit.path);
        } else {
            println!("     {} {} missing", "✗".red(), unit.path);
        }
        println!();
    }

    // Summary
    let active = report.active_count();
    let total = report.total_count();
    let drifted = report.drifted_count();
    if drifted == 0 {
        println!(
            "  {}",
            format!("All {total} optimizations active.").green().bold()
        );
    } else {
        println!(
            "  {}",
            format!("{active}/{total} optimizations active, {drifted} drifted")
                .yellow()
                .bold()
        );
    }
}

pub fn print_status_json(report: &StatusReport) {
    println!(
        "{}",
        serde_json::to_string_pretty(report).unwrap()
    );
}
