use super::{ExitCode, not_implemented};
use crate::cli::{Context, SetupArgs};

pub fn run(_args: &SetupArgs, _ctx: &Context) -> anyhow::Result<ExitCode> {
    not_implemented("setup")
}
