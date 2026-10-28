use std::path::{Component, Path, PathBuf};

use codemycel_core::config::paths::{CONFIG_FILE_NAME, REPO_DIR_NAME};

pub const INDEX_FILE_NAME: &str = "index.db";

/// The repository a command operates on. `indexed` is set when it was found by its
/// `.codemycel/index.db` or `.codemycel/config.toml`, not by `.git` or as the fallback.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepoLocation {
    pub root: PathBuf,
    pub indexed: bool,
}

impl RepoLocation {
    pub fn index_path(&self) -> PathBuf {
        self.root.join(REPO_DIR_NAME).join(INDEX_FILE_NAME)
    }
}

pub struct RepoLocator;

impl RepoLocator {
    /// Walks up from `start` (absolute) below `user_home`: the nearest ancestor whose
    /// `.codemycel/` holds `index.db` or `config.toml`, never `codemycel_home` itself; else the
    /// nearest holding `.git`; else `start`. `..` segments are resolved lexically first.
    pub fn locate(
        start: &Path,
        user_home: Option<&Path>,
        codemycel_home: Option<&Path>,
    ) -> RepoLocation {
        let start = without_parent_segments(start);
        let ancestors: Vec<&Path> = start
            .ancestors()
            .take_while(|dir| Some(*dir) != user_home)
            .collect();
        let indexed = ancestors.iter().find(|dir| {
            let marker = dir.join(REPO_DIR_NAME);
            Some(marker.as_path()) != codemycel_home
                && (marker.join(INDEX_FILE_NAME).is_file()
                    || marker.join(CONFIG_FILE_NAME).is_file())
        });
        if let Some(dir) = indexed {
            return RepoLocation {
                root: dir.to_path_buf(),
                indexed: true,
            };
        }
        let git = ancestors.iter().find(|dir| dir.join(".git").exists());
        RepoLocation {
            root: git.map_or_else(|| start.clone(), |dir| dir.to_path_buf()),
            indexed: false,
        }
    }
}

fn without_parent_segments(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}
