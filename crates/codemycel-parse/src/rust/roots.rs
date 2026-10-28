use std::collections::BTreeSet;
use std::path::Path;

use codemycel_core::{Language, RepoPath, Root};
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use serde::Deserialize;
use serde::de::IgnoredAny;

const MANIFEST: &str = "Cargo.toml";

#[derive(Deserialize)]
struct Manifest {
    package: Option<IgnoredAny>,
    workspace: Option<WorkspaceTable>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct WorkspaceTable {
    members: Vec<String>,
    exclude: Vec<String>,
}

struct Workspace {
    dir: RepoPath,
    members: GlobSet,
    exclude: GlobSet,
}

/// Every `[workspace]` manifest, plus every `[package]` manifest that no workspace lists among
/// its `members` (spec §8.2). Unreadable or malformed manifests are not roots.
pub(super) fn detect(repo_root: &Path, files: &[RepoPath]) -> Vec<Root> {
    let mut workspaces = Vec::new();
    let mut packages = Vec::new();
    for path in files {
        let Some(dir) = manifest_dir(path) else {
            continue;
        };
        let Some(manifest) = std::fs::read_to_string(repo_root.join(path.as_str()))
            .ok()
            .and_then(|text| toml::from_str::<Manifest>(&text).ok())
        else {
            continue;
        };
        match manifest.workspace {
            Some(table) => workspaces.push(Workspace {
                members: globs(&table.members, false),
                exclude: globs(&table.exclude, true),
                dir,
            }),
            None if manifest.package.is_some() => packages.push(dir),
            None => {}
        }
    }
    let standalone = packages
        .into_iter()
        .filter(|dir| !workspaces.iter().any(|ws| ws.has_member(dir)));
    let roots: BTreeSet<Root> = workspaces
        .iter()
        .map(|ws| ws.dir.clone())
        .chain(standalone)
        .map(|path| Root {
            path,
            language: Language::Rust,
        })
        .collect();
    roots.into_iter().collect()
}

fn manifest_dir(path: &RepoPath) -> Option<RepoPath> {
    let dir = match path.as_str().rsplit_once('/') {
        Some((dir, MANIFEST)) => dir,
        None if path.as_str() == MANIFEST => "",
        _ => return None,
    };
    RepoPath::new(dir).ok()
}

/// Patterns relative to the workspace directory; `*` stays within one path segment. An exclude
/// entry also covers everything below it, as Cargo treats excluded paths as prefixes.
fn globs(entries: &[String], with_descendants: bool) -> GlobSet {
    let mut set = GlobSetBuilder::new();
    for entry in entries {
        let Some(entry) = RepoPath::new(entry).ok().filter(|entry| !entry.is_root()) else {
            continue;
        };
        let descendants = format!("{entry}/**");
        let patterns = [
            Some(entry.as_str()),
            with_descendants.then_some(descendants.as_str()),
        ];
        for pattern in patterns.into_iter().flatten() {
            if let Ok(glob) = GlobBuilder::new(pattern).literal_separator(true).build() {
                set.add(glob);
            }
        }
    }
    set.build().unwrap_or_else(|_| GlobSet::empty())
}

impl Workspace {
    fn has_member(&self, dir: &RepoPath) -> bool {
        let relative = if self.dir.is_root() {
            Some(dir.as_str())
        } else {
            dir.as_str()
                .strip_prefix(self.dir.as_str())
                .and_then(|rest| rest.strip_prefix('/'))
        };
        relative.is_some_and(|rel| self.members.is_match(rel) && !self.exclude.is_match(rel))
    }
}
