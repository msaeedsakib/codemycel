use super::{ExitCode, not_implemented};
use crate::cli::{Context, UnresolvedArgs};

pub fn run(_args: &UnresolvedArgs, _ctx: &Context) -> anyhow::Result<ExitCode> {
    not_implemented("unresolved")
}
