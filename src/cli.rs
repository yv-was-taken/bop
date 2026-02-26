use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::Shell;

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

    /// Enable aggressive optimizations that trade performance for battery life.
    /// Includes deeper PCIe sleep states (L1.1/L1.2), lower TDP, turbo boost
    /// disable, and full USB autosuspend. May cause WiFi instability, input
    /// latency, or reduced performance.
    #[arg(long, global = true)]
    pub aggressive: bool,
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

    /// Automatic AC/battery power switching via udev
    Auto {
        #[command(subcommand)]
        action: Option<AutoAction>,
    },

    /// Capture system state as a JSON snapshot for debugging or profile development
    Snapshot {
        /// Output file path (default: stdout)
        #[arg(short, long)]
        output: Option<String>,
    },

    /// Generate shell completions
    Completions {
        /// Shell to generate completions for (auto-detected if omitted)
        shell: Option<Shell>,
    },
}

#[derive(Subcommand)]
pub enum AutoAction {
    /// Install udev rule for automatic switching and apply immediately
    Enable,
    /// Remove udev rule and stop automatic switching
    Disable,
    /// Show auto-switching status
    Status,
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

/// Print shell completions to stdout.
pub fn print_completions(shell: Option<Shell>) {
    let shell = shell.or_else(Shell::from_env).unwrap_or_else(|| {
        eprintln!(
            "Could not detect shell. Specify one: bop completions bash|zsh|fish|elvish|powershell"
        );
        std::process::exit(1);
    });
    clap_complete::generate(shell, &mut Cli::command(), "bop", &mut std::io::stdout());
}
