//! The codemycel command-line interface.

mod cli;
mod commands;

use clap::Parser;

use crate::cli::Cli;
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
    match commands::dispatch(&cli.command, &cli.context) {
        Ok(code) => code.into(),
        Err(error) => {
            eprintln!("error: {error:#}");
            ExitCode::Error.into()
        }
    }
}
