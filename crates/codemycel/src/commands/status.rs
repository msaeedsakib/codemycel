use codemycel_core::query::Budget;
use codemycel_index::{Lock, LockKind};

use super::ExitCode;
use crate::cli::{Context, StatusArgs};

pub fn run(args: &StatusArgs, ctx: &Context) -> anyhow::Result<ExitCode> {
    super::query(ctx, |engine| {
        let repo = engine.repo_root();
        engine.status(
            Lock::current_owner(repo, LockKind::Sync),
            Lock::current_owner(repo, LockKind::Session),
            Budget::new(args.max_tokens),
        )
    })
}
