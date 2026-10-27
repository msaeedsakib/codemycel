use super::{ExitCode, not_implemented};
use crate::cli::{Context, StatusArgs};

pub fn run(_args: &StatusArgs, _ctx: &Context) -> anyhow::Result<ExitCode> {
    not_implemented("status")
}
