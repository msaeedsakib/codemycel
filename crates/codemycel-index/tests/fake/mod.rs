//! A small Rust repository on disk, the scripted fake server, and a `ServerAdapter` for it.
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use codemycel_core::config::{Config, HomeConfig, RepoConfig, ServerConfig};
use codemycel_core::{
    Arch, Candidate, Language, LanguageAdapter, Os, ReadinessStrategy, RepoPath, ServerAdapter,
    Toolchain,
};
use codemycel_lsp::client::{LineIndex, path_to_uri};
use codemycel_parse::rust::RustAdapter;
use serde_json::{Value, json};
use tempfile::TempDir;

pub fn fake_lsp() -> PathBuf {
    let exe = std::env::current_exe().unwrap();
    let dir = exe.parent().unwrap().parent().unwrap();
    let path = dir.join(format!(
        "codemycel-fake-lsp{}",
        std::env::consts::EXE_SUFFIX
    ));
    assert!(
        path.is_file(),
        "{} is missing; build it with `cargo build -p codemycel-fake-lsp`",
        path.display()
    );
    path
}

/// Launches the fake server on one scenario, for Rust roots.
pub struct FakeAdapter {
    args: &'static [&'static str],
    pub toolchain: Option<Toolchain>,
}

impl FakeAdapter {
    pub fn new(scenario: &Path) -> Self {
        let arg: &'static str = Box::leak(scenario.display().to_string().into_boxed_str());
        Self {
            args: Box::leak(vec![arg].into_boxed_slice()),
            toolchain: None,
        }
    }
}

impl ServerAdapter for FakeAdapter {
    fn language(&self) -> Language {
        Language::Rust
    }
    fn name(&self) -> &'static str {
        "fake"
    }
    fn pinned_version(&self) -> &'static str {
        "1.0.0"
    }
    fn binary_name(&self) -> &'static str {
        "codemycel-fake-lsp"
    }
    fn toolchain(&self) -> Option<Toolchain> {
        self.toolchain
    }
    fn download_url(&self, _: Os, _: Arch) -> Option<String> {
        None
    }
    fn pinned_sha256(&self, _: Os, _: Arch) -> Option<&'static str> {
        None
    }
    fn archive_member(&self, _: Os) -> Option<&'static str> {
        None
    }
    fn validate(&self, binary: &Path) -> Result<String, String> {
        if binary.is_file() {
            Ok("1.0.0".to_owned())
        } else {
            Err("missing".to_owned())
        }
    }
    fn launch_args(&self) -> &'static [&'static str] {
        self.args
    }
    fn language_id(&self, _: &str) -> &'static str {
        "rust"
    }
    fn initialization_options(&self, _: &ServerConfig) -> Value {
        Value::Null
    }
    fn readiness(&self) -> ReadinessStrategy {
        ReadinessStrategy::WaitForQuiescent {
            timeout: Duration::from_secs(10),
        }
    }
    fn request_concurrency(&self) -> usize {
        8
    }
    fn config_file_name(&self) -> Option<&'static str> {
        None
    }
}

/// A repository in a temporary directory, and a second one for the scenario and logs.
pub struct Repo {
    pub dir: TempDir,
    pub scratch: TempDir,
    texts: BTreeMap<String, String>,
}

impl Repo {
    pub fn new(files: &[(&str, &str)]) -> Self {
        let dir = tempfile::tempdir().unwrap();
        for (path, text) in files {
            let absolute = dir.path().join(path);
            std::fs::create_dir_all(absolute.parent().unwrap()).unwrap();
            std::fs::write(absolute, text).unwrap();
        }
        Self {
            dir,
            scratch: tempfile::tempdir().unwrap(),
            texts: files
                .iter()
                .map(|(p, t)| ((*p).to_owned(), (*t).to_owned()))
                .collect(),
        }
    }

    pub fn path(&self) -> &Path {
        self.dir.path()
    }

