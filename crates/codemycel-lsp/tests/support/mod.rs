//! The scripted fake server and a `ServerAdapter` that launches it.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::time::Duration;

use codemycel_core::config::ServerConfig;
use codemycel_core::{Arch, Language, Os, ReadinessStrategy, ServerAdapter, Toolchain};

/// The fake server binary, built alongside the workspace's tests.
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

pub fn scenario(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/scenarios")
        .join(format!("{name}.json"))
}

/// Launches the fake server on one scenario.
pub struct FakeAdapter {
    pub args: &'static [&'static str],
    pub readiness: ReadinessStrategy,
    pub config_file: Option<&'static str>,
    pub concurrency: usize,
}

impl FakeAdapter {
    pub fn new(scenario: &Path) -> Self {
        let arg: &'static str = Box::leak(scenario.display().to_string().into_boxed_str());
        Self {
            args: Box::leak(vec![arg].into_boxed_slice()),
            readiness: ReadinessStrategy::WaitForQuiescent {
                timeout: Duration::from_secs(10),
            },
            config_file: None,
            concurrency: 8,
        }
    }

    pub fn readiness(mut self, readiness: ReadinessStrategy) -> Self {
        self.readiness = readiness;
        self
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
        None
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
    fn validate(&self, _: &Path) -> Result<String, String> {
        Ok("1.0.0".to_owned())
    }
    fn launch_args(&self) -> &'static [&'static str] {
        self.args
    }
    fn language_id(&self, _: &str) -> &'static str {
        "rust"
    }
    fn initialization_options(&self, config: &ServerConfig) -> serde_json::Value {
        serde_json::json!({ "buildScripts": config.build_scripts.unwrap_or(true) })
    }
    fn readiness(&self) -> ReadinessStrategy {
        self.readiness
    }
    fn request_concurrency(&self) -> usize {
        self.concurrency
    }
    fn config_file_name(&self) -> Option<&'static str> {
        self.config_file
    }
}

/// Whether a process with `pid` is still running (a zombie counts as gone).
pub fn alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        let output = std::process::Command::new("ps")
            .args(["-o", "stat=", "-p", &pid.to_string()])
            .output()
            .unwrap();
        let stat = String::from_utf8_lossy(&output.stdout);
        let stat = stat.trim();
        !stat.is_empty() && !stat.starts_with('Z')
    }
    #[cfg(windows)]
    {
        let output = std::process::Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/NH", "/FO", "CSV"])
            .output()
            .unwrap();
        String::from_utf8_lossy(&output.stdout).contains(&format!("\"{pid}\""))
    }
}
