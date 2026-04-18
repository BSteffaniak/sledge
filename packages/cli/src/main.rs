use std::process::ExitCode;

use clap::Parser;

fn main() -> ExitCode {
    let cli = sledge::cli::Cli::parse();
    match sledge::run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err}");
            let mut source = err.source();
            while let Some(e) = source {
                eprintln!("  caused by: {e}");
                source = e.source();
            }
            ExitCode::FAILURE
        }
    }
}
