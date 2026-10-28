use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;
use std::str::FromStr;

use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer};

use super::ServerConfig;
use crate::model::{Language, RepoPath};

/// `.codemycel/config.toml` as written. Absent keys stay absent; `Config` applies defaults.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RepoConfig {
    #[serde(default)]
    pub index: IndexConfig,
    #[serde(default, rename = "root")]
    pub roots: Vec<RootOverride>,
    #[serde(default)]
    pub scip: ScipConfig,
    #[serde(default)]
    pub servers: BTreeMap<Language, ServerConfig>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IndexConfig {
    /// Extra ignore globs in gitignore syntax.
    #[serde(default)]
    pub ignore: Vec<String>,
    pub max_file_size: Option<ByteSize>,
}

/// A `[[root]]` entry that adds, re-types, or disables a detected root.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RootOverride {
    pub path: RepoPath,
    pub language: Language,
    #[serde(default)]
    pub disabled: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScipConfig {
    pub path: Option<PathBuf>,
    /// The commit the SCIP file was generated at.
    pub commit: Option<String>,
}

/// A size in bytes, written as a bare integer or as a string with a `B`, `KiB`, `MiB`, or `GiB` unit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ByteSize(pub u64);

impl FromStr for ByteSize {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let invalid =
            || format!("invalid size `{text}`: expected a number with B, KiB, MiB, or GiB");
        let trimmed = text.trim();
        let split = trimmed
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(trimmed.len());
        let (digits, unit) = trimmed.split_at(split);
        let multiplier: u64 = match unit.trim_start() {
            "" | "B" => 1,
            "KiB" => 1 << 10,
            "MiB" => 1 << 20,
            "GiB" => 1 << 30,
            _ => return Err(invalid()),
        };
        digits
            .parse::<u64>()
            .ok()
            .and_then(|count| count.checked_mul(multiplier))
            .map(ByteSize)
            .ok_or_else(invalid)
    }
}

impl<'de> Deserialize<'de> for ByteSize {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct ByteSizeVisitor;

        impl Visitor<'_> for ByteSizeVisitor {
            type Value = ByteSize;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a byte count or a size string such as \"2MiB\"")
            }

            fn visit_u64<E: de::Error>(self, value: u64) -> Result<ByteSize, E> {
                Ok(ByteSize(value))
            }

            fn visit_i64<E: de::Error>(self, value: i64) -> Result<ByteSize, E> {
                u64::try_from(value)
                    .map(ByteSize)
                    .map_err(|_| E::custom(format!("invalid size `{value}`: must not be negative")))
            }

            fn visit_str<E: de::Error>(self, value: &str) -> Result<ByteSize, E> {
                value.parse().map_err(E::custom)
            }
        }

        deserializer.deserialize_any(ByteSizeVisitor)
    }
}
