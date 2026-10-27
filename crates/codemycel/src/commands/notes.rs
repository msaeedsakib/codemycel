use super::{ExitCode, not_implemented};
use crate::cli::{Context, NotesArgs};

pub fn run(_args: &NotesArgs, _ctx: &Context) -> anyhow::Result<ExitCode> {
    not_implemented("notes")
}
