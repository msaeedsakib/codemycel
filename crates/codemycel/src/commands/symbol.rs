use super::{ExitCode, not_implemented};
use crate::cli::{Context, SymbolArgs};

pub fn run(_args: &SymbolArgs, _ctx: &Context) -> anyhow::Result<ExitCode> {
    not_implemented("symbol")
}
