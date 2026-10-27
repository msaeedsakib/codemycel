use std::path::Path;
use std::time::Duration;

use crate::Language;
use crate::config::ServerConfig;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Os {
    Linux,
    Macos,
    Windows,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Arch {
    X86_64,
    Aarch64,
}

/// How the enricher decides a server has loaded its project (spec §7.5 step 2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Readiness {
    /// Wait for `experimental/serverStatus` with `quiescent: true`; degrade the root on timeout.
    WaitForQuiescent { timeout: Duration },
    /// Retry `textDocument/definition` until non-empty; proceed either way after the timeout.
    ProbeDefinition { timeout: Duration },
}

/// The language-server half of a language (spec §7.1, §7.4, §7.5).
pub trait ServerAdapter: Send + Sync {
    fn language(&self) -> Language;

    /// Directory name under `~/.codemycel/servers/` and the name `status` prints.
    fn name(&self) -> &'static str;

    fn pinned_version(&self) -> &'static str;

    /// `None` when there is no release asset for the target.
    fn download_url(&self, os: Os, arch: Arch) -> Option<String>;

    /// Lowercase hex SHA-256 of the archive `download_url` returns.
    fn pinned_sha256(&self, os: Os, arch: Arch) -> Option<&'static str>;

    /// Path of the binary inside the downloaded archive; `None` when the download is the binary.
    fn archive_member(&self, os: Os) -> Option<&'static str>;

    /// The version a candidate binary reports, or why it is rejected (spec §7.4).
    fn validate(&self, binary: &Path) -> Result<String, String>;

    fn launch_args(&self) -> &'static [&'static str];

    fn initialization_options(&self, config: &ServerConfig) -> serde_json::Value;

    fn readiness(&self) -> Readiness;

    /// Requests in flight per server.
    fn request_concurrency(&self) -> usize;

    /// The per-root configuration file whose diagnostics `status` reports (spec §7.6).
    fn config_file_name(&self) -> Option<&'static str>;
}
