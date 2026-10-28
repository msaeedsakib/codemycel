//! A language server that replays a JSON scenario over stdio, for tests.
//!
//! Usage: `codemycel-fake-lsp <scenario.json>`. `$ROOT` in any string of a scripted result or
//! notification is replaced by the `rootUri` from `initialize`.

use std::io::{self, BufRead, BufReader, Write};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Scenario {
    /// Answer `initialize`, then answer nothing and send nothing.
    silent_after_initialize: bool,
    /// Notifications sent after `initialized`, in order.
    after_initialized: Vec<Scripted>,
    /// Per launch: how many requests are answered before the process exits with status 1.
    crash_after: Vec<Option<u32>>,
    /// A file counting launches, so `crash_after` can differ after a restart.
    state_file: Option<String>,
    /// A file that receives every incoming message, one JSON object per line.
    log_file: Option<String>,
    /// Start a long-sleeping child process and report its pid in a `fake/child` notification.
    spawn_child: bool,
    responses: Vec<Response>,
    /// Notifications sent after a `didOpen` of a URI ending in `uri`.
    on_open: Vec<OnOpen>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Scripted {
    method: String,
    params: Value,
    #[serde(default)]
    delay_ms: u64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct OnOpen {
    uri: String,
    method: String,
    params: Value,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Response {
    method: String,
    /// Matches a request whose document URI ends with `/<uri>`; any document when absent.
    uri: Option<String>,
    /// 0-based LSP line and UTF-16 character; any position when absent.
    line: Option<u32>,
    character: Option<u32>,
    #[serde(default)]
    result: Value,
    /// The first `fail_times` matches get an internal error.
    #[serde(default)]
    fail_times: u32,
    /// The first `silent_times` matches get no answer at all.
    #[serde(default)]
    silent_times: u32,
    #[serde(default)]
    delay_ms: u64,
}

type Out = Arc<Mutex<io::Stdout>>;

fn main() {
    let mut args = std::env::args().skip(1);
    let first = args.next().unwrap_or_default();
    if first == "--sleep" {
        std::thread::sleep(Duration::from_secs(600));
        return;
    }
    let scenario: Scenario = match std::fs::read_to_string(&first)
        .map_err(|e| e.to_string())
        .and_then(|text| serde_json::from_str(&text).map_err(|e| e.to_string()))
    {
        Ok(scenario) => scenario,
        Err(error) => {
            eprintln!("fake-lsp: cannot load scenario {first}: {error}");
            std::process::exit(2);
        }
    };
    let launch = scenario.state_file.as_deref().map_or(0, count_launch);
    let crash_after = scenario.crash_after.get(launch).copied().flatten();
    let out: Out = Arc::new(Mutex::new(io::stdout()));
    let mut log = scenario.log_file.as_ref().and_then(|path| {
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .ok()
    });
    let mut used = vec![0u32; scenario.responses.len()];
    let mut answered = 0u32;
    let mut root = String::new();
    let mut initialized = false;
    let mut stdin = BufReader::new(io::stdin());
    let mut _child = None;
    while let Some(message) = read(&mut stdin) {
        if let Some(log) = log.as_mut() {
            let _ = writeln!(log, "{message}");
        }
        let method = message["method"].as_str().unwrap_or_default().to_owned();
        let id = message.get("id").cloned();
        match (method.as_str(), id) {
            ("initialize", Some(id)) => {
                root = message["params"]["rootUri"]
                    .as_str()
                    .unwrap_or_default()
                    .trim_end_matches('/')
                    .to_owned();
                initialized = true;
                send(
                    &out,
                    &json!({"jsonrpc": "2.0", "id": id, "result": {"capabilities": {}}}),
                );
            }
            _ if scenario.silent_after_initialize && initialized => {}
            ("initialized", None) => {
                if scenario.spawn_child {
                    _child = spawn_child(&out);
                }
                let out = out.clone();
                let scripted: Vec<(String, Value, u64)> = scenario
                    .after_initialized
                    .iter()
                    .map(|s| (s.method.clone(), substitute(&s.params, &root), s.delay_ms))
                    .collect();
                std::thread::spawn(move || {
                    for (method, params, delay) in scripted {
                        std::thread::sleep(Duration::from_millis(delay));
                        send(
                            &out,
                            &json!({"jsonrpc": "2.0", "method": method, "params": params}),
                        );
                    }
                });
            }
            ("exit", None) => std::process::exit(0),
            ("textDocument/didOpen", None) => {
                let uri = message["params"]["textDocument"]["uri"]
                    .as_str()
                    .unwrap_or_default();
                for open in &scenario.on_open {
                    if ends_with_path(uri, &open.uri) {
                        let params = substitute(&open.params, &root);
                        send(
                            &out,
                            &json!({"jsonrpc": "2.0", "method": open.method, "params": params}),
                        );
                    }
                }
            }
            ("shutdown", Some(id)) => {
                send(&out, &json!({"jsonrpc": "2.0", "id": id, "result": null}));
            }
            (_, Some(id)) => {
                if crash_after.is_some_and(|limit| answered >= limit) {
                    std::process::exit(1);
                }
                answered += 1;
                let (uri, position) = target(&message["params"]);
                let matched = scenario.responses.iter().enumerate().find(|(_, r)| {
                    r.method == method
                        && r.uri.as_deref().is_none_or(|u| ends_with_path(&uri, u))
                        && r.line.is_none_or(|l| Some(l) == position.map(|p| p.0))
                        && r.character.is_none_or(|c| Some(c) == position.map(|p| p.1))
                });
                let Some((index, response)) = matched else {
                    send(&out, &json!({"jsonrpc": "2.0", "id": id, "result": null}));
                    continue;
                };
                used[index] += 1;
                let use_count = used[index];
                if use_count <= response.silent_times {
                    continue;
                }
                let reply = if use_count <= response.silent_times + response.fail_times {
                    json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32603, "message": "scripted failure"}})
                } else {
                    json!({"jsonrpc": "2.0", "id": id, "result": substitute(&response.result, &root)})
                };
                let delay = response.delay_ms;
                let out = out.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_millis(delay));
                    send(&out, &reply);
                });
            }
            _ => {}
        }
    }
}

