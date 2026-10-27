use std::collections::BTreeMap;

use serde::Deserialize;

use super::ServerConfig;
use crate::model::Language;

/// `~/.codemycel/config.toml` as written (spec §13.5).
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HomeConfig {
    /// Never download language servers (spec §7.4 step 5).
    #[serde(default)]
    pub no_download: bool,
    #[serde(default)]
    pub servers: BTreeMap<Language, ServerConfig>,
}
