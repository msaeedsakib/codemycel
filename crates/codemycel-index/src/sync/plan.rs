use std::collections::{BTreeMap, BTreeSet};

use codemycel_core::store::StoreRead;
use codemycel_core::{EdgeKind, NodeKind, NodeRef, RepoPath, SemanticPath};

use super::changes::ChangeSet;
use super::symbols_diff::ChangedSymbols;
use crate::IndexError;

const VERIFIED: [EdgeKind; 7] = [
    EdgeKind::Imports,
    EdgeKind::Exports,
    EdgeKind::Calls,
    EdgeKind::References,
    EdgeKind::Extends,
    EdgeKind::Implements,
    EdgeKind::Overrides,
];

/// What an incremental sync rewrites and re-verifies.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SyncPlan {
    /// Changed code files that still exist, parsed or not; their code rows are rewritten.
    pub rewrite: BTreeSet<RepoPath>,
    /// Parsed code files whose verified edges and unresolved rows are rebuilt.
    pub reverify: BTreeSet<RepoPath>,
    /// Files whose traits and interfaces are asked for implementers: `reverify`, plus the
    /// files of the traits and methods the re-verified files implement or override.
    pub implementer_sources: BTreeSet<RepoPath>,
}

impl SyncPlan {
    /// `symbols` holds every changed code file, parsed before or now. Re-verified, among the
    /// files `parsed` now: each changed file; every file with a verified edge to or from a
    /// removed or changed symbol, or to or from the file itself once any of its symbols changed;
    /// every file with an unresolved candidate named like an added symbol; for a trait impl
    /// added or removed, every file declaring a type or trait of those names; and every file
    /// whose SCIP coverage flipped. The index is read before anything is written.
    pub fn build(
        changes: &ChangeSet,
        symbols: &BTreeMap<RepoPath, ChangedSymbols>,
        coverage_flips: &BTreeSet<RepoPath>,
        parsed: &BTreeSet<RepoPath>,
        store: &impl StoreRead,
    ) -> Result<Self, IndexError> {
        let mut reverify: BTreeSet<RepoPath> = coverage_flips.clone();
        for (file, changed) in symbols {
            reverify.insert(file.clone());
            let mut touched: Vec<NodeRef> = changed
                .removed
                .iter()
                .chain(&changed.changed)
                .cloned()
                .collect();
            if !changed.is_empty() || !parsed.contains(file) {
                touched.push(NodeRef::new(SemanticPath::for_file(file)?, NodeKind::File));
            }
            for node in &touched {
                reverify.extend(linked_files(store, node, &VERIFIED)?);
            }
            for node in &changed.added {
                reverify.extend(store.files_with_unresolved_text(&node.name)?);
            }
            for found in changed.impls_added.iter().chain(&changed.impls_removed) {
                reverify.extend(declaring_files(store, &found.self_type, is_implementer)?);
                reverify.extend(declaring_files(store, &found.trait_name, is_parent)?);
            }
        }
        reverify.retain(|file| parsed.contains(file));
        let mut implementer_sources = reverify.clone();
        for file in &reverify {
            for node in store.nodes_in_file(file)? {
                let parents =
                    store.edges_from(node.id, &[EdgeKind::Implements, EdgeKind::Overrides])?;
                implementer_sources.extend(parents.iter().filter_map(|e| file_of(&e.edge.dst)));
            }
        }
        let rewrite = symbols
            .keys()
            .filter(|file| !changes.deleted.contains(*file))
            .cloned()
            .collect();
        Ok(Self {
            rewrite,
            reverify,
            implementer_sources,
        })
    }
}

/// The files at the other end of `node`'s stored edges of `kinds`, in either direction.
fn linked_files(
    store: &impl StoreRead,
    node: &NodeRef,
    kinds: &[EdgeKind],
) -> Result<BTreeSet<RepoPath>, IndexError> {
    let mut files = BTreeSet::new();
    for stored in store.node_by_semantic_path(&node.path, Some(node.kind))? {
        for edge in store.edges_to(stored.id, kinds)? {
            files.extend(file_of(&edge.edge.src));
        }
        for edge in store.edges_from(stored.id, kinds)? {
            files.extend(file_of(&edge.edge.dst));
        }
    }
    Ok(files)
}

/// Files declaring a node of a matching kind named like the last path segment of `text`, with
/// generic arguments dropped: `crate::a::Foo<T>` looks for `Foo`.
fn declaring_files(
    store: &impl StoreRead,
    text: &str,
    kind: fn(NodeKind) -> bool,
) -> Result<BTreeSet<RepoPath>, IndexError> {
    let base = text.split('<').next().unwrap_or(text);
    let name = base.rsplit("::").next().unwrap_or(base).trim();
    let name = name.rsplit(' ').next().unwrap_or(name);
    Ok(store
        .nodes_named(name)?
        .iter()
        .filter(|stored| kind(stored.node.kind))
        .filter_map(|stored| stored.node.file.clone())
        .collect())
}

fn file_of(node: &NodeRef) -> Option<RepoPath> {
    RepoPath::new(node.path.file()?).ok()
}

fn is_implementer(kind: NodeKind) -> bool {
    matches!(
        kind,
        NodeKind::Struct | NodeKind::Enum | NodeKind::Class | NodeKind::TypeAlias
    )
}

fn is_parent(kind: NodeKind) -> bool {
    matches!(kind, NodeKind::Trait | NodeKind::Interface)
}
