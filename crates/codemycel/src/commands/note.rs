use super::{ExitCode, not_implemented};
use crate::cli::{Context, NoteArgs};

pub fn run(_args: &NoteArgs, _ctx: &Context) -> anyhow::Result<ExitCode> {
    not_implemented("note")
}
