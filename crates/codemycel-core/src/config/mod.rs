mod home_config;
pub mod paths;
mod repo_config;
mod server;

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use thiserror::Error;

pub use home_config::HomeConfig;
pub use paths::home_dir;
pub use repo_config::{ByteSize, IndexConfig, RepoConfig, RootOverride, ScipConfig};
pub use server::ServerConfig;

use crate::model::Language;

/// The commented `.codemycel/config.toml` that `init` writes (spec §14). It parses to the defaults.
pub const TEMPLATE: &str = r#"# codemycel repository configuration. Every key is commented out; uncomment one to override its default.

[index]
# ignore = ["fixtures/**"]           # extra ignore globs, gitignore syntax
# max_file_size = "2MiB"             # larger files are listed, not parsed; B, KiB, MiB, or GiB

# [[root]]                           # add, re-type, or disable a detected root; repeat per root
# path = "apps/web"
# language = "typescript"            # rust, typescript, or javascript
# disabled = true

[scip]
# path = "index.scip"
# commit = "<sha>"                   # recorded by codemycel at first import; edit to override

# [servers.typescript]
# path = "/custom/path/tsc"

# [servers.rust]
# path = "/custom/path/rust-analyzer"
# build_scripts = true               # also controls proc-macro expansion; set false for untrusted repos
"#;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("cannot read {}: {source}", path.display())]
    Read { path: PathBuf, source: io::Error },
    #[error("invalid configuration in {}: {source}", path.display())]
    Parse {
        path: PathBuf,
        source: toml::de::Error,
    },
    #[error("cannot locate the user home directory; set CODEMYCEL_HOME")]
    NoHomeDir,
}

/// The repository and home configuration merged, with every optional key defaulted.
/// A key set in both files takes the repository's value, field by field within a server table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    pub ignore: Vec<String>,
    pub max_file_size: u64,
    pub roots: Vec<RootOverride>,
    /// Relative paths are relative to the repository root.
    pub scip_path: PathBuf,
    pub scip_commit: Option<String>,
    pub servers: BTreeMap<Language, ServerConfig>,
    pub no_download: bool,
}

impl Config {
    pub const DEFAULT_MAX_FILE_SIZE: u64 = 2 << 20;
    pub const DEFAULT_SCIP_PATH: &str = "index.scip";

    /// Loads `<repo_root>/.codemycel/config.toml` and the home config; either may be absent.
    pub fn load(repo_root: &Path) -> Result<Self, ConfigError> {
        Self::load_with_home(repo_root, &home_dir()?)
    }

    pub fn load_with_home(repo_root: &Path, home: &Path) -> Result<Self, ConfigError> {
        let repo = read_optional(&paths::repo_config_path(repo_root))?;
        let home = read_optional(&paths::home_config_path(home))?;
        Ok(Self::merge(repo, home))
    }

    pub fn merge(repo: RepoConfig, home: HomeConfig) -> Self {
        let mut servers = home.servers;
        for (language, repo_server) in repo.servers {
            let merged = servers.entry(language).or_default();
            merged.path = repo_server.path.or(merged.path.take());
            merged.build_scripts = repo_server.build_scripts.or(merged.build_scripts);
        }
        Self {
            ignore: repo.index.ignore,
            max_file_size: repo
                .index
                .max_file_size
                .map_or(Self::DEFAULT_MAX_FILE_SIZE, |size| size.0),
            roots: repo.roots,
            scip_path: repo
                .scip
                .path
                .unwrap_or_else(|| PathBuf::from(Self::DEFAULT_SCIP_PATH)),
            scip_commit: repo.scip.commit,
            servers,
            no_download: home.no_download,
        }
    }

    pub fn server(&self, language: Language) -> ServerConfig {
        self.servers.get(&language).cloned().unwrap_or_default()
    }

    /// rust-analyzer build scripts and proc-macro expansion; on unless a config turns it off (spec §7.5).
    pub fn build_scripts(&self, language: Language) -> bool {
        self.servers
            .get(&language)
            .and_then(|server| server.build_scripts)
            .unwrap_or(true)
    }
}

impl Default for Config {
    fn default() -> Self {
        Self::merge(RepoConfig::default(), HomeConfig::default())
    }
}

fn read_optional<T: DeserializeOwned + Default>(path: &Path) -> Result<T, ConfigError> {
    match std::fs::read_to_string(path) {
        Ok(text) => toml::from_str(&text).map_err(|source| ConfigError::Parse {
            path: path.to_owned(),
            source,
        }),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(T::default()),
        Err(source) => Err(ConfigError::Read {
            path: path.to_owned(),
            source,
        }),
    }
}
