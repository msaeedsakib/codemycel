//! The codemycel command-line interface.

mod cli;
mod commands;
mod progress;
mod update_check;

use clap::Parser;

use crate::cli::{Cli, Command};
use crate::commands::ExitCode;

fn main() -> std::process::ExitCode {
    codemycel_core::logging::init_logging();
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error) => {
            let _ = error.print();
            let code = if error.use_stderr() {
                ExitCode::Error
            } else {
                ExitCode::Ok
            };
            return code.into();
        }
    };
    let code = match commands::dispatch(&cli.command, &cli.context) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("error: {error:#}");
            ExitCode::Error
        }
    };
    if !matches!(
        cli.command,
        Command::Mcp(_) | Command::Ui(_) | Command::Upgrade(_)
    ) && let Ok(home) = codemycel_core::config::home_dir()
    {
        update_check::run(&home);
    }
    code.into()
}
