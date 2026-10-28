use std::collections::{BTreeMap, BTreeSet};
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
    #[serde(default)]
    dependencies: toml::Table,
    #[serde(default, rename = "dev-dependencies")]
    dev_dependencies: toml::Table,
    #[serde(default, rename = "build-dependencies")]
    build_dependencies: toml::Table,
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

/// Every `[workspace]` manifest, plus every `[package]` manifest that is not a member of any of
/// them. Members are the packages `members` lists and, transitively, the path dependencies of
/// members that lie inside the workspace directory, as Cargo counts them. Unreadable or malformed
/// manifests are not roots.
pub(super) fn detect(repo_root: &Path, files: &[RepoPath]) -> Vec<Root> {
    let mut workspaces = Vec::new();
    let mut packages: BTreeMap<RepoPath, Vec<RepoPath>> = BTreeMap::new();
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
        if manifest.package.is_some() {
            let dependencies = [
                &manifest.dependencies,
                &manifest.dev_dependencies,
                &manifest.build_dependencies,
            ]
            .into_iter()
            .flat_map(|table| table.values())
            .filter_map(|dependency| dependency.get("path")?.as_str())
            .filter_map(|relative| resolve(&dir, relative))
            .collect();
            packages.insert(dir.clone(), dependencies);
        }
        if let Some(table) = manifest.workspace {
            workspaces.push(Workspace {
                members: globs(&table.members, false),
                exclude: globs(&table.exclude, true),
                dir,
            });
        }
    }
    let mut members = BTreeSet::new();
    for workspace in &workspaces {
        members.extend(workspace.members_among(&packages));
    }
    let workspace_dirs = workspaces.iter().map(|ws| ws.dir.clone());
    let standalone = packages
        .into_keys()
        .filter(|dir| !members.contains(dir) && !workspaces.iter().any(|ws| ws.dir == *dir));
    let roots: BTreeSet<Root> = workspace_dirs
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

/// `relative` joined onto `dir`, or `None` when it is absolute or leaves the repository.
fn resolve(dir: &RepoPath, relative: &str) -> Option<RepoPath> {
    let relative = relative.replace('\\', "/");
    if relative.starts_with('/') || Path::new(&relative).is_absolute() {
        return None;
    }
    let mut segments: Vec<&str> = dir.as_str().split('/').filter(|s| !s.is_empty()).collect();
    for segment in relative.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop()?;
            }
            other => segments.push(other),
        }
    }
    RepoPath::new(&segments.join("/")).ok()
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
    /// The package directories that belong to this workspace: listed members, the root package,
    /// and every path dependency reachable from them that lies inside the workspace directory.
    fn members_among(&self, packages: &BTreeMap<RepoPath, Vec<RepoPath>>) -> BTreeSet<RepoPath> {
        let mut members: BTreeSet<RepoPath> = packages
            .keys()
            .filter(|dir| {
                **dir == self.dir
                    || self
                        .relative(dir)
                        .is_some_and(|rel| self.members.is_match(rel))
            })
            .filter(|dir| !self.excluded(dir))
            .cloned()
            .collect();
        let mut queue: Vec<RepoPath> = members.iter().cloned().collect();
        while let Some(dir) = queue.pop() {
            for dependency in packages.get(&dir).into_iter().flatten() {
                let inside = *dependency == self.dir || self.relative(dependency).is_some();
                if inside
                    && !self.excluded(dependency)
                    && packages.contains_key(dependency)
                    && members.insert(dependency.clone())
                {
                    queue.push(dependency.clone());
                }
            }
        }
        members
    }

    fn relative<'p>(&self, dir: &'p RepoPath) -> Option<&'p str> {
        if self.dir.is_root() {
            return Some(dir.as_str()).filter(|rel| !rel.is_empty());
        }
        dir.as_str()
            .strip_prefix(self.dir.as_str())
            .and_then(|rest| rest.strip_prefix('/'))
    }

    fn excluded(&self, dir: &RepoPath) -> bool {
        self.relative(dir)
            .is_some_and(|rel| self.exclude.is_match(rel))
    }
}
