mod hooks;
mod roots;

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use codemycel_core::{AdapterError, Language, LanguageAdapter, ParsedFile, RepoPath, Root};

use crate::driver::{Driver, QuerySet};

const QUERIES: QuerySet = QuerySet {
    symbols: include_str!("queries/symbols.scm"),
    candidates: include_str!("queries/candidates.scm"),
    locals: include_str!("queries/locals.scm"),
};

/// The syntactic half of Rust. The driver is built on first parse; a query that fails to compile
/// surfaces as `AdapterError::Parse`.
#[derive(Default)]
pub struct RustAdapter {
    driver: OnceLock<Result<Driver, String>>,
}

impl RustAdapter {
    pub fn new() -> Self {
        Self::default()
    }

    fn driver(&self) -> Result<&Driver, &str> {
        self.driver
            .get_or_init(|| {
                Driver::new(
                    tree_sitter_rust::LANGUAGE.into(),
                    QUERIES,
                    Box::new(hooks::RustHooks),
                )
                .map_err(|error| error.to_string())
            })
            .as_ref()
            .map_err(String::as_str)
    }
}

impl LanguageAdapter for RustAdapter {
    fn language(&self) -> Language {
        Language::Rust
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["rs"]
    }

    fn detect_roots(
        &self,
        repo_root: &Path,
        files: &[RepoPath],
    ) -> Result<Vec<Root>, AdapterError> {
        Ok(roots::detect(repo_root, files))
    }

    fn parse(&self, file: &RepoPath, bytes: &[u8]) -> Result<ParsedFile, AdapterError> {
        let driver = self.driver().map_err(|message| AdapterError::Parse {
            path: file.clone(),
            message: message.to_owned(),
        })?;
        driver.parse(file, bytes)
    }

    /// Under `CARGO_HOME` (default `~/.cargo`), `RUSTUP_HOME` (default `~/.rustup`), or a `target`
    /// directory.
    fn external_path_predicate(&self, canonical_path: &Path, repo_root: &Path) -> bool {
        let home = std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .filter(|home| !home.is_empty());
        let toolchain_dir = |var: &str, default: &str| {
            std::env::var_os(var)
                .filter(|dir| !dir.is_empty())
                .map(PathBuf::from)
                .or_else(|| home.as_ref().map(|home| Path::new(home).join(default)))
        };
        let homes = [
            toolchain_dir("CARGO_HOME", ".cargo"),
            toolchain_dir("RUSTUP_HOME", ".rustup"),
        ];
        is_external(
            canonical_path,
            repo_root,
            &with_canonical(homes.into_iter().flatten()),
        )
    }

    /// Cargo manifests, toolchain files, Cargo configs, and build scripts. `Cargo.lock` is left
    /// out: rust-analyzer rewrites it itself whenever it loads a changed manifest.
    fn project_files(&self, _repo_root: &Path, files: &[RepoPath]) -> Vec<RepoPath> {
        const NAMES: [&str; 4] = [
            "Cargo.toml",
            "rust-toolchain",
            "rust-toolchain.toml",
            "build.rs",
        ];
        files
            .iter()
            .filter(|file| {
                let path = file.as_str();
                let (dir, name) = path.rsplit_once('/').unwrap_or(("", path));
                let cargo_config = (dir == ".cargo" || dir.ends_with("/.cargo"))
                    && matches!(name, "config" | "config.toml");
                NAMES.contains(&name) || cargo_config
            })
            .cloned()
            .collect()
    }
}

fn with_canonical(paths: impl Iterator<Item = PathBuf>) -> Vec<PathBuf> {
    paths
        .flat_map(|path| {
            let canonical = std::fs::canonicalize(&path).ok();
            [Some(path), canonical]
        })
        .flatten()
        .collect()
}

/// A `target` segment counts only below the repository root, so a repository that itself lives
/// under a `target` directory is not external.
fn is_external(path: &Path, repo_root: &Path, toolchain_homes: &[PathBuf]) -> bool {
    if toolchain_homes.iter().any(|home| path.starts_with(home)) {
        return true;
    }
    let inside = with_canonical(std::iter::once(repo_root.to_path_buf()))
        .into_iter()
        .find_map(|root| path.strip_prefix(root).ok().map(Path::to_path_buf))
        .unwrap_or_else(|| path.to_path_buf());
    inside
        .components()
        .any(|component| component.as_os_str() == "target")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toolchain_homes_and_target_directories_are_external() {
        let homes = [PathBuf::from("/opt/cargo"), PathBuf::from("/opt/rustup")];
        let repo = Path::new("/work/target/repo");
        let external = |path: &str| is_external(Path::new(path), repo, &homes);
        assert!(external("/opt/cargo/registry/src/serde/lib.rs"));
        assert!(external("/opt/rustup/toolchains/stable/lib/std.rs"));
        assert!(external("/work/target/repo/target/debug/build/out.rs"));
        assert!(external("/work/target/repo/crates/a/target/gen.rs"));
        assert!(!external("/work/target/repo/crates/a/src/lib.rs"));
        assert!(!external("/work/target/repo/crates/targets/lib.rs"));
    }
}
