use anyhow::Context as _;
use codemycel_core::config::TEMPLATE;
use codemycel_core::config::paths::{notes_dir, repo_config_path};
use codemycel_index::SyncMode;

use super::ExitCode;
use crate::cli::{Context, InitArgs};

/// Creates `.codemycel/` with the commented config and `notes/` where they are missing, then
/// builds the index in full, which on an indexed repository is `sync --full`.
pub fn run(args: &InitArgs, ctx: &Context) -> anyhow::Result<ExitCode> {
    let location = super::locate(ctx)?;
    let notes = notes_dir(&location.root);
    std::fs::create_dir_all(&notes).with_context(|| format!("creating {}", notes.display()))?;
    let config = repo_config_path(&location.root);
    if !config.exists() {
        std::fs::write(&config, TEMPLATE)
            .with_context(|| format!("writing {}", config.display()))?;
    }
    super::sync::index(
        &location,
        SyncMode::Full,
        args.no_download,
        args.scip_commit.as_deref(),
    )
}
