use std::collections::BTreeSet;
use std::path::Path;
use std::process::{Command, Stdio};

use codemycel_core::RepoPath;
use codemycel_core::config::{Config, ConfigWriter};

use crate::{ScipError, ScipIndex};

impl ScipIndex {
    /// The commit the index describes, recorded in `.codemycel/config.toml` once. A configured
    /// commit is kept while the index file is the one imported last (`previous_hash`), and on a
    /// first import without `override_sha`. Otherwise the commit is recorded: `override_sha`
    /// (the `--scip-commit` flag) when given, else git HEAD. The override is ignored while a
    /// configured commit is kept.
    pub fn commit_handling(
        &self,
        repo_root: &Path,
        config: &Config,
        previous_hash: Option<&str>,
        override_sha: Option<&str>,
    ) -> Result<String, ScipError> {
        let unchanged = previous_hash == Some(self.hash());
        let first_import = previous_hash.is_none();
        if let Some(commit) = &config.scip_commit
            && (unchanged || (first_import && override_sha.is_none()))
        {
            return Ok(commit.clone());
        }
        let commit = match override_sha {
            Some(sha) => sha.to_owned(),
            None => git(repo_root, &["rev-parse", "HEAD"])
                .and_then(|out| String::from_utf8(out).ok())
                .map(|out| out.trim().to_owned())
                .filter(|head| !head.is_empty())
                .ok_or(ScipError::NoHead)?,
        };
        ConfigWriter::set_scip_commit(repo_root, &commit)?;
        Ok(commit)
    }

    /// Marks the documents SCIP may answer for: the file is unchanged between `commit` and the
    /// working tree, and so is every file holding a definition its occurrences point to. Without
    /// git, or with an unknown commit, nothing is covered.
    pub fn compute_coverage(&mut self, repo_root: &Path, commit: &str) {
        let Some(changed) = changed_files(repo_root, commit) else {
            self.covered.clear();
            return;
        };
        let covered = self
            .documents
            .iter()
            .filter(|(path, document)| {
                !changed.contains(*path)
                    && document
                        .occurrences
                        .iter()
                        .filter(|o| !o.definition)
                        .filter_map(|o| self.definitions.get(&o.symbol))
                        .flatten()
                        .all(|(target, _)| !changed.contains(target))
            })
            .map(|(path, _)| path.clone())
            .collect();
        self.covered = covered;
    }

    pub fn covers(&self, file: &RepoPath) -> bool {
        self.covered.contains(file)
    }
}

/// Tracked files that differ from `commit` in the working tree, plus untracked files that are
/// not ignored.
fn changed_files(repo_root: &Path, commit: &str) -> Option<BTreeSet<RepoPath>> {
    let diff = git(
        repo_root,
        &["diff", "--name-only", "--relative", "-z", commit, "--"],
    )?;
    let untracked = git(
        repo_root,
        &["ls-files", "--others", "--exclude-standard", "-z"],
    )?;
    Some(
        diff.split(|&b| b == 0)
            .chain(untracked.split(|&b| b == 0))
            .filter(|name| !name.is_empty())
            .filter_map(|name| RepoPath::new(std::str::from_utf8(name).ok()?).ok())
            .collect(),
    )
}

fn git(repo_root: &Path, args: &[&str]) -> Option<Vec<u8>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    output.status.success().then_some(output.stdout)
}
