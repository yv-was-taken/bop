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
    println!();

    // Print header
    println!(
        "{:>8} {:>10} {:>10} {:>10} {:>10} {:>10}",
        "Time".dimmed(),
        "Battery W".cyan(),
        "CPU W".cyan(),
        "SoC W".cyan(),
        "Batt %".cyan(),
        "Est Hours".cyan(),
    );
    println!("{}", "-".repeat(68).dimmed());

    let start = Instant::now();
    let rapl = power_draw::RaplReader::new(&sysfs);
    let mut prev_rapl = rapl.read_energy();

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

        print!(
            "\r{:>8} {:>10} {:>10} {:>10} {:>10} {:>10}",
            time_str,
            bat_power
                .map(|w| format!("{:.1}W", w))
                .unwrap_or_else(|| "N/A".to_string()),
            cpu_power
                .map(|w| format!("{:.1}W", w))
                .unwrap_or_else(|| "N/A".to_string()),
            soc_power
                .map(|w| format!("{:.1}W", w))
                .unwrap_or_else(|| "N/A".to_string()),
            battery
                .capacity_percent
                .map(|p| format!("{}%", p))
                .unwrap_or_else(|| "N/A".to_string()),
            est_hours
                .map(|h| format!("{:.1}h", h))
                .unwrap_or_else(|| "N/A".to_string()),
        );
        let _ = std::io::stdout().flush();

        // Move to next line every 10 readings for scrollback
        if elapsed.as_secs() % 20 == 0 {
            println!();
        }

        prev_rapl = curr_rapl;
    }
}
