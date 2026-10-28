use std::collections::BTreeMap;

use codemycel_core::{Edge, RepoPath, Root};

/// The verified source that owns a file's outgoing edges.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Owner<'a> {
    Scip,
    Lsp(&'a Root),
}

/// Exactly one resolver per file: SCIP when it covers the file, else its root's server.
#[derive(Debug, Default)]
pub struct Ownership<'a> {
    owners: BTreeMap<RepoPath, Owner<'a>>,
}

impl<'a> Ownership<'a> {
    pub fn assign(&mut self, file: RepoPath, owner: Owner<'a>) {
        self.owners.insert(file, owner);
    }

    /// Keeps the edges whose source node lives in a file `owner` owns; the rest belong to
    /// another resolver, which produces them itself.
    pub fn retain(&self, owner: Owner<'a>, edges: Vec<Edge>) -> Vec<Edge> {
        edges
            .into_iter()
            .filter(|edge| {
                edge.src
                    .path
                    .file()
                    .and_then(|file| RepoPath::new(file).ok())
                    .and_then(|file| self.owners.get(&file))
                    == Some(&owner)
            })
            .collect()
    }
}
