use std::path::Path;
use std::time::Duration;

use codemycel_core::config::ServerConfig;
use codemycel_core::{Arch, Language, Os, ReadinessStrategy, ServerAdapter, Toolchain};
use serde_json::json;

use crate::provision::validate::{VALIDATE_TIMEOUT, command_output};

const VERSION: &str = "2026-10-26";

#[derive(Clone, Copy, Debug, Default)]
pub struct RustAnalyzer;

impl RustAnalyzer {
    pub fn new() -> Self {
        Self
    }
}

fn target(os: Os, arch: Arch) -> Option<&'static str> {
    match (os, arch) {
        (Os::Linux, Arch::X86_64) => Some("x86_64-unknown-linux-gnu"),
        (Os::Linux, Arch::Aarch64) => Some("aarch64-unknown-linux-gnu"),
        (Os::Macos, Arch::X86_64) => Some("x86_64-apple-darwin"),
        (Os::Macos, Arch::Aarch64) => Some("aarch64-apple-darwin"),
        (Os::Windows, Arch::X86_64) => Some("x86_64-pc-windows-msvc"),
        (Os::Windows, Arch::Aarch64) => None,
    }
}

impl ServerAdapter for RustAnalyzer {
    fn language(&self) -> Language {
        Language::Rust
    }

    fn name(&self) -> &'static str {
        "rust-analyzer"
    }

    fn pinned_version(&self) -> &'static str {
        VERSION
    }

    fn binary_name(&self) -> &'static str {
        "rust-analyzer"
    }

    fn toolchain(&self) -> Option<Toolchain> {
        Some(Toolchain {
            prerequisite: "cargo",
            locate: &["rustup", "which", "rust-analyzer"],
            install: &["rustup", "component", "add", "rust-analyzer"],
        })
    }

    fn download_url(&self, os: Os, arch: Arch) -> Option<String> {
        let target = target(os, arch)?;
        let extension = if os == Os::Windows { "zip" } else { "gz" };
        Some(format!(
            "https://github.com/rust-lang/rust-analyzer/releases/download/{VERSION}/rust-analyzer-{target}.{extension}"
        ))
    }

    fn pinned_sha256(&self, os: Os, arch: Arch) -> Option<&'static str> {
        match (os, arch) {
            (Os::Linux, Arch::X86_64) => {
                Some("4c83773525f93b636ed79761389568b4d2a89c05fa7ecc440245032d49ad80ce")
            }
            (Os::Linux, Arch::Aarch64) => {
                Some("1b225c4c9ae329ca96e8a75a5bbcb5562a7f1d25c57c8b55d3e02299a3dbe836")
            }
            (Os::Macos, Arch::X86_64) => {
                Some("b3e4904877bcf7013a4a342c449cff1bf0ea0b91d5a1a30ac8df3d32142519c0")
            }
            (Os::Macos, Arch::Aarch64) => {
                Some("8fb90cb58f5910d8d8b24ac1a93cca49f053365cbf0eb3c682144ad0a0027dcf")
            }
            (Os::Windows, Arch::X86_64) => {
                Some("13549dd0c380647685c82c87b8d80904f81bc4de92a82c099b6083c5b9aa1ca1")
            }
            (Os::Windows, Arch::Aarch64) => None,
        }
    }

    fn archive_member(&self, os: Os) -> Option<&'static str> {
        (os == Os::Windows).then_some("rust-analyzer.exe")
    }

    fn validate(&self, binary: &Path) -> Result<String, String> {
        let output = command_output(binary, &["--version"], VALIDATE_TIMEOUT)?;
        let version = output
            .lines()
            .next()
            .unwrap_or_default()
            .trim()
            .trim_start_matches("rust-analyzer")
            .trim();
        if version.is_empty() {
            Err(format!("`{} --version` printed no version", binary.display()))
        } else {
            Ok(version.to_owned())
        }
    }

    fn launch_args(&self) -> &'static [&'static str] {
        &[]
    }

    fn language_id(&self, _extension: &str) -> &'static str {
        "rust"
    }

    fn initialization_options(&self, config: &ServerConfig) -> serde_json::Value {
        let build_scripts = config.build_scripts.unwrap_or(true);
        json!({
            "checkOnSave": false,
            "diagnostics": { "enable": false },
            "cachePriming": { "enable": true },
            "cargo": { "buildScripts": { "enable": build_scripts } },
            "procMacro": { "enable": build_scripts },
        })
    }

    fn readiness(&self) -> ReadinessStrategy {
        ReadinessStrategy::WaitForQuiescent {
            timeout: Duration::from_secs(10 * 60),
        }
    }

    fn request_concurrency(&self) -> usize {
        8
    }

    fn config_file_name(&self) -> Option<&'static str> {
        None
    }
}
