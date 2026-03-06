use crate::preset::Preset;
use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::Shell;
use std::path::PathBuf;

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

    /// Power optimization preset: off, default, moderate, saver, supersaver
    #[arg(long, global = true, value_enum, conflicts_with = "aggressive")]
    pub preset: Option<Preset>,

    /// Deprecated: alias for --preset supersaver
    #[arg(long, global = true, hide = true, conflicts_with = "preset")]
    pub aggressive: bool,

    /// Path to config file (overrides system/user configs)
    #[arg(long, global = true)]
    pub config: Option<PathBuf>,
}

impl Cli {
    /// Return the effective preset from CLI flags.
    /// --preset takes priority; --aggressive maps to Supersaver.
    pub fn effective_preset(&self) -> Option<Preset> {
        if let Some(p) = self.preset {
            Some(p)
        } else if self.aggressive {
            Some(Preset::Supersaver)
        } else {
            None
        }
    }
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

    /// View or generate configuration
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },

    /// Generate shell completions
    Completions {
        /// Shell to generate completions for (auto-detected if omitted)
        shell: Option<Shell>,
    },
}

#[derive(Subcommand)]
pub enum ConfigAction {
    /// Print the loaded (merged) configuration
    Show,
    /// Write default config to ~/.config/bop/config.toml
    Init,
    /// Show config file locations and which exist
    Path,
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
