use codemycel_core::query::Budget;

use super::ExitCode;
use crate::cli::{Context, SearchArgs};

pub fn run(args: &SearchArgs, ctx: &Context) -> anyhow::Result<ExitCode> {
    super::query(ctx, |engine| {
        engine.search(
            &args.query,
            &args.kind,
            args.limit,
            Budget::new(args.max_tokens),
        )
    })
}
