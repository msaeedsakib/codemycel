use super::{ExitCode, not_implemented};
use crate::cli::{Context, ImpactArgs};

pub fn run(_args: &ImpactArgs, _ctx: &Context) -> anyhow::Result<ExitCode> {
    not_implemented("impact")
}
