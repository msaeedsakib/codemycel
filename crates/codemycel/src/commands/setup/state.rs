use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::Context as _;
use serde::{Deserialize, Serialize};

/// What `install` created in a file it changed, so `remove` takes away only that.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct Created {
    pub file: bool,
    #[serde(default)]
    pub servers: bool,
}

/// `<codemycel home>/setup.json`: per harness, per file path, what `install` created.
pub struct State {
    path: PathBuf,
    harnesses: BTreeMap<String, BTreeMap<String, Created>>,
}

impl State {
    pub fn load() -> anyhow::Result<Self> {
        let path = codemycel_core::config::home_dir()?.join("setup.json");
        let harnesses = match std::fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text)
                .with_context(|| format!("parsing {}", path.display()))?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
            Err(error) => {
                return Err(error).with_context(|| format!("reading {}", path.display()));
            }
        };
        Ok(Self { path, harnesses })
    }

    pub fn record(&mut self, harness: &str, file: &Path, created: Created) {
        self.harnesses
            .entry(harness.to_owned())
            .or_default()
            .insert(file.display().to_string(), created);
    }

    /// The record for `file`, dropped from the state; nothing created when there is none.
    pub fn take(&mut self, harness: &str, file: &Path) -> Created {
        let Some(files) = self.harnesses.get_mut(harness) else {
            return Created::default();
        };
        let created = files.remove(&file.display().to_string());
        if files.is_empty() {
            self.harnesses.remove(harness);
        }
        created.unwrap_or_default()
    }

    /// Writes the state, or deletes its file when no record is left.
    pub fn save(&self) -> anyhow::Result<()> {
        if self.harnesses.is_empty() {
            return match std::fs::remove_file(&self.path) {
                Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                    Err(error).with_context(|| format!("removing {}", self.path.display()))
                }
                _ => Ok(()),
            };
        }
        let text = serde_json::to_string_pretty(&self.harnesses)? + "\n";
        super::write(&self.path, &text).with_context(|| format!("writing {}", self.path.display()))
    }
}
