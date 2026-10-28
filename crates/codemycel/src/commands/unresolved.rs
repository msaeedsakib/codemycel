use codemycel_core::query::Budget;

use super::ExitCode;
use crate::cli::{Context, UnresolvedArgs};

pub fn run(args: &UnresolvedArgs, ctx: &Context) -> anyhow::Result<ExitCode> {
    super::query(ctx, |engine| {
        engine.unresolved(
            args.scope.as_deref(),
            args.reason,
            Budget::new(args.max_tokens),
        )
    })
}
