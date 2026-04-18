//! sledge daemon library entry point.

pub mod cli;
pub mod daemon;
pub mod ipc;
pub mod logging;
pub mod status;

use anyhow::Result;
use cli::{Cli, Command};

/// Dispatch a parsed [`Cli`] to the appropriate command handler.
///
/// # Errors
///
/// Propagates the handler's error.
pub fn run(mut cli: Cli) -> Result<()> {
    let cmd = cli
        .command
        .take()
        .unwrap_or(Command::Run(cli::RunArgs::default()));
    match cmd {
        Command::Run(args) => daemon::run(&cli, args),
        Command::Status => status::print_status(&cli),
        Command::Reload => status::send_reload(&cli),
        Command::Validate(args) => status::validate_config(&args.path),
        Command::CheckPermissions => status::print_permissions(),
    }
}
