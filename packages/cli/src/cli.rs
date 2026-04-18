//! Command-line argument definitions.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "sledge", version, about = "Low-level keyboard-event daemon", long_about = None)]
pub struct Cli {
    /// Override the config file path (default: ~/.config/sledge/config.toml)
    #[arg(long, global = true)]
    pub config: Option<PathBuf>,

    /// Override the health socket path (default:
    /// $XDG_RUNTIME_DIR/sledge.sock or /tmp/sledge-<uid>.sock)
    #[arg(long, global = true)]
    pub socket: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Run the daemon in the foreground (default when no subcommand is
    /// given).
    Run(RunArgs),
    /// Query a running daemon for status.
    Status,
    /// Ask a running daemon to reload its configuration.
    Reload,
    /// Validate a configuration file without running the daemon.
    Validate(ValidateArgs),
    /// Report whether the required macOS permissions are granted.
    CheckPermissions,
}

#[derive(Debug, Args, Default)]
pub struct RunArgs {
    /// Write logs to stdout as well as the log file.
    #[arg(long)]
    pub stdout_logs: bool,
}

#[derive(Debug, Args)]
pub struct ValidateArgs {
    pub path: PathBuf,
}
