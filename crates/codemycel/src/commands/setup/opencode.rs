//! | What | Where | Format |
//! |---|---|---|
//! | detected by | `~/.config/opencode/` | directory |
//! | global MCP servers | `~/.config/opencode/opencode.jsonc` if present, else `opencode.json`; `mcp` | JSONC |
//! | global instructions | `~/.config/opencode/AGENTS.md` | Markdown |

use std::path::Path;

use serde_json::json;

use super::{Format, Harness};

pub fn harness(home: &Path) -> Harness {
    let dir = home.join(".config").join("opencode");
    let jsonc = dir.join("opencode.jsonc");
    Harness {
        id: "opencode",
        name: "OpenCode",
        config: if jsonc.is_file() {
            jsonc
        } else {
            dir.join("opencode.json")
        },
        format: Format::Json { servers: "mcp" },
        entry: vec![
            ("type", json!("local")),
            ("command", json!(["codemycel", "mcp"])),
            ("enabled", json!(true)),
        ],
        instructions: Some(dir.join("AGENTS.md")),
        dir,
    }
}
