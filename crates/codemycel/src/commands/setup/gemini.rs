//! | What | Where | Format |
//! |---|---|---|
//! | detected by | `~/.gemini/` | directory |
//! | user MCP servers | `~/.gemini/settings.json`, `mcpServers` | JSON, comments allowed |
//! | global instructions | `~/.gemini/GEMINI.md` | Markdown |

use std::path::Path;

use serde_json::json;

use super::{Format, Harness};

pub fn harness(home: &Path) -> Harness {
    let dir = home.join(".gemini");
    Harness {
        id: "gemini",
        name: "Gemini CLI",
        config: dir.join("settings.json"),
        format: Format::Json {
            servers: "mcpServers",
        },
        entry: vec![("command", json!("codemycel")), ("args", json!(["mcp"]))],
        instructions: Some(dir.join("GEMINI.md")),
        dir,
    }
}
