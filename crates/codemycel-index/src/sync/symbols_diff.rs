use std::collections::BTreeMap;

use codemycel_core::{ImplRecord, Node, NodeKind, NodeRef};

/// A trait impl as its record spells it: the self type and the trait.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct TraitImpl {
    pub self_type: String,
    pub trait_name: String,
}

/// How one file's symbols changed between the index and a new parse.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ChangedSymbols {
    pub added: Vec<Node>,
    pub removed: Vec<NodeRef>,
    /// Kept, but with a different signature or visibility, a function or method without an
    /// explicit return type whose body changed, since its inferred type may have, or a trait or
    /// interface one of whose members was added, removed, or changed.
    pub changed: Vec<NodeRef>,
    pub impls_added: Vec<TraitImpl>,
    pub impls_removed: Vec<TraitImpl>,
    /// The file's import and export statements changed, as its file node's body hash says.
    pub surface: bool,
}

impl ChangedSymbols {
    /// No symbol was added, removed, or changed; impl records are not symbols.
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty() && self.changed.is_empty()
    }
}

pub struct SymbolsDiff;

impl SymbolsDiff {
    /// Diffs a file's stored symbols and impl records against its new parse. File nodes are
    /// not symbols; only their body hashes are compared, a missing one counting as `None`.
    /// Either side is empty for an added or deleted file.
    pub fn compute(
        old_nodes: &[Node],
        old_impls: &[ImplRecord],
        new_nodes: &[Node],
        new_impls: &[ImplRecord],
    ) -> ChangedSymbols {
        let surface = |nodes: &[Node]| {
            nodes
                .iter()
                .find(|n| n.kind == NodeKind::File)
                .and_then(|n| n.body_hash)
        };
        let symbols = |nodes: &[Node]| -> BTreeMap<NodeRef, Node> {
            nodes
                .iter()
                .filter(|n| n.kind != NodeKind::File)
                .map(|n| (n.node_ref(), n.clone()))
                .collect()
        };
        let mut old = symbols(old_nodes);
        let new = symbols(new_nodes);
        let mut diff = ChangedSymbols {
            surface: surface(old_nodes) != surface(new_nodes),
            ..ChangedSymbols::default()
        };
        for (identity, node) in &new {
            match old.remove(identity) {
                None => diff.added.push(node.clone()),
                Some(before) => {
                    let signature = before.signature != node.signature
                        || before.visibility != node.visibility
                        || before.has_explicit_return_type != node.has_explicit_return_type;
                    let inferred = matches!(node.kind, NodeKind::Function | NodeKind::Method)
                        && !node.has_explicit_return_type
                        && before.body_hash != node.body_hash;
                    if signature || inferred {
                        diff.changed.push(identity.clone());
                    }
                }
            }
        }
        diff.removed = old.into_keys().collect();
        let members: Vec<NodeRef> = diff
            .added
            .iter()
            .map(Node::node_ref)
            .chain(diff.removed.iter().cloned())
            .chain(diff.changed.iter().cloned())
            .collect();
        for member in &members {
            let Some(parent) = new.keys().find(|parent| is_parent_of(parent, member)) else {
                continue;
            };
            let added = diff.added.iter().any(|node| node.node_ref() == *parent);
            if !added && !diff.changed.contains(parent) {
                diff.changed.push(parent.clone());
            }
        }
        let mut old_impls = trait_impls(old_impls);
        for found in trait_impls(new_impls) {
            match old_impls.iter().position(|old| *old == found) {
                Some(index) => {
                    old_impls.remove(index);
                }
                None => diff.impls_added.push(found),
            }
        }
        diff.impls_removed = old_impls;
        diff
    }
}

/// Whether `parent` is a trait or interface that declares `member` directly.
fn is_parent_of(parent: &NodeRef, member: &NodeRef) -> bool {
    if !matches!(parent.kind, NodeKind::Trait | NodeKind::Interface) {
        return false;
    }
    let containers = member.path.containers();
    let Some((name, outer)) = containers.split_last() else {
        return false;
    };
    parent.path.file() == member.path.file()
        && parent.path.name() == *name
        && parent.path.containers() == outer
}

fn trait_impls(records: &[ImplRecord]) -> Vec<TraitImpl> {
    let mut impls: Vec<TraitImpl> = records
        .iter()
        .filter_map(|record| {
            Some(TraitImpl {
                self_type: record.self_type_text.clone(),
                trait_name: record.trait_text.clone()?,
            })
        })
        .collect();
    impls.sort();
    impls
}
