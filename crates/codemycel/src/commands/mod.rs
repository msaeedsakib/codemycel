mod doctor;
mod impact;
mod init;
mod mcp;
mod note;
mod notes;
mod overview;
mod search;
mod setup;
mod status;
mod symbol;
mod sync;
mod trace;
mod ui;
mod unresolved;
mod upgrade;

use crate::cli::{Command, Context};

/// Process exit status (spec §11.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExitCode {
    Ok = 0,
    Error = 1,
    /// A root is degraded or a name lookup was ambiguous.
    Degraded = 2,
}

impl From<ExitCode> for std::process::ExitCode {
    fn from(code: ExitCode) -> Self {
        Self::from(code as u8)
    }
}

pub fn dispatch(command: &Command, ctx: &Context) -> anyhow::Result<ExitCode> {
    match command {
        Command::Init(args) => init::run(args, ctx),
        Command::Sync(args) => sync::run(args, ctx),
        Command::Mcp(args) => mcp::run(args, ctx),
        Command::Ui(args) => ui::run(args, ctx),
        Command::Setup(args) => setup::run(args, ctx),
        Command::Doctor(args) => doctor::run(args, ctx),
        Command::Upgrade(args) => upgrade::run(args, ctx),
        Command::Search(args) => search::run(args, ctx),
        Command::Symbol(args) => symbol::run(args, ctx),
        Command::Impact(args) => impact::run(args, ctx),
        Command::Trace(args) => trace::run(args, ctx),
        Command::Overview(args) => overview::run(args, ctx),
        Command::Unresolved(args) => unresolved::run(args, ctx),
        Command::Status(args) => status::run(args, ctx),
        Command::Note(args) => note::run(args, ctx),
        Command::Notes(args) => notes::run(args, ctx),
    }
}

fn not_implemented(command: &str) -> anyhow::Result<ExitCode> {
    eprintln!("not implemented: {command}");
    Ok(ExitCode::Error)
}
