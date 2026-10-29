use codemycel_core::query::Budget;

use super::ExitCode;
use crate::cli::{Context, OverviewArgs};

pub fn run(args: &OverviewArgs, ctx: &Context) -> anyhow::Result<ExitCode> {
    super::query(ctx, |engine| {
        engine.overview(args.scope.as_deref(), Budget::new(args.max_tokens))
    })
}
