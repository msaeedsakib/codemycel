use super::{ExitCode, not_implemented};
use crate::cli::{Context, SyncArgs};

pub fn run(_args: &SyncArgs, _ctx: &Context) -> anyhow::Result<ExitCode> {
    not_implemented("sync")
}
