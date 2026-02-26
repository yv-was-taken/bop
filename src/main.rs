use anyhow::Result;
use bop::cli::{Cli, Command, WakeAction};
use bop::detect::HardwareInfo;
use bop::sysfs::SysfsRoot;
use clap::Parser;
use colored::Colorize;

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Audit => cmd_audit(cli.json)?,
        Command::Apply { dry_run } => cmd_apply(dry_run)?,
        Command::Monitor => cmd_monitor()?,
        Command::Revert => cmd_revert()?,
        Command::Status => cmd_status(cli.json)?,
        Command::Wake { action } => cmd_wake(action)?,
    }

    Ok(())
}

fn cmd_audit(json: bool) -> Result<()> {
    let sysfs = SysfsRoot::system();
    let hw = HardwareInfo::detect(&sysfs);

    // Find matching profile
    let profile = bop::profile::detect_profile(&hw);

    if json {
        let (findings, score) = match &profile {
            Some(p) => {
                let findings = p.audit(&hw);
                let score = bop::audit::calculate_score(&findings);
                (findings, score)
            }
            None => (Vec::new(), 100),
        };
        let profile_name = profile
            .as_ref()
            .map(|p| p.name())
            .unwrap_or("Unknown (generic)");
        bop::output::print_audit_json(&hw, &findings, score, profile_name);
        return Ok(());
    }

    bop::output::print_hardware_summary(&hw);

    match profile {
        Some(ref p) => {
            println!("  {} {}", "Matched profile:".bold(), p.name().green());

            let findings = p.audit(&hw);
            let score = bop::audit::calculate_score(&findings);
            bop::output::print_audit_findings(&findings, score);

            if !findings.is_empty() {
                println!(
                    "  Run {} to see what would change, or {} to apply.",
                    "bop apply --dry-run".cyan(),
                    "sudo bop apply".cyan()
                );
            }
        }
        None => {
            println!(
                "  {} No hardware profile matched. Generic audit only.",
                "Note:".yellow()
            );
            println!(
                "  Detected: {} {}",
                hw.dmi.board_vendor.as_deref().unwrap_or("Unknown"),
                hw.dmi.board_name.as_deref().unwrap_or("")
            );
            println!();
        }
    }

    Ok(())
}

fn cmd_apply(dry_run: bool) -> Result<()> {
    let sysfs = SysfsRoot::system();
    let hw = HardwareInfo::detect(&sysfs);

    let profile = bop::profile::detect_profile(&hw);
    if profile.is_none() {
        anyhow::bail!(
            "No hardware profile matched. Cannot apply optimizations for unknown hardware."
        );
    }

    let plan = bop::apply::build_plan(&hw, &sysfs);

    bop::apply::print_plan(&plan);

    if dry_run {
        println!("{}", "Dry run complete. No changes applied.".yellow());
        return Ok(());
    }

    if !nix::unistd::geteuid().is_root() {
        anyhow::bail!("Must run as root: sudo bop apply");
    }

    // Confirm
    println!("{}", "This will apply the changes listed above.".bold());
    print!("Continue? [y/N] ");
    std::io::Write::flush(&mut std::io::stdout())?;

    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;
    if !input.trim().eq_ignore_ascii_case("y") {
        println!("Aborted.");
        return Ok(());
    }

    println!();
    println!("{}", "Applying optimizations...".bold());

    let state = bop::apply::execute_plan(&plan, &hw, false)?;

    println!();
    println!("{}", "Applied successfully!".green().bold());
    println!(
        "  {} sysfs changes, {} kernel params, {} services disabled",
        state.sysfs_changes.len(),
        state.kernel_params_added.len(),
        state.services_disabled.len()
    );

    if !state.kernel_params_added.is_empty() {
        println!();
        println!(
            "{}",
            "  Kernel parameter changes require a reboot to take effect.".yellow()
        );
    }

    println!();
    println!(
        "  State saved. Run {} to undo all changes.",
        "sudo bop revert".cyan()
    );

    Ok(())
}

fn cmd_monitor() -> Result<()> {
    bop::monitor::run()?;
    Ok(())
}

fn cmd_revert() -> Result<()> {
    bop::revert::revert()?;
    Ok(())
}

fn cmd_status(json: bool) -> Result<()> {
    let report = match bop::status::check()? {
        Some(r) => r,
        None => {
            println!(
                "{}",
                "No optimizations applied. Run `sudo bop apply` to get started.".yellow()
            );
            return Ok(());
        }
    };

    if json {
        bop::output::print_status_json(&report);
    } else {
        bop::output::print_status(&report);
    }

    Ok(())
}

fn cmd_wake(action: WakeAction) -> Result<()> {
    match action {
        WakeAction::List => bop::wake::list()?,
        WakeAction::Enable { controller } => bop::wake::enable(&controller)?,
        WakeAction::Disable { controller } => bop::wake::disable(&controller)?,
        WakeAction::Scan => bop::wake::scan()?,
    }
    Ok(())
}
