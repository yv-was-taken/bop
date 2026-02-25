pub mod power_draw;

use crate::detect::battery::BatteryInfo;
use crate::error::Result;
use crate::sysfs::SysfsRoot;
use colored::Colorize;
use std::io::Write;
use std::time::{Duration, Instant};

/// Run the real-time power monitor.
pub fn run() -> Result<()> {
    let sysfs = SysfsRoot::system();

    println!("{}", "Power Monitor".bold().underline());
    println!("Press Ctrl+C to stop");

    let start = Instant::now();
    let rapl = power_draw::RaplReader::new(&sysfs);
    let mut prev_rapl = rapl.read_energy();

    let has_rapl = prev_rapl.is_some();
    if !has_rapl {
        println!(
            "  {} RAPL counters unavailable (try running with sudo for CPU/SoC power)",
            "Note:".yellow()
        );
    }

    println!();
    if has_rapl {
        println!(
            "{:>8} {:>10} {:>10} {:>10} {:>10} {:>10}",
            "Time".dimmed(),
            "Battery W".cyan(),
            "CPU W".cyan(),
            "SoC W".cyan(),
            "Batt %".cyan(),
            "Est Hours".cyan(),
        );
    } else {
        println!(
            "{:>8} {:>10} {:>10} {:>10}",
            "Time".dimmed(),
            "Battery W".cyan(),
            "Batt %".cyan(),
            "Est Hours".cyan(),
        );
    }
    println!("{}", "-".repeat(if has_rapl { 68 } else { 46 }).dimmed());

    loop {
        std::thread::sleep(Duration::from_secs(2));

        let elapsed = start.elapsed();
        let battery = BatteryInfo::detect(&sysfs);
        let curr_rapl = rapl.read_energy();

        // Battery power
        let bat_power = battery.power_watts();

        // RAPL power (delta over 2 seconds)
        let (cpu_power, soc_power) = if let (Some(prev), Some(curr)) = (&prev_rapl, &curr_rapl) {
            let dt = 2.0; // seconds
            let cpu_w = (curr.cpu_uj.saturating_sub(prev.cpu_uj)) as f64 / 1_000_000.0 / dt;
            let soc_w = (curr.soc_uj.saturating_sub(prev.soc_uj)) as f64 / 1_000_000.0 / dt;
            (Some(cpu_w), Some(soc_w))
        } else {
            (None, None)
        };

        // Estimated remaining hours
        let est_hours = match (battery.energy_wh(), bat_power) {
            (Some(energy), Some(power)) if power > 0.5 => Some(energy / power),
            _ => None,
        };

        let time_str = format!(
            "{:02}:{:02}",
            elapsed.as_secs() / 60,
            elapsed.as_secs() % 60
        );

        let fmt = |v: Option<f64>, suffix: &str| -> String {
            v.map(|w| format!("{:.1}{}", w, suffix))
                .unwrap_or_else(|| "N/A".to_string())
        };
        let batt_pct = battery
            .capacity_percent
            .map(|p| format!("{}%", p))
            .unwrap_or_else(|| "N/A".to_string());

        if has_rapl {
            print!(
                "\r{:>8} {:>10} {:>10} {:>10} {:>10} {:>10}",
                time_str,
                fmt(bat_power, "W"),
                fmt(cpu_power, "W"),
                fmt(soc_power, "W"),
                batt_pct,
                fmt(est_hours, "h"),
            );
        } else {
            print!(
                "\r{:>8} {:>10} {:>10} {:>10}",
                time_str,
                fmt(bat_power, "W"),
                batt_pct,
                fmt(est_hours, "h"),
            );
        }
        let _ = std::io::stdout().flush();

        // Move to next line every 10 readings for scrollback
        if elapsed.as_secs().is_multiple_of(20) {
            println!();
        }

        prev_rapl = curr_rapl;
    }
}
