use codemycel_core::query::Budget;

use super::ExitCode;
use crate::cli::{Context, TraceArgs};

pub fn run(args: &TraceArgs, ctx: &Context) -> anyhow::Result<ExitCode> {
    super::query(ctx, |engine| {
        engine.trace(
            &args.from,
            &args.to,
            &args.kinds,
            args.max_depth,
            Budget::new(args.max_tokens),
        )
    })
}
