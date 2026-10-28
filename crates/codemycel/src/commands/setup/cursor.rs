//! | What | Where | Format |
//! |---|---|---|
//! | detected by | `~/.cursor/` | directory |
//! | global MCP servers | `~/.cursor/mcp.json`, `mcpServers` | JSON |
//! | global instructions | none on disk: User Rules live in Cursor's settings | — |

use std::path::Path;

use serde_json::json;

use super::{Format, Harness};

pub fn harness(home: &Path) -> Harness {
    let dir = home.join(".cursor");
    Harness {
        id: "cursor",
        name: "Cursor",
        config: dir.join("mcp.json"),
        format: Format::Json {
            servers: "mcpServers",
        },
        entry: vec![("command", json!("codemycel")), ("args", json!(["mcp"]))],
        instructions: None,
        dir,
    }
}
