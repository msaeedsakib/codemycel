use super::{ExitCode, not_implemented};
use crate::cli::{Context, McpArgs};

pub fn run(_args: &McpArgs, _ctx: &Context) -> anyhow::Result<ExitCode> {
    not_implemented("mcp")
}
