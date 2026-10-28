use anyhow::Context as _;
use codemycel_index::SessionOptions;

use super::ExitCode;
use crate::cli::{Context, McpArgs};

pub fn run(_args: &McpArgs, ctx: &Context) -> anyhow::Result<ExitCode> {
    let location = super::locate(ctx)?;
    let home = codemycel_core::config::home_dir().context("finding the codemycel home")?;
    codemycel_mcp::serve(&location.root, SessionOptions::new(home)).context("serving MCP")?;
    Ok(ExitCode::Ok)
}
