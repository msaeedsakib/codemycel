//! | What | Where | Format |
//! |---|---|---|
//! | detected by | `~/.claude/` | directory |
//! | user-scope MCP servers | `~/.claude.json`, top-level `mcpServers` | JSON |
//! | global instructions | `~/.claude/CLAUDE.md` | Markdown |

use std::path::Path;

use serde_json::json;

use super::{Format, Harness};

pub fn harness(home: &Path) -> Harness {
    let dir = home.join(".claude");
    Harness {
        id: "claude-code",
        name: "Claude Code",
        config: home.join(".claude.json"),
        format: Format::Json {
            servers: "mcpServers",
        },
        entry: vec![
            ("type", json!("stdio")),
            ("command", json!("codemycel")),
            ("args", json!(["mcp"])),
        ],
        instructions: Some(dir.join("CLAUDE.md")),
        dir,
    }
}
