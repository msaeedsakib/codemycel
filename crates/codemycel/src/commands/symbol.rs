use codemycel_core::query::Budget;
use codemycel_core::query::symbol::SymbolOptions;

use super::ExitCode;
use crate::cli::{Context, SymbolArgs};

pub fn run(args: &SymbolArgs, ctx: &Context) -> anyhow::Result<ExitCode> {
    let options = SymbolOptions {
        include_body: args.include_body,
        callers: args.callers,
        callees: args.callees,
        limit: args.limit,
    };
    super::query(ctx, |engine| {
        engine.symbol(&args.name, options, Budget::new(args.max_tokens))
    })
}
