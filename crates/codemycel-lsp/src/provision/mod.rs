pub mod cache;
pub mod download;
pub mod lookup;
pub mod validate;

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use codemycel_core::config::{Config, ServerConfig};
use codemycel_core::{Arch, Language, Os, ServerAdapter, ServerSource};
use thiserror::Error;

/// A server binary that passed validation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Located {
    pub path: PathBuf,
    pub version: String,
    pub source: ServerSource,
    /// Candidates tried before this one, for `doctor`.
    pub rejected: Vec<Rejected>,
}

/// A candidate binary that failed validation, and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rejected {
    pub path: PathBuf,
    pub source: ServerSource,
    pub reason: String,
}

/// No step of the lookup produced a valid binary.
#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub struct NoServer {
    pub rejected: Vec<Rejected>,
    /// Why step 5 did not help: downloads disabled, no asset for this target, or the error.
    pub download: String,
}

impl fmt::Display for NoServer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "no usable server: {}", self.download)?;
        for rejected in &self.rejected {
            write!(
                f,
                "; rejected {} ({}): {}",
                rejected.path.display(),
                rejected.source,
                rejected.reason
            )?;
        }
        Ok(())
    }
}

/// Finds and validates language servers in the order config path, toolchain, `PATH`, cache,
/// download.
#[derive(Clone, Debug)]
pub struct Provisioner {
    servers: BTreeMap<Language, ServerConfig>,
    home: PathBuf,
    allow_download: bool,
    search_path: Option<OsString>,
    target: Option<(Os, Arch)>,
}

impl Provisioner {
    /// `home` is the codemycel home holding `servers/`. Downloads happen only when
    /// `allow_download` is set and the config does not say `no_download`. `PATH` is read now.
    pub fn new(config: &Config, home: PathBuf, allow_download: bool) -> Self {
        Self {
            servers: config.servers.clone(),
            home,
            allow_download: allow_download && !config.no_download,
            search_path: std::env::var_os("PATH"),
            target: current_target(),
        }
    }

    /// Replaces the directories searched for servers and toolchain programs; `None` searches none.
    pub fn with_search_path(mut self, search_path: Option<OsString>) -> Self {
        self.search_path = search_path;
        self
    }

    /// The `PATH` servers and toolchain commands run with.
    pub fn search_path(&self) -> Option<&OsStr> {
        self.search_path.as_deref()
    }

    pub fn home(&self) -> &Path {
        &self.home
    }

    /// Whether the program a language's roots need (its toolchain prerequisite) is on the search
    /// path and starts. Whether it then works for a root, which with rustup depends on the
    /// toolchain the root's directory selects, is `toolchain_missing`'s question. True for a
    /// server without a toolchain.
    pub fn prerequisite_present(&self, adapter: &dyn ServerAdapter) -> bool {
        adapter.toolchain().is_none_or(|toolchain| {
            self.run_prerequisite(toolchain.prerequisite, &std::env::temp_dir())
                .is_some()
        })
    }

