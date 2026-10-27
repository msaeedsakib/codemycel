use std::ffi::OsString;
use std::path::{Path, PathBuf};

use super::ConfigError;

pub const HOME_ENV: &str = "CODEMYCEL_HOME";
pub const REPO_DIR_NAME: &str = ".codemycel";
pub const CONFIG_FILE_NAME: &str = "config.toml";

/// The codemycel home: `CODEMYCEL_HOME` if set, else `<user home>/.codemycel` (spec §13.5).
pub fn home_dir() -> Result<PathBuf, ConfigError> {
    home_dir_with(|name| std::env::var_os(name), std::env::home_dir())
}

/// `home_dir` over an explicit environment lookup and user home, so tests need not mutate the process environment.
pub fn home_dir_with(
    env: impl Fn(&str) -> Option<OsString>,
    user_home: Option<PathBuf>,
) -> Result<PathBuf, ConfigError> {
    match env(HOME_ENV).filter(|value| !value.is_empty()) {
        Some(home) => Ok(PathBuf::from(home)),
        None => user_home
            .map(|home| home.join(REPO_DIR_NAME))
            .ok_or(ConfigError::NoHomeDir),
    }
}

pub fn repo_config_path(repo_root: &Path) -> PathBuf {
    repo_root.join(REPO_DIR_NAME).join(CONFIG_FILE_NAME)
}

pub fn home_config_path(home: &Path) -> PathBuf {
    home.join(CONFIG_FILE_NAME)
}
