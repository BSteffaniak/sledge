//! Command-line argument definitions.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "sledge", version, about = "Low-level keyboard-event daemon", long_about = None)]
pub struct Cli {
    /// Override the config file path (default: ~/.config/sledge/config.toml)
    #[arg(long, global = true)]
    pub config: Option<PathBuf>,

    /// Override the health socket path (Linux requires a private same-user
    /// parent; default is $XDG_RUNTIME_DIR/sledge.sock with no /tmp fallback).
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
    /// Report platform permission/authorization status without grabbing devices.
    CheckPermissions,
}

#[derive(Debug, Args, Default)]
pub struct RunArgs {
    /// Write logs to stdout as well as the log file.
    #[arg(long)]
    pub stdout_logs: bool,

    /// Explicit privileged Linux broker socket. Connecting does not authorize
    /// capture; the broker must approve the session/seat independently.
    #[cfg(target_os = "linux")]
    #[arg(long)]
    pub linux_broker_socket: Option<PathBuf>,

    /// Explicit Linux desktop adapter; never inferred from environment variables.
    #[cfg(target_os = "linux")]
    #[arg(long, value_enum)]
    pub linux_desktop: Option<LinuxDesktop>,

    /// Explicit adapter socket (required for Sway, Hyprland and X11).
    #[cfg(target_os = "linux")]
    #[arg(long, requires = "linux_desktop")]
    pub linux_desktop_socket: Option<PathBuf>,

    /// X11 screen index; used only with explicitly selected X11.
    #[cfg(target_os = "linux")]
    #[arg(long, default_value_t = 0)]
    pub linux_x11_screen: usize,

    /// File containing raw MIT-MAGIC-COOKIE-1 bytes, not a full Xauthority file.
    #[cfg(target_os = "linux")]
    #[arg(long, requires = "linux_desktop")]
    pub linux_x11_cookie: Option<PathBuf>,

    /// Isolated Hyprland research listener; never enables production capture.
    #[cfg(target_os = "linux")]
    #[command(flatten)]
    pub linux_hyprland_research: HyprlandResearchArgs,

    /// Explicit source-only input method overlay.
    #[cfg(target_os = "linux")]
    #[arg(long, value_enum, requires = "linux_desktop")]
    pub linux_input_method: Option<LinuxInputMethod>,
}

#[cfg(target_os = "linux")]
#[derive(Debug, Args, Default)]
pub struct HyprlandResearchArgs {
    /// Explicit private SHP1 listener for an isolated research compositor only.
    #[arg(long, requires_all = ["linux_hyprland_research_pid", "linux_hyprland_research_api_hash", "linux_hyprland_research_artifact"], conflicts_with = "linux_broker_socket")]
    pub linux_hyprland_research_socket: Option<PathBuf>,

    /// Exact same-user isolated compositor process, never desktop discovery.
    #[arg(long, requires = "linux_hyprland_research_socket", value_parser = clap::value_parser!(i32).range(1..))]
    pub linux_hyprland_research_pid: Option<i32>,

    /// Exact research plugin API hash (identity check, not qualification).
    #[arg(long, requires = "linux_hyprland_research_socket")]
    pub linux_hyprland_research_api_hash: Option<String>,

    /// Exact research artifact identity (identity check, not qualification).
    #[arg(long, requires = "linux_hyprland_research_socket")]
    pub linux_hyprland_research_artifact: Option<String>,
}

#[cfg(target_os = "linux")]
#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum LinuxDesktop {
    Gnome,
    Kwin,
    Sway,
    Hyprland,
    X11,
}

#[cfg(target_os = "linux")]
#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum LinuxInputMethod {
    Ibus,
    Fcitx5,
}

#[derive(Debug, Args)]
pub struct ValidateArgs {
    pub path: PathBuf,
}
