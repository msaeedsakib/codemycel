//! | What | Where | Format |
//! |---|---|---|
//! | detected by | `~/.codex/` | directory |
//! | user MCP servers | `~/.codex/config.toml`, table `[mcp_servers.<name>]` | TOML |
//! | global instructions | `~/.codex/AGENTS.md` | Markdown |

use std::path::Path;

use serde_json::json;

use super::{Format, Harness};

pub fn harness(home: &Path) -> Harness {
    let dir = home.join(".codex");
    Harness {
        id: "codex",
        name: "Codex",
        config: dir.join("config.toml"),
        format: Format::Toml {
            servers: "mcp_servers",
        },
        entry: vec![("command", json!("codemycel")), ("args", json!(["mcp"]))],
        instructions: Some(dir.join("AGENTS.md")),
        dir,
    }
}