    /// The parser's candidate with `text` inside the node named `enclosing`.
    pub fn candidate(&self, file: &str, text: &str, enclosing: &str) -> Candidate {
        let path = RepoPath::new(file).unwrap();
        let parsed = RustAdapter::new()
            .parse(&path, self.texts[file].as_bytes())
            .unwrap();
        parsed
            .candidates
            .iter()
            .find(|c| c.text == text && c.enclosing.path.name() == enclosing)
            .cloned()
            .unwrap_or_else(|| {
                panic!(
                    "no `{text}` in `{enclosing}` among {:#?}",
                    parsed.candidates
                )
            })
    }

    /// A scripted answer to `method` at `candidate`'s position.
    pub fn respond(&self, method: &str, candidate: &Candidate, result: Value) -> Value {
        let index = LineIndex::new(self.texts[candidate.file.as_str()].clone());
        let at = index.to_lsp(candidate.at).unwrap();
        json!({"method": method, "uri": candidate.file.as_str(), "line": at.line,
               "character": at.character, "result": result})
    }

    /// A location at the first `needle` after `anchor` in `file`.
    pub fn location(&self, file: &str, anchor: &str, needle: &str) -> Value {
        let text = &self.texts[file];
        let start =
            text.find(anchor).unwrap() + text[text.find(anchor).unwrap()..].find(needle).unwrap();
        let index = LineIndex::new(text.clone());
        let at = |offset: usize| index.position(u32::try_from(offset).unwrap()).unwrap();
        json!({"uri": self.uri(file), "range": {"start": at(start), "end": at(start + needle.len())}})
    }

    pub fn uri(&self, file: &str) -> String {
        path_to_uri(&self.path().join(file))
            .unwrap()
            .as_str()
            .to_owned()
    }

    /// Writes a scenario that reports quiescence after `initialized` and logs every message.
    pub fn scenario(&self, responses: Value) -> PathBuf {
        let path = self.scratch.path().join("scenario.json");
        let scenario = json!({
            "after_initialized": [{"method": "experimental/serverStatus", "params": {"quiescent": true}}],
            "responses": responses,
            "log_file": self.log_path(),
        });
        std::fs::write(&path, scenario.to_string()).unwrap();
        path
    }

    pub fn log_path(&self) -> PathBuf {
        self.scratch.path().join("messages.log")
    }

    /// Every message the fake server received, one per line.
    pub fn log(&self) -> String {
        std::fs::read_to_string(self.log_path()).unwrap_or_default()
    }

    /// The messages of each server launch in launch order, with the root URI it was initialized
    /// with. Launches must not overlap.
    pub fn launches(&self) -> Vec<(String, Vec<Value>)> {
        let mut launches: Vec<(String, Vec<Value>)> = Vec::new();
        let messages = self
            .log()
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .collect::<Vec<_>>();
        for message in messages {
            if message["method"] == "initialize" {
                let root = message["params"]["rootUri"].as_str().unwrap_or_default();
                launches.push((root.trim_end_matches('/').to_owned(), Vec::new()));
            }
            if let Some((_, received)) = launches.last_mut() {
                received.push(message);
            }
        }
        launches
    }

    /// The URIs the server was sent `didOpen` for, in order.
    pub fn opened(&self) -> Vec<String> {
        self.log()
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .filter(|message| message["method"] == "textDocument/didOpen")
            .filter_map(|message| {
                message["params"]["textDocument"]["uri"]
                    .as_str()
                    .map(ToOwned::to_owned)
            })
            .collect()
    }

    /// Config whose Rust server is the fake binary.
    pub fn config(&self) -> Config {
        let mut servers = BTreeMap::new();
        servers.insert(
            Language::Rust,
            ServerConfig {
                path: Some(fake_lsp()),
                build_scripts: None,
            },
        );
        Config::merge(
            RepoConfig {
                servers,
                ..RepoConfig::default()
            },
            HomeConfig::default(),
        )
    }

    pub fn home(&self) -> PathBuf {
        self.scratch.path().join("home")
    }
}
