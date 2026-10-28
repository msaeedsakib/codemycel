use std::collections::{HashMap, HashSet};
use std::ffi::OsStr;
use std::path::{Component, Path, PathBuf};

use ignore::gitignore::{Gitignore, GitignoreBuilder};

use crate::IndexError;
use crate::walk::{ALWAYS_IGNORED, config_matcher, tracked_ignored};

/// The walk's ignore rules, asked about one path at a time: always-ignored directories, the
/// config globs, and every `.gitignore` inside the repository with `.git/info/exclude`, except
/// for files git tracks although a rule matches them.
pub(super) struct Rules {
    root: PathBuf,
    canonical: Option<PathBuf>,
    config: Gitignore,
    tracked: HashSet<PathBuf>,
    gitignores: HashMap<PathBuf, Gitignore>,
}

impl Rules {
    pub(super) fn new(root: &Path, ignore: &[String]) -> Result<Self, IndexError> {
        let config = config_matcher(root, ignore)?;
        let tracked = tracked_ignored(root, &config).into_iter().collect();
        Ok(Self {
            root: root.to_owned(),
            canonical: root.canonicalize().ok(),
            config,
            tracked,
            gitignores: HashMap::new(),
        })
    }

    /// `path` relative to the repository root, which some backends report canonicalized.
    /// `None` for the root itself and for paths outside it.
    pub(super) fn relative(&self, path: &Path) -> Option<PathBuf> {
        let relative = path
            .strip_prefix(&self.root)
            .ok()
            .or_else(|| path.strip_prefix(self.canonical.as_ref()?).ok())?;
        (!relative.as_os_str().is_empty()).then(|| relative.to_owned())
    }

    pub(super) fn absolute(&self, relative: &Path) -> PathBuf {
        self.root.join(relative)
    }

    /// Drops the cached rules an edited ignore file at `relative` feeds.
    pub(super) fn forget_if_ignore_file(&mut self, relative: &Path) {
        if relative.file_name() == Some(OsStr::new(".gitignore")) {
            self.gitignores
                .remove(relative.parent().unwrap_or(Path::new("")));
        } else if relative == Path::new(".git/info/exclude") {
            self.gitignores.remove(Path::new(""));
        }
    }

    pub(super) fn ignored(&mut self, relative: &Path) -> bool {
        let parts: Vec<&OsStr> = relative
            .components()
            .filter_map(|component| match component {
                Component::Normal(part) => Some(part),
                _ => None,
            })
            .collect();
        let Some((last, parents)) = parts.split_last() else {
            return true;
        };
        if parents.iter().any(|part| always_ignored(part)) {
            return true;
        }
        let absolute = self.absolute(relative);
        if always_ignored(last) && !absolute.is_file() {
            return true;
        }
        let is_dir = absolute.is_dir();
        if self
            .config
            .matched_path_or_any_parents(relative, is_dir)
            .is_ignore()
        {
            return true;
        }
        if self.tracked.contains(relative) {
            return false;
        }
        let mut prefix = PathBuf::new();
        for (depth, part) in parts.iter().enumerate() {
            prefix.push(part);
            let prefix_is_dir = depth + 1 < parts.len() || is_dir;
            if self.gitignored(&prefix, prefix_is_dir) {
                return true;
            }
        }
        false
    }

    /// The deepest ignore file with a rule for `path` decides.
    fn gitignored(&mut self, path: &Path, is_dir: bool) -> bool {
        let mut dir = path.parent();
        while let Some(current) = dir {
            let below = path.strip_prefix(current).unwrap_or(path);
            let matched = self.matcher(current).matched(below, is_dir);
            if matched.is_ignore() {
                return true;
            }
            if matched.is_whitelist() {
                return false;
            }
            dir = current.parent();
        }
        false
    }

    fn matcher(&mut self, dir: &Path) -> &Gitignore {
        let root = &self.root;
        self.gitignores.entry(dir.to_owned()).or_insert_with(|| {
            let absolute = root.join(dir);
            let mut builder = GitignoreBuilder::new(&absolute);
            if dir.as_os_str().is_empty() {
                let _ = builder.add(absolute.join(".git").join("info").join("exclude"));
            }
            let _ = builder.add(absolute.join(".gitignore"));
            builder.build().unwrap_or_else(|error| {
                tracing::warn!("ignore rules in {}: {error}", absolute.display());
                Gitignore::empty()
            })
        })
    }
}

fn always_ignored(part: &OsStr) -> bool {
    ALWAYS_IGNORED.iter().any(|name| part == *name)
}
