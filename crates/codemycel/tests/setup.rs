#![allow(clippy::unwrap_used)]

use std::path::PathBuf;

use assert_cmd::cargo::cargo_bin_cmd;
use tempfile::TempDir;

const OTHER_JSON: &str = r#"{
  "numStartups": 3,
  "mcpServers": {
    "other": {
      "type": "stdio",
      "command": "other-server",
      "args": []
    }
  },
  "projects": {}
}
"#;

const OTHER_TOML: &str = r#"# my settings
model = "o3"

[mcp_servers.other]
command = "other-server"
args = ["--flag"]  # keep this

[profiles.fast]
model = "o4-mini"
"#;

const OTHER_COMPACT: &str = r#"{"mcpServers":{"other":{"command":"other-server"}}}"#;

const OTHER_COMMENTED: &str = r#"{
  // the look
  "theme": "Dracula",
  "mcpServers": {
    "other": { "command": "other-server" }
  }
}
"#;

const OTHER_JSONC: &str = r#"{
  "$schema": "https://opencode.ai/config.json",
  // servers
  "mcp": {
    "other": { "type": "local", "command": ["other-server"], "enabled": true },
  },
}
"#;

struct Case {
    id: &'static str,
    dir: &'static str,
    config: (&'static str, &'static str),
    instructions: Option<(&'static str, &'static str)>,
}

const CASES: [Case; 5] = [
    Case {
        id: "claude-code",
        dir: ".claude",
        config: (".claude.json", OTHER_JSON),
        instructions: Some((".claude/CLAUDE.md", "# Mine\n\nKeep answers short.\n")),
    },
    Case {
        id: "codex",
        dir: ".codex",
        config: (".codex/config.toml", OTHER_TOML),
        instructions: Some((".codex/AGENTS.md", "Use tabs.")),
    },
    Case {
        id: "cursor",
        dir: ".cursor",
        config: (".cursor/mcp.json", OTHER_COMPACT),
        instructions: None,
    },
    Case {
        id: "gemini",
        dir: ".gemini",
        config: (".gemini/settings.json", OTHER_COMMENTED),
        instructions: Some((".gemini/GEMINI.md", "# Gemini\n\n- be brief\n\n")),
    },
    Case {
        id: "opencode",
        dir: ".config/opencode",
        config: (".config/opencode/opencode.jsonc", OTHER_JSONC),
        instructions: Some((".config/opencode/AGENTS.md", "No emojis.\n")),
    },
];

struct Home {
    temp: TempDir,
}

impl Home {
    fn new() -> Self {
        Self {
            temp: tempfile::tempdir().unwrap(),
        }
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.temp.path().join("user").join(relative)
    }

    fn write(&self, relative: &str, text: &str) {
        let path = self.path(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn read(&self, relative: &str) -> Option<String> {
        std::fs::read_to_string(self.path(relative)).ok()
    }

    fn setup(&self, args: &[&str]) {
        let user = self.path("");
        cargo_bin_cmd!("codemycel")
            .arg("setup")
            .args(args)
            .env("HOME", &user)
            .env("USERPROFILE", &user)
            .env("CODEMYCEL_HOME", self.temp.path().join("codemycel"))
            .env("CODEMYCEL_NO_UPDATE_CHECK", "1")
            .current_dir(self.temp.path())
            .assert()
            .code(0);
    }
}

/// The `codemycel` server entry of a config file, as JSON.
fn entry(case: &Case, text: &str) -> Option<serde_json::Value> {
    if case.config.0.ends_with(".toml") {
        let document: toml_edit::DocumentMut = text.parse().unwrap();
        let table = document.get("mcp_servers")?.get("codemycel")?.as_table()?;
        let command = table.get("command")?.as_str()?.to_owned();
        let args: Vec<_> = table
            .get("args")?
            .as_array()?
            .iter()
            .map(|v| v.as_str().unwrap().to_owned())
            .collect();
        return Some(serde_json::json!({ "command": command, "args": args }));
    }
    let value = jsonc_parser::cst::CstRootNode::parse(text, &Default::default())
        .unwrap()
        .to_serde_value()
        .unwrap();
    let servers = if case.id == "opencode" {
        "mcp"
    } else {
        "mcpServers"
    };
    value.get(servers)?.get("codemycel").cloned()
}

fn assert_registered(case: &Case, text: &str) {
    let entry = entry(case, text).unwrap_or_else(|| panic!("{}: no entry in\n{text}", case.id));
    let rendered = entry.to_string();
    assert!(
        rendered.contains("codemycel") && rendered.contains("mcp"),
        "{}: {rendered}",
        case.id
    );
}

fn instructions_text(case: &Case, home: &Home) -> Option<String> {
    case.instructions.and_then(|(path, _)| home.read(path))
}

#[test]
fn setup_merges_and_remove_restores_exactly() {
    for case in &CASES {
        let home = Home::new();
        home.write(case.config.0, case.config.1);
        if let Some((path, text)) = case.instructions {
            home.write(path, text);
        }

        home.setup(&["--harness", case.id]);

        let config = home.read(case.config.0).unwrap();
        assert_registered(case, &config);
        assert!(config.contains("other-server"), "{}: {config}", case.id);
        for comment in ["// the look", "// servers", "# my settings", "# keep this"] {
            if case.config.1.contains(comment) {
                assert!(config.contains(comment), "{}: {config}", case.id);
            }
        }
        if let Some((_, seeded)) = case.instructions {
            let text = instructions_text(case, &home).unwrap();
            assert!(text.starts_with(seeded), "{}: {text}", case.id);
            assert!(text.contains("<!-- codemycel:start -->\n## codemycel\n"));
            assert!(text.contains("<!-- codemycel:end -->"));
        }

        home.setup(&["--remove", "--harness", case.id]);

        assert_eq!(
            home.read(case.config.0).as_deref(),
            Some(case.config.1),
            "{}",
            case.id
        );
        if let Some((_, seeded)) = case.instructions {
            assert_eq!(
                instructions_text(case, &home).as_deref(),
                Some(seeded),
                "{}",
                case.id
            );
        }
    }
}

#[test]
fn setup_is_idempotent() {
    let home = Home::new();
    for case in &CASES {
        std::fs::create_dir_all(home.path(case.dir)).unwrap();
    }
    let ids = CASES.map(|case| case.id).join(",");
    let snapshot = |home: &Home| {
        CASES
            .iter()
            .map(|case| (home.read(case.config.0), instructions_text(case, home)))
            .collect::<Vec<_>>()
    };

    home.setup(&["--harness", &ids]);
    let first = snapshot(&home);
    home.setup(&["--harness", &ids]);

    assert_eq!(snapshot(&home), first);
    for (case, (config, instructions)) in CASES.iter().zip(&first) {
        let opencode_json = case.id == "opencode";
        let config = if opencode_json {
            home.read(".config/opencode/opencode.json")
        } else {
            config.clone()
        };
        assert_registered(case, config.as_deref().unwrap());
        if case.instructions.is_some() {
            assert_eq!(
                instructions
                    .as_deref()
                    .unwrap()
                    .matches("<!-- codemycel:start -->")
                    .count(),
                1
            );
        }
    }

    home.setup(&["--remove", "--harness", &ids]);

    for case in &CASES {
        assert!(!home.path(case.config.0).exists(), "{}", case.id);
        assert!(!home.path(".config/opencode/opencode.json").exists());
        if let Some((path, _)) = case.instructions {
            assert!(!home.path(path).exists(), "{}", case.id);
        }
    }
}
