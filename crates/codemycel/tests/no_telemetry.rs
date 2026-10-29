#![allow(clippy::unwrap_used)]

mod support;

use std::collections::{HashMap, HashSet, VecDeque};
use std::io::{BufRead as _, BufReader, Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use support::{codemycel, fixture_copy};
use tempfile::TempDir;

/// Workspace crates allowed to depend on an HTTP client; crates depending on them inherit it.
const CLIENT_CRATES: [&str; 2] = ["codemycel-lsp", "codemycel"];

const CLIENT_MODULES: [&str; 3] = [
    "crates/codemycel-lsp/src/provision/download.rs",
    "crates/codemycel/src/commands/upgrade.rs",
    "crates/codemycel/src/update_check.rs",
];

const LISTENER_MODULES: &str = "crates/codemycel-ui/src/server/";

const CLIENT_PACKAGES: [&str; 11] = [
    "attohttpc",
    "awc",
    "curl",
    "curl-sys",
    "http_req",
    "isahc",
    "minreq",
    "reqwest",
    "surf",
    "ureq",
    "ureq-proto",
];

/// Packages that are network clients only with one of these features on.
const CLIENT_FEATURES: [(&str, &str); 3] = [
    ("hyper", "client"),
    ("hyper-util", "client"),
    ("hyper-util", "client-legacy"),
];

const CLIENT_APIS: [&str; 15] = [
    "ureq",
    "reqwest",
    "isahc",
    "attohttpc",
    "surf::",
    "curl::",
    "hyper::client",
    "hyper_util::client",
    "TcpStream",
    "TcpSocket",
    "UdpSocket",
    "ToSocketAddrs",
    "lookup_host",
    "\"curl\"",
    "\"wget\"",
];

const LISTENER_APIS: [&str; 1] = ["TcpListener"];

const DENIED: &str = "network access denied";

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn cargo() -> Command {
    let mut command = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()));
    command.current_dir(workspace_root());
    command
}

fn host_triple() -> String {
    let output = cargo().arg("-vV").output().unwrap();
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .find_map(|line| line.strip_prefix("host: "))
        .unwrap()
        .to_owned()
}

/// Every chain `member -> … -> client` through normal or build dependencies outside allowed routes.
fn client_dependency_chains() -> Vec<String> {
    let output = cargo()
        .args(["metadata", "--format-version", "1", "--all-features"])
        .args(["--filter-platform", &host_triple()])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let metadata: Value = serde_json::from_slice(&output.stdout).unwrap();
    let names: HashMap<&str, &str> = metadata["packages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|package| {
            (
                package["id"].as_str().unwrap(),
                package["name"].as_str().unwrap(),
            )
        })
        .collect();
    let nodes: HashMap<&str, &Value> = metadata["resolve"]["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|node| (node["id"].as_str().unwrap(), node))
        .collect();
    let members: HashSet<&str> = metadata["workspace_members"]
        .as_array()
        .unwrap()
        .iter()
        .map(|id| id.as_str().unwrap())
        .collect();
    let allowed = |id: &str| members.contains(id) && CLIENT_CRATES.contains(&names[id]);
    let is_client = |id: &str| {
        let name = names[id];
        CLIENT_PACKAGES.contains(&name)
            || nodes[id]["features"]
                .as_array()
                .unwrap()
                .iter()
                .any(|feature| CLIENT_FEATURES.contains(&(name, feature.as_str().unwrap())))
    };

    let mut chains = Vec::new();
    for &member in &members {
        let mut parent: HashMap<&str, &str> = HashMap::new();
        let mut queue = VecDeque::from([member]);
        while let Some(id) = queue.pop_front() {
            for dep in nodes[id]["deps"].as_array().unwrap() {
                let dep_id = dep["pkg"].as_str().unwrap();
                let runtime = dep["dep_kinds"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|kind| kind["kind"].as_str() != Some("dev"));
                if !runtime
                    || allowed(dep_id)
                    || dep_id == member
                    || (allowed(member) && id == member && names[dep_id] == "ureq")
                    || parent.contains_key(dep_id)
                {
                    continue;
                }
                parent.insert(dep_id, id);
                if is_client(dep_id) {
                    let mut chain = vec![names[dep_id]];
                    let mut at = id;
                    while at != member {
                        chain.push(names[at]);
                        at = parent[at];
                    }
                    chain.push(names[member]);
                    chain.reverse();
                    chains.push(chain.join(" -> "));
                }
                queue.push_back(dep_id);
            }
        }
    }
    chains.sort();
    chains
}

