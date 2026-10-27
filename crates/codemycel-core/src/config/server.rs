use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// One `[servers.<language>]` table (spec §7.4, §14), read from either config file.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServerConfig {
    /// Explicit server binary, the first step of the lookup order.
    pub path: Option<PathBuf>,
    /// rust-analyzer build scripts and proc-macro expansion; `None` means the default, on.
    pub build_scripts: Option<bool>,
}