fn count_launch(path: &str) -> usize {
    let launches: usize = std::fs::read_to_string(path)
        .ok()
        .and_then(|text| text.trim().parse().ok())
        .unwrap_or(0);
    let _ = std::fs::write(path, (launches + 1).to_string());
    launches
}

fn spawn_child(out: &Out) -> Option<std::process::Child> {
    let exe = std::env::current_exe().ok()?;
    let child = std::process::Command::new(exe)
        .arg("--sleep")
        .spawn()
        .ok()?;
    send(
        out,
        &json!({"jsonrpc": "2.0", "method": "fake/child", "params": {"pid": child.id()}}),
    );
    Some(child)
}

/// The document URI and position a request is about.
fn target(params: &Value) -> (String, Option<(u32, u32)>) {
    let (uri, position) = if params.get("item").is_some() {
        (
            &params["item"]["uri"],
            &params["item"]["selectionRange"]["start"],
        )
    } else {
        (&params["textDocument"]["uri"], &params["position"])
    };
    let position = match (position["line"].as_u64(), position["character"].as_u64()) {
        (Some(line), Some(character)) => Some((line as u32, character as u32)),
        _ => None,
    };
    (uri.as_str().unwrap_or_default().to_owned(), position)
}

fn ends_with_path(uri: &str, suffix: &str) -> bool {
    uri == suffix || uri.ends_with(&format!("/{suffix}"))
}

fn substitute(value: &Value, root: &str) -> Value {
    match value {
        Value::String(text) => Value::String(text.replace("$ROOT", root)),
        Value::Array(items) => Value::Array(items.iter().map(|v| substitute(v, root)).collect()),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(k, v)| (k.clone(), substitute(v, root)))
                .collect(),
        ),
        other => other.clone(),
    }
}

fn read(reader: &mut impl BufRead) -> Option<Value> {
    let mut length = None;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).ok()? == 0 {
            return None;
        }
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        if let Some(value) = line.strip_prefix("Content-Length:") {
            length = value.trim().parse::<usize>().ok();
        }
    }
    let mut body = vec![0; length?];
    reader.read_exact(&mut body).ok()?;
    serde_json::from_slice(&body).ok()
}

fn send(out: &Out, message: &Value) {
    let body = message.to_string();
    if let Ok(mut out) = out.lock() {
        let _ = write!(out, "Content-Length: {}\r\n\r\n{body}", body.len());
        let _ = out.flush();
    }
}