    /// What the toolchain prerequisite prints when it fails to run in `dir`, typically because
    /// the directory pins a toolchain that is not installed. `None` when it runs there, or when
    /// the language has no toolchain or the prerequisite is absent.
    pub fn toolchain_missing(&self, adapter: &dyn ServerAdapter, dir: &Path) -> Option<String> {
        let toolchain = adapter.toolchain()?;
        let output = self.run_prerequisite(toolchain.prerequisite, dir)?;
        if output.status.success() {
            return None;
        }
        let stderr = String::from_utf8_lossy(&output.stderr);
        let message = stderr
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty())
            .map(|line| line.trim_start_matches("error: ").to_owned());
        Some(message.unwrap_or_else(|| {
            format!(
                "{} --version fails in {}",
                toolchain.prerequisite,
                dir.display()
            )
        }))
    }

    /// `<prerequisite> --version` in `dir`, with `RUSTUP_AUTO_INSTALL=0` so that rustup 1.28.1 or
    /// later does not install the toolchain the directory pins. Older rustup ignores the variable:
    /// before 1.28.0 it installs the pinned toolchain. `None` when the program is absent or does not
    /// start.
    fn run_prerequisite(&self, prerequisite: &str, dir: &Path) -> Option<Output> {
        let program = lookup::find_on_path(prerequisite, self.search_path())?;
        let mut command = Command::new(program);
        command
            .arg("--version")
            .current_dir(dir)
            .env("RUSTUP_AUTO_INSTALL", "0")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        if let Some(path) = self.search_path() {
            command.env("PATH", path);
        }
        command.output().ok()
    }

    pub fn locate(&self, adapter: &dyn ServerAdapter) -> Result<Located, NoServer> {
        let mut rejected = Vec::new();
        let mut try_candidate = |path: PathBuf, source: ServerSource| match adapter.validate(&path)
        {
            Ok(version) => Some((path, version, source)),
            Err(reason) => {
                rejected.push(Rejected {
                    path,
                    source,
                    reason,
                });
                None
            }
        };
        let configured = self
            .servers
            .get(&adapter.language())
            .and_then(|server| server.path.clone());
        let found = configured
            .and_then(|path| try_candidate(path, ServerSource::Config))
            .or_else(|| {
                self.toolchain_binary(adapter)
                    .and_then(|path| try_candidate(path, ServerSource::Toolchain))
            })
            .or_else(|| {
                lookup::find_on_path(adapter.binary_name(), self.search_path())
                    .and_then(|path| try_candidate(path, ServerSource::Path))
            })
            .or_else(|| {
                let cached = cache::cached_binary(&self.home, adapter);
                cached
                    .is_file()
                    .then_some(cached)
                    .and_then(|path| try_candidate(path, ServerSource::Cache))
            });
        if let Some((path, version, source)) = found {
            return Ok(Located {
                path,
                version,
                source,
                rejected,
            });
        }
        match self.download(adapter) {
            Ok(path) => match adapter.validate(&path) {
                Ok(version) => Ok(Located {
                    path,
                    version,
                    source: ServerSource::Download,
                    rejected,
                }),
                Err(reason) => Err(NoServer {
                    rejected,
                    download: format!("the downloaded binary failed validation: {reason}"),
                }),
            },
            Err(download) => Err(NoServer { rejected, download }),
        }
    }

    /// The toolchain's answer, installing the server through it once when the toolchain is
    /// present but the server is not.
    fn toolchain_binary(&self, adapter: &dyn ServerAdapter) -> Option<PathBuf> {
        let toolchain = adapter.toolchain()?;
        let program = toolchain.locate.first()?;
        lookup::find_on_path(program, self.search_path())?;
        let located = lookup::run_tool(toolchain.locate, self.search_path()).or_else(|| {
            lookup::run_tool(toolchain.install, self.search_path())?;
            lookup::run_tool(toolchain.locate, self.search_path())
        })?;
        Some(PathBuf::from(located))
    }

    fn download(&self, adapter: &dyn ServerAdapter) -> Result<PathBuf, String> {
        if !self.allow_download {
            return Err("downloads are disabled".to_owned());
        }
        let (os, arch) = self
            .target
            .ok_or_else(|| "no release asset for this platform".to_owned())?;
        let url = adapter
            .download_url(os, arch)
            .ok_or_else(|| "no release asset for this platform".to_owned())?;
        let sha256 = adapter
            .pinned_sha256(os, arch)
            .ok_or_else(|| "no pinned checksum for this platform".to_owned())?;
        download::download(
            &self.home,
            adapter,
            &url,
            sha256,
            adapter.archive_member(os),
        )
    }
}

fn current_target() -> Option<(Os, Arch)> {
    let os = match std::env::consts::OS {
        "linux" => Os::Linux,
        "macos" => Os::Macos,
        "windows" => Os::Windows,
        _ => return None,
    };
    let arch = match std::env::consts::ARCH {
        "x86_64" => Arch::X86_64,
        "aarch64" => Arch::Aarch64,
        _ => return None,
    };
    Some((os, arch))
}
