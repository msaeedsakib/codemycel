use codemycel_core::query::Budget;

use super::ExitCode;
use crate::cli::{Context, NotesArgs};

pub fn run(args: &NotesArgs, ctx: &Context) -> anyhow::Result<ExitCode> {
    super::query(ctx, |engine| {
        engine.notes(
            args.target.as_deref(),
            args.orphans,
            Budget::new(args.max_tokens),
        )
    })
}
