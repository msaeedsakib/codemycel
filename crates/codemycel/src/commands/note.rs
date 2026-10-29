use codemycel_core::query::Budget;
use codemycel_index::{NoteRequest, NoteTool};

use super::ExitCode;
use crate::cli::{Context, NoteArgs};

/// Writes the note directly; only an MCP session that does not watch is read-only.
pub fn run(args: &NoteArgs, ctx: &Context) -> anyhow::Result<ExitCode> {
    let location = super::locate(ctx)?;
    if super::open_index(&location, ctx.json)?.is_none() {
        return Ok(ExitCode::Degraded);
    }
    let request = NoteRequest {
        target: &args.target,
        body: &args.body,
        kind: args.kind,
        replace: args.replace.as_deref(),
    };
    let answer = NoteTool::write(
        &location.root,
        &request,
        false,
        Budget::new(args.max_tokens),
    )?;
    super::print_answer(&answer, ctx.json)
}
