#![allow(clippy::unwrap_used)]

use std::io::{BufRead as _, BufReader, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use assert_cmd::cargo::cargo_bin_cmd;
use serde_json::{Value, json};
use tempfile::TempDir;

const INSTRUCTIONS: &str = "codemycel holds a verified knowledge graph of this repository: files, symbols, and the calls, references, imports, and type relations between them, each confirmed by a language server or SCIP index. Counts are lower bounds: anything marked unresolved could not be verified and is listed, never guessed. Start with `search` to find symbols by words, or pass a name you already know directly to `symbol`. Use `impact` before changing a symbol to see what depends on it, `trace` to connect two symbols, `overview` to orient in an unfamiliar area, and `status` when an answer looks stale. Symbol bodies are read from disk on request and flagged if the file changed since indexing. Write durable findings with `note`; later sessions will see them. Each answer ends with a freshness line; trust it.";

const TOOLS: [&str; 9] = [
    "search",
    "symbol",
    "impact",
    "trace",
    "overview",
    "unresolved",
    "status",
    "note",
    "notes",
];

const REPLY_TIMEOUT: Duration = Duration::from_secs(60);

/// A repository and a codemycel home, both temporary. Commands run with an empty `PATH`, so no
/// language server is found, and without downloads or the version check.
struct Sandbox {
    _temp: TempDir,
    repo: PathBuf,
    home: PathBuf,
    empty_path: PathBuf,
}

impl Sandbox {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let repo = temp.path().join("repo");
        let home = temp.path().join("home");
        let empty_path = temp.path().join("empty-path");
        for dir in [&repo, &home, &empty_path] {
            std::fs::create_dir_all(dir).unwrap();
        }
        std::fs::write(home.join("config.toml"), "no_download = true\n").unwrap();
        let sandbox = Self {
            _temp: temp,
            repo,
            home,
            empty_path,
        };
        sandbox.write(
            "Cargo.toml",
            "[package]\nname = \"tiny\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        );
        sandbox.write("src/lib.rs", "pub fn alpha() -> u8 {\n    1\n}\n");
        sandbox
    }

    fn write(&self, relative: &str, text: &str) {
        let path = self.repo.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn init(&self) {
        self.init_at(&self.repo);
    }

    /// Indexes `dir` into this home's registry; the exit code is 2 when the root is degraded
    /// without cargo.
    fn init_at(&self, dir: &Path) {
        let _ = cargo_bin_cmd!("codemycel")
            .args(["init", "--no-download"])
            .current_dir(dir)
            .env("CODEMYCEL_HOME", &self.home)
            .env("CODEMYCEL_NO_UPDATE_CHECK", "1")
            .env("PATH", &self.empty_path)
            .assert();
        assert!(dir.join(".codemycel/index.db").is_file());
    }

    fn mcp(&self) -> Client {
        let mut child = Command::new(env!("CARGO_BIN_EXE_codemycel"))
            .arg("mcp")
            .current_dir(&self.repo)
            .env("CODEMYCEL_HOME", &self.home)
            .env("CODEMYCEL_NO_UPDATE_CHECK", "1")
            .env("PATH", &self.empty_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, lines) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        let mut client = Client {
            child,
            stdin: Some(stdin),
            lines,
            next_id: 0,
            initialize_result: None,
        };
        client.initialize_result = Some(client.request(
            "initialize",
            json!({
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": { "name": "test", "version": "0" }
            }),
        ));
        client.notify("notifications/initialized");
        client
    }
}

/// A line-delimited JSON-RPC client over the server's stdio.
struct Client {
    child: Child,
    stdin: Option<ChildStdin>,
    lines: Receiver<String>,
    next_id: u64,
    initialize_result: Option<Value>,
}

impl Client {
    fn send(&mut self, message: &Value) {
        let stdin = self.stdin.as_mut().unwrap();
        writeln!(stdin, "{message}").unwrap();
        stdin.flush().unwrap();
    }

    fn notify(&mut self, method: &str) {
        self.send(&json!({ "jsonrpc": "2.0", "method": method }));
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        self.next_id += 1;
        let id = self.next_id;
        self.send(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        let deadline = Instant::now() + REPLY_TIMEOUT;
        loop {
            let line = self
                .lines
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .unwrap_or_else(|_| panic!("no reply to {method}"));
            let message: Value = serde_json::from_str(&line).unwrap();
            if message["id"] == json!(id) {
                assert!(message.get("error").is_none(), "{message}");
                return message["result"].clone();
            }
        }
    }

    fn tools(&mut self) -> Vec<Value> {
        self.request("tools/list", json!({}))["tools"]
            .as_array()
            .unwrap()
            .clone()
    }

    /// The text a tool answers.
    fn call(&mut self, tool: &str, arguments: Value) -> String {
        let result = self.request(
            "tools/call",
            json!({ "name": tool, "arguments": arguments }),
        );
        result["content"][0]["text"].as_str().unwrap().to_owned()
    }

    /// Calls `tool` until its answer satisfies `done`, or panics after a minute.
    fn call_until(&mut self, tool: &str, arguments: &Value, done: impl Fn(&str) -> bool) -> String {
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            let text = self.call(tool, arguments.clone());
            if done(&text) {
                return text;
            }
            assert!(Instant::now() < deadline, "last answer:\n{text}");
            std::thread::sleep(Duration::from_millis(250));
        }
    }
}

impl Client {
    /// The server's exit status, with stdin still open; panics after ten seconds.
    fn wait(&mut self) -> std::process::ExitStatus {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                return status;
            }
            assert!(Instant::now() < deadline, "the server did not exit");
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        drop(self.stdin.take());
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if let Ok(Some(_)) = self.child.try_wait() {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let _ = self.child.kill();
        if !std::thread::panicking() {
            panic!("the server did not exit on stdin EOF");
        }
    }
}

fn arguments(tool: &str) -> Value {
    match tool {
        "search" => json!({ "query": "alpha" }),
        "symbol" | "impact" => json!({ "name": "alpha" }),
        "trace" => json!({ "from": "alpha", "to": "alpha" }),
        "note" => json!({ "target": "alpha", "body": "x" }),
        _ => json!({}),
    }
}

fn mentions_alpha(text: &str) -> bool {
    text.contains("src/lib.rs::alpha")
}

/// The CLI's stdout for `args` in the sandbox, run like the server.
fn cli(sandbox: &Sandbox, args: &[&str]) -> String {
    let output = cargo_bin_cmd!("codemycel")
        .args(args)
        .current_dir(&sandbox.repo)
        .env("CODEMYCEL_HOME", &sandbox.home)
        .env("CODEMYCEL_NO_UPDATE_CHECK", "1")
        .env("PATH", &sandbox.empty_path)
        .output()
        .unwrap();
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn mcp_cli_text_and_json_agree() {
    let sandbox = Sandbox::new();
    sandbox.write("src/main.rs", "fn main() {\n    tiny::alpha();\n}\n");
    sandbox.init();
    let mut client = sandbox.mcp();
    client.call_until("search", &arguments("search"), mentions_alpha);
    let cases: [(&str, &[&str]); 7] = [
        ("search", &["search", "alpha"]),
        ("symbol", &["symbol", "alpha"]),
        ("impact", &["impact", "alpha"]),
        ("trace", &["trace", "alpha", "alpha"]),
        ("overview", &["overview"]),
        ("unresolved", &["unresolved"]),
        ("notes", &["notes"]),
    ];
    for (tool, args) in cases {
        let over_mcp = client.call(tool, arguments(tool));
        let text = cli(&sandbox, args);
        assert_eq!(over_mcp, text, "{tool}: MCP and CLI text differ");
        let mut json_args = args.to_vec();
        json_args.push("--json");
        let json: Value = serde_json::from_str(&cli(&sandbox, &json_args)).unwrap();
        let headers = text
            .lines()
            .filter(|line| !line.starts_with(' '))
            .filter_map(|line| line.strip_suffix(':'));
        for header in headers {
            assert!(json[header].is_array(), "{tool}: no `{header}` in {json}");
        }
        let footer = text.lines().last().unwrap();
        let commit = json["freshness"]["indexed_commit"]
            .as_str()
            .unwrap_or("no git");
        assert!(
            footer.starts_with(&format!("index: {}", &commit[..commit.len().min(7)])),
            "{tool}: {footer}"
        );
    }
}

#[test]
fn initialize_returns_verbatim_instructions() {
    let sandbox = Sandbox::new();
    let client = sandbox.mcp();

    let result = client.initialize_result.clone().unwrap();

    assert_eq!(result["instructions"], json!(INSTRUCTIONS));
    assert_eq!(result["serverInfo"]["name"], json!("codemycel"));
    assert_eq!(
        result["serverInfo"]["version"],
        json!(env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn tools_list_has_nine_tools_with_short_descriptions() {
    let sandbox = Sandbox::new();
    let mut client = sandbox.mcp();

    let tools = client.tools();

    let mut names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    names.sort_unstable();
    let mut expected = TOOLS;
    expected.sort_unstable();
    assert_eq!(names, expected);
    for tool in &tools {
        let words = tool["description"]
            .as_str()
            .unwrap()
            .split_whitespace()
            .count();
        assert!(words <= 60, "{}: {words} words", tool["name"]);
    }
}

#[test]
fn note_is_the_only_non_read_only_tool() {
    let sandbox = Sandbox::new();
    let mut client = sandbox.mcp();

    for tool in client.tools() {
        let read_only = tool["annotations"]["readOnlyHint"].as_bool();
        let expected = tool["name"] != json!("note");
        assert_eq!(read_only, Some(expected), "{}", tool["name"]);
    }
}

#[test]
fn search_tool_returns_text_answer() {
    let sandbox = Sandbox::new();
    sandbox.init();
    let mut client = sandbox.mcp();

    let text = client.call("search", json!({ "query": "alpha" }));

    assert!(mentions_alpha(&text), "{text}");
    assert!(
        text.lines().last().unwrap().starts_with("index: "),
        "{text}"
    );
    assert!(serde_json::from_str::<Value>(&text).is_err(), "{text}");
}

#[test]
fn catch_up_sync_runs_on_start_and_queries_are_served_meanwhile() {
    let sandbox = Sandbox::new();
    sandbox.init();
    sandbox.write(
        "src/lib.rs",
        "pub fn alpha() -> u8 {\n    1\n}\n\npub fn written_after_init() -> u8 {\n    2\n}\n",
    );
    let mut client = sandbox.mcp();

    let first = client.call("search", json!({ "query": "alpha" }));
    assert!(mentions_alpha(&first), "{first}");

    client.call_until(
        "search",
        &json!({ "query": "written_after_init" }),
        |text| text.contains("src/lib.rs::written_after_init"),
    );
}

#[test]
fn mcp_recovers_when_index_appears() {
    let sandbox = Sandbox::new();
    let other = sandbox.repo.with_file_name("other");
    std::fs::create_dir_all(&other).unwrap();
    std::fs::write(other.join("README.md"), "# Other\n").unwrap();
    sandbox.init_at(&other);
    let mut client = sandbox.mcp();

    for tool in TOOLS {
        let text = client.call(tool, arguments(tool));
        let mut lines = text.lines();
        let first = lines.next().unwrap();
        assert!(
            first.starts_with("no index: run codemycel init in ") && first.ends_with("repo"),
            "{tool}: {text}"
        );
        assert_eq!(
            lines.next(),
            Some("indexed repositories:"),
            "{tool}: {text}"
        );
        let listed = lines.next().unwrap();
        assert!(
            listed.starts_with("  ") && listed.ends_with("other"),
            "{tool}: {text}"
        );
        assert_eq!(lines.next(), None, "{tool}: {text}");
    }
    assert!(!lock_exists(&sandbox.repo));

    sandbox.init();

    let text = client.call("search", json!({ "query": "alpha" }));
    assert!(mentions_alpha(&text), "{text}");
}

#[cfg(unix)]
#[test]
fn sigterm_stops_the_session_and_releases_its_lock() {
    let sandbox = Sandbox::new();
    sandbox.init();
    let mut client = sandbox.mcp();
    let owner = sandbox.repo.join(".codemycel/session.owner");
    client.call("search", json!({ "query": "alpha" }));
    let deadline = Instant::now() + Duration::from_secs(30);
    while !owner.exists() {
        assert!(Instant::now() < deadline, "the session never took its lock");
        std::thread::sleep(Duration::from_millis(50));
    }

    let killed = Command::new("kill")
        .args(["-TERM", &client.child.id().to_string()])
        .status()
        .unwrap();

    assert!(killed.success());
    assert_eq!(client.wait().code(), Some(0));
    assert!(!owner.exists());
}

fn lock_exists(repo: &Path) -> bool {
    repo.join(".codemycel/session.lock").exists()
}
