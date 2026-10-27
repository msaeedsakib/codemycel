use super::{ExitCode, not_implemented};
use crate::cli::{Context, UiArgs};

pub fn run(_args: &UiArgs, _ctx: &Context) -> anyhow::Result<ExitCode> {
    not_implemented("ui")
}
