use std::path::Path;

use ignore::gitignore::{Gitignore, GitignoreBuilder};
use ignore::{Walk, WalkBuilder};

/// Directories never walked, wherever they appear.
pub const ALWAYS_IGNORED: [&str; 8] = [
    ".git",
    ".codemycel",
    "node_modules",
    "target",
    "dist",
    "build",
    ".next",
    "coverage",
];

/// The config `ignore` globs, in gitignore syntax relative to the repository root.
pub fn config_matcher(repo_root: &Path, globs: &[String]) -> Result<Gitignore, ignore::Error> {
    let mut builder = GitignoreBuilder::new(repo_root);
    for glob in globs {
        builder.add_line(None, glob)?;
    }
    builder.build()
}

/// `.gitignore` files inside the repository (nested ones included) and `.git/info/exclude`
/// apply, with or without a `.git` directory. Ignore files above the repository root, the user's
/// global excludes, and `.ignore` files do not, so the walk depends only on the repository's own
/// content. Symlinks are not followed.
pub fn walker(repo_root: &Path, config_ignore: Gitignore) -> Walk {
    let root = repo_root.to_owned();
    WalkBuilder::new(repo_root)
        .hidden(false)
        .parents(false)
        .ignore(false)
        .git_global(false)
        .git_ignore(true)
        .git_exclude(true)
        .require_git(false)
        .follow_links(false)
        .filter_entry(move |entry| {
            let is_dir = entry.file_type().is_some_and(|t| t.is_dir());
            if is_dir && entry.depth() > 0 {
                let name = entry.file_name().to_string_lossy();
                if ALWAYS_IGNORED.contains(&name.as_ref()) {
                    return false;
                }
            }
            let relative = entry.path().strip_prefix(&root).unwrap_or(entry.path());
            relative.as_os_str().is_empty()
                || !config_ignore
                    .matched_path_or_any_parents(relative, is_dir)
                    .is_ignore()
        })
        .build()
}
