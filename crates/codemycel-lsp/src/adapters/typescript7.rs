use std::path::Path;
use std::time::Duration;

use codemycel_core::config::ServerConfig;
use codemycel_core::{Arch, Language, Os, ReadinessStrategy, ServerAdapter, Toolchain};

use crate::provision::validate::{VALIDATE_TIMEOUT, command_output};

const VERSION: &str = "7.0.2";

/// The TypeScript 7 native compiler's language server, serving TypeScript and JavaScript.
#[derive(Clone, Copy, Debug, Default)]
pub struct TypeScript7;

impl TypeScript7 {
    pub fn new() -> Self {
        Self
    }
}

fn asset(os: Os, arch: Arch) -> Option<(&'static str, &'static str)> {
    Some(match (os, arch) {
        (Os::Macos, Arch::Aarch64) => (
            "darwin-arm64",
            "902e2fe1cf0799198ef902c6b8c310a450fef629a6baba41d45641ef75c04ebd",
        ),
        (Os::Macos, Arch::X86_64) => (
            "darwin-x64",
            "eba158cb54050f723d5ff781438f33de5640054440bb4f2bd170cfe9bc2eb551",
        ),
        (Os::Linux, Arch::Aarch64) => (
            "linux-arm64",
            "c83d931ac9dd7549cde6e71246aa9d6a9812843023df3e277fe3b5dcf41dd0ea",
        ),
        (Os::Linux, Arch::X86_64) => (
            "linux-x64",
            "7ecad6f67377e831856367ab062ef394f21506a611405bf8ac0ff039348637d3",
        ),
        (Os::Windows, Arch::X86_64) => (
            "win32-x64",
            "61fc4e141d2bc687db580e71bbfa63b9c209f0310645d82ca1b457eb3a24fd19",
        ),
        (Os::Windows, Arch::Aarch64) => return None,
    })
}

/// The version in `tsc --version` output such as `Version 7.0.2`.
fn reported_version(output: &str) -> Option<&str> {
    output
        .split_whitespace()
        .find(|word| word.starts_with(|c: char| c.is_ascii_digit()))
}

impl ServerAdapter for TypeScript7 {
    fn language(&self) -> Language {
        Language::TypeScript
    }

    fn name(&self) -> &'static str {
        "typescript"
    }

    fn pinned_version(&self) -> &'static str {
        VERSION
    }

    fn binary_name(&self) -> &'static str {
        "tsc"
    }

    fn toolchain(&self) -> Option<Toolchain> {
        None
    }

    fn download_url(&self, os: Os, arch: Arch) -> Option<String> {
        let (target, _) = asset(os, arch)?;
        Some(format!(
            "https://github.com/microsoft/typescript-go/releases/download/typescript%2Fv{VERSION}/typescript-{target}.tgz"
        ))
    }

    fn pinned_sha256(&self, os: Os, arch: Arch) -> Option<&'static str> {
        asset(os, arch).map(|(_, sha256)| sha256)
    }

    fn archive_member(&self, os: Os) -> Option<&'static str> {
        Some(match os {
            Os::Windows => "package/lib/tsc.exe",
            Os::Linux | Os::Macos => "package/lib/tsc",
        })
    }

    /// Requires `--version` to report 7 or later and the binary to run `--lsp --stdio`, which
    /// exits at once on a closed stdin. A Node-based TypeScript 5 `tsc` fails the first check.
    fn validate(&self, binary: &Path) -> Result<String, String> {
        let output = command_output(binary, &["--version"], VALIDATE_TIMEOUT)?;
        let version = reported_version(&output)
            .ok_or_else(|| format!("`--version` printed no version: {output}"))?;
        let major = version
            .split('.')
            .next()
            .and_then(|major| major.parse::<u32>().ok())
            .ok_or_else(|| format!("`--version` printed an unreadable version: {output}"))?;
        if major < 7 {
            return Err(format!(
                "TypeScript {version} has no language server; TypeScript 7 or later is required"
            ));
        }
        command_output(binary, &["--lsp", "--stdio"], VALIDATE_TIMEOUT)
            .map_err(|error| format!("TypeScript {version} does not accept `--lsp`: {error}"))?;
        Ok(version.to_owned())
    }

    fn launch_args(&self) -> &'static [&'static str] {
        &["--lsp", "--stdio"]
    }

    fn language_id(&self, extension: &str) -> &'static str {
        match extension {
            "tsx" => "typescriptreact",
            "js" | "mjs" | "cjs" => "javascript",
            "jsx" => "javascriptreact",
            _ => "typescript",
        }
    }

    fn initialization_options(&self, _: &ServerConfig) -> serde_json::Value {
        serde_json::Value::Null
    }

    fn readiness(&self) -> ReadinessStrategy {
        ReadinessStrategy::ProbeDefinition {
            timeout: Duration::from_secs(60),
        }
    }

    fn request_concurrency(&self) -> usize {
        8
    }

    fn config_file_name(&self) -> Option<&'static str> {
        Some("tsconfig.json")
    }
}
