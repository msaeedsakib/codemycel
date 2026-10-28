use codemycel_core::query::Budget;

use super::ExitCode;
use crate::cli::{Context, ImpactArgs};

pub fn run(args: &ImpactArgs, ctx: &Context) -> anyhow::Result<ExitCode> {
    super::query(ctx, |engine| {
        engine.impact(
            &args.name,
            args.depth,
            &args.kinds,
            Budget::new(args.max_tokens),
        )
    })
}
