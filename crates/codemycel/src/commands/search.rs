use super::{ExitCode, not_implemented};
use crate::cli::{Context, SearchArgs};

pub fn run(_args: &SearchArgs, _ctx: &Context) -> anyhow::Result<ExitCode> {
    not_implemented("search")
}
