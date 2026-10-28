//! `Cargo.toml` and `package.json` packages.

mod cargo;
mod npm;

use codemycel_core::{AdapterError, RepoPath};

pub use cargo::parse_cargo;
pub use npm::parse_npm;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Ecosystem {
    Cargo,
    Npm,
}

impl Ecosystem {
    /// The ecosystem segment of an external package's semantic path (`package:cargo:uuid`).
    pub fn as_str(self) -> &'static str {
        match self {
            Ecosystem::Cargo => "cargo",
            Ecosystem::Npm => "npm",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum DependencyKind {
    Normal,
    Dev,
    Build,
    Peer,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dependency {
    /// The package depended on: Cargo's `package = "…"` rename target when present, else the key.
    pub name: String,
    pub kind: DependencyKind,
    /// An npm `workspace:` version, which always names a workspace package.
    pub workspace_protocol: bool,
}

/// One manifest. `name` is `None` for a manifest that declares no package (a virtual Cargo
/// workspace, a nameless `package.json`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackageParse {
    pub ecosystem: Ecosystem,
    pub manifest: RepoPath,
    pub directory: RepoPath,
    pub name: Option<String>,
    pub version: Option<String>,
    pub dependencies: Vec<Dependency>,
    /// Workspace member globs, relative to `directory`.
    pub workspace_members: Vec<String>,
    pub workspace_exclude: Vec<String>,
}

impl PackageParse {
    /// Whether `dir` is one of this workspace's members: matched by a member glob and by no
    /// exclude glob, both relative to this manifest's directory, `*` staying within a segment.
    pub fn has_member(&self, dir: &RepoPath) -> bool {
        let relative = if self.directory.is_root() {
            Some(dir.as_str())
        } else {
            dir.as_str()
                .strip_prefix(self.directory.as_str())
                .and_then(|rest| rest.strip_prefix('/'))
        };
        let Some(relative) = relative else {
            return false;
        };
        let matches = |globs: &[String]| globs.iter().any(|glob| glob_matches(glob, relative));
        matches(&self.workspace_members) && !matches(&self.workspace_exclude)
    }
}

fn glob_matches(glob: &str, path: &str) -> bool {
    let glob = glob.trim_start_matches("./").trim_end_matches('/');
    globset::GlobBuilder::new(glob)
        .literal_separator(true)
        .build()
        .is_ok_and(|glob| glob.compile_matcher().is_match(path))
}

fn directory(manifest: &RepoPath) -> Result<RepoPath, AdapterError> {
    let parent = manifest
        .as_str()
        .rsplit_once('/')
        .map_or("", |(dir, _)| dir);
    RepoPath::new(parent).map_err(|error| AdapterError::Parse {
        path: manifest.clone(),
        message: error.to_string(),
    })
}

fn malformed(manifest: &RepoPath, message: impl ToString) -> AdapterError {
    AdapterError::Parse {
        path: manifest.clone(),
        message: message.to_string(),
    }
}
