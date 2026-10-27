use super::{ExitCode, not_implemented};
use crate::cli::{Context, TraceArgs};

pub fn run(_args: &TraceArgs, _ctx: &Context) -> anyhow::Result<ExitCode> {
    not_implemented("trace")
}
