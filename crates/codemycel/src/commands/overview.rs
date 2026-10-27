use super::{ExitCode, not_implemented};
use crate::cli::{Context, OverviewArgs};

pub fn run(_args: &OverviewArgs, _ctx: &Context) -> anyhow::Result<ExitCode> {
    not_implemented("overview")
}