/// The lines of `source` outside `#[cfg(test)]` items, numbered from 1.
fn production_lines(source: &str) -> Vec<(usize, &str)> {
    let mut lines = Vec::new();
    let mut skipping = false;
    let mut depth = 0i32;
    let mut opened = false;
    for (index, line) in source.lines().enumerate() {
        if !skipping && line.trim() == "#[cfg(test)]" {
            (skipping, depth, opened) = (true, 0, false);
            continue;
        }
        if skipping {
            for c in line.chars() {
                match c {
                    '{' => (depth, opened) = (depth + 1, true),
                    '}' => depth -= 1,
                    _ => {}
                }
            }
            if (opened && depth <= 0) || (!opened && line.trim_end().ends_with(';')) {
                skipping = false;
            }
            continue;
        }
        lines.push((index + 1, line));
    }
    lines
}

fn rust_sources(dir: &Path, found: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_sources(&path, found);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            found.push(path);
        }
    }
}

/// `file:line: api` for every network API used outside the module allowed to use it.
fn misplaced_network_apis() -> Vec<String> {
    let root = workspace_root();
    let mut files = Vec::new();
    for entry in std::fs::read_dir(root.join("crates")).unwrap() {
        let crate_dir = entry.unwrap().path();
        if crate_dir.join("src").is_dir() {
            rust_sources(&crate_dir.join("src"), &mut files);
        }
        if crate_dir.join("build.rs").is_file() {
            files.push(crate_dir.join("build.rs"));
        }
    }
    assert!(files.len() > 50, "found only {} source files", files.len());

    let mut misplaced = Vec::new();
    for file in files {
        let relative = file
            .strip_prefix(&root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        let source = std::fs::read_to_string(&file).unwrap();
        for (number, line) in production_lines(&source) {
            for api in CLIENT_APIS {
                if line.contains(api) && !CLIENT_MODULES.contains(&relative.as_str()) {
                    misplaced.push(format!("{relative}:{number}: {api}"));
                }
            }
            for api in LISTENER_APIS {
                if line.contains(api) && !relative.starts_with(LISTENER_MODULES) {
                    misplaced.push(format!("{relative}:{number}: {api}"));
                }
            }
        }
    }
    misplaced
}

#[test]
fn no_network_outside_allowed_modules() {
    let chains = client_dependency_chains();
    assert!(
        chains.is_empty(),
        "network clients reachable outside {CLIENT_CRATES:?}:\n{}",
        chains.join("\n")
    );
    let misplaced = misplaced_network_apis();
    assert!(
        misplaced.is_empty(),
        "network APIs outside {CLIENT_MODULES:?} and {LISTENER_MODULES}:\n{}",
        misplaced.join("\n")
    );
}

#[test]
fn production_lines_skip_test_modules() {
    let source = "use a;\n#[cfg(test)]\nmod tests {\n    fn f() {\n    }\n}\nuse b;\n#[cfg(test)]\nuse c;\nuse d;\n";
    let kept: Vec<&str> = production_lines(source)
        .into_iter()
        .map(|(_, line)| line)
        .collect();
    assert_eq!(kept, ["use a;", "use b;", "use d;"]);
}

/// The Rust fixture indexed without downloads, in a home that forbids them.
struct Indexed {
    _temp: TempDir,
    repo: PathBuf,
    home: TempDir,
}

fn indexed() -> Indexed {
    let (temp, repo) = fixture_copy("rust/workspace-basic");
    let home = tempfile::tempdir().unwrap();
    std::fs::write(home.path().join("config.toml"), "no_download = true\n").unwrap();
    let output = codemycel(home.path())
        .args(["init", "--no-download", "--repo"])
        .arg(&repo)
        .output()
        .unwrap();
    assert!(
        matches!(output.status.code(), Some(0 | 2)),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    Indexed {
        _temp: temp,
        repo,
        home,
    }
}

/// How each run reaches the binary and which environment it gets.
#[derive(Clone, Copy)]
enum Network {
    DeniedByEnv,
    NoInterfaces,
}

impl Indexed {
    fn command(&self, network: Network) -> Command {
        let binary = env!("CARGO_BIN_EXE_codemycel");
        let mut command = match network {
            Network::DeniedByEnv => {
                let mut command = Command::new(binary);
                command.env("CODEMYCEL_TEST_DENY_NETWORK", "1");
                command
            }
            Network::NoInterfaces => {
                let mut command = Command::new("unshare");
                command.arg("-rn").arg(binary);
                command.env_remove("CODEMYCEL_TEST_DENY_NETWORK");
                command
            }
        };
        command
            .current_dir(&self.repo)
            .env("CODEMYCEL_HOME", self.home.path())
            .env_remove("CODEMYCEL_NO_UPDATE_CHECK");
        command
    }
}

const TOOL_RUNS: [&[&str]; 9] = [
    &["search", "circle"],
    &["symbol", "crates/core/src/geometry.rs::Area::area"],
    &["impact", "crates/core/src/geometry.rs::Area::area"],
    &[
        "trace",
        "crates/cli/src/main.rs::main",
        "crates/core/src/geometry.rs::Area::area",
    ],
    &["overview"],
    &["unresolved"],
    &["status"],
    &["notes"],
    &["note", "repo", "--body", "written offline"],
];

fn assert_offline_output(what: &str, output: &Output) {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        matches!(output.status.code(), Some(0 | 2)),
        "{what}: {}\n{stdout}\n{stderr}",
        output.status
    );
    assert!(
        !stdout.contains(DENIED) && !stderr.contains(DENIED),
        "{what}:\n{stdout}\n{stderr}"
    );
}

fn run_tools(indexed: &Indexed, network: Network) {
    for args in TOOL_RUNS {
        let output = indexed.command(network).args(args).output().unwrap();
        assert_offline_output(&args.join(" "), &output);
    }
}

fn mcp_arguments(tool: &str) -> Value {
    match tool {
        "search" => json!({ "query": "circle" }),
        "symbol" | "impact" => json!({ "name": "crates/core/src/geometry.rs::Area::area" }),
        "trace" => json!({
            "from": "crates/cli/src/main.rs::main",
            "to": "crates/core/src/geometry.rs::Area::area",
        }),
        "note" => json!({ "target": "repo", "body": "written offline" }),
        _ => json!({}),
    }
}

/// Calls every tool once over one `mcp` session, then closes stdin and expects a clean exit.
fn run_mcp(indexed: &Indexed, network: Network) {
    const REPLY_TIMEOUT: Duration = Duration::from_secs(60);
    let mut child = indexed
        .command(network)
        .arg("mcp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let mut stderr = child.stderr.take().unwrap();
    let stderr_reader = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = stderr.read_to_string(&mut text);
        text
    });
    let (sender, lines) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            if sender.send(line).is_err() {
                break;
            }
        }
    });
    let mut request = |id: Option<u64>, method: &str, params: Value| -> Value {
        let Some(id) = id else {
            writeln!(stdin, "{}", json!({ "jsonrpc": "2.0", "method": method })).unwrap();
            return Value::Null;
        };
        let message = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        writeln!(stdin, "{message}").unwrap();
        stdin.flush().unwrap();
        let deadline = Instant::now() + REPLY_TIMEOUT;
        loop {
            let line = lines
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .unwrap_or_else(|_| panic!("no reply to {method}"));
            let reply: Value = serde_json::from_str(&line).unwrap();
            if reply["id"] == json!(id) {
                assert!(reply.get("error").is_none(), "{method}: {reply}");
                return reply["result"].clone();
            }
        }
    };
    request(
        Some(1),
        "initialize",
        json!({
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": { "name": "no-telemetry", "version": "0" }
        }),
    );
    request(None, "notifications/initialized", Value::Null);
    let tools = request(Some(2), "tools/list", json!({}));
    let names: Vec<String> = tools["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(names.len(), 9, "{names:?}");
    for (id, name) in (3..).zip(&names) {
        let result = request(
            Some(id),
            "tools/call",
            json!({ "name": name, "arguments": mcp_arguments(name) }),
        );
        assert_ne!(result["isError"], json!(true), "{name}: {result}");
        assert!(!result.to_string().contains(DENIED), "{name}: {result}");
    }
    drop(stdin);

    let deadline = Instant::now() + Duration::from_secs(10);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            panic!("the mcp server did not exit on stdin EOF");
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let stderr = stderr_reader.join().unwrap();
    assert!(status.success(), "{status}\n{stderr}");
    assert!(!stderr.contains(DENIED), "{stderr}");
}

#[test]
fn tools_and_mcp_run_with_network_denied() {
    let indexed = indexed();
    run_tools(&indexed, Network::DeniedByEnv);
    run_mcp(&indexed, Network::DeniedByEnv);
}

#[cfg(target_os = "linux")]
#[test]
fn tools_and_mcp_run_without_network_interfaces() {
    let probe = Command::new("unshare").args(["-rn", "true"]).output();
    if !probe.as_ref().is_ok_and(|output| output.status.success()) {
        let reason = match probe {
            Ok(output) => String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            Err(error) => error.to_string(),
        };
        eprintln!("skipped: unshare -rn is unavailable here ({reason})");
        return;
    }
    let indexed = indexed();
    run_tools(&indexed, Network::NoInterfaces);
    run_mcp(&indexed, Network::NoInterfaces);
}
