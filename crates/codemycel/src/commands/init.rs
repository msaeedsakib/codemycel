use super::{ExitCode, not_implemented};
use crate::cli::{Context, InitArgs};

pub fn run(_args: &InitArgs, _ctx: &Context) -> anyhow::Result<ExitCode> {
    not_implemented("init")
}
