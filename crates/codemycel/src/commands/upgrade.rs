use super::{ExitCode, not_implemented};
use crate::cli::{Context, UpgradeArgs};

pub fn run(_args: &UpgradeArgs, _ctx: &Context) -> anyhow::Result<ExitCode> {
    not_implemented("upgrade")
}
