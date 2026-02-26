use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "bop",
    about = "Battery Optimization Project - hardware-aware power tuning for Linux laptops",
    version
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,

    /// Output as JSON instead of formatted tables
    #[arg(long, global = true)]
    pub json: bool,
}

#[derive(Subcommand)]
pub enum Command {
    /// Scan system and show power optimization findings
    Audit,

    /// Apply recommended optimizations
    Apply {
        /// Show what would be changed without applying
        #[arg(long)]
        dry_run: bool,
    },

    /// Real-time power draw monitoring (RAPL + battery)
    Monitor,

    /// Undo all changes from saved state
    Revert,

    /// Show current optimization state and detect drift
    Status,

    /// Manage expansion card wakeup sources (Framework-specific)
    Wake {
        #[command(subcommand)]
        action: WakeAction,
    },
}

#[derive(Subcommand)]
pub enum WakeAction {
    /// List all USB controllers, connected devices, and wake status
    List,
    /// Enable wakeup for a controller
    Enable {
        /// Controller name (e.g., XHC1)
        controller: String,
    },
    /// Disable wakeup for a controller
    Disable {
        /// Controller name (e.g., XHC1)
        controller: String,
    },
    /// Re-scan controllers and auto-enable wake for those with connected devices
    Scan,
}
