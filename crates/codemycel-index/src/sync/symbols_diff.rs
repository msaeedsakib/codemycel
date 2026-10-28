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
    /// Kept, but with a different signature, or a function or method without an explicit
    /// return type whose body changed, since its inferred type may have.
    pub changed: Vec<NodeRef>,
    pub impls_added: Vec<TraitImpl>,
    pub impls_removed: Vec<TraitImpl>,
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
    /// not symbols and are ignored on both sides. Either side is empty for an added or deleted
    /// file.
    pub fn compute(
        old_nodes: &[Node],
        old_impls: &[ImplRecord],
        new_nodes: &[Node],
        new_impls: &[ImplRecord],
    ) -> ChangedSymbols {
        let symbols = |nodes: &[Node]| -> BTreeMap<NodeRef, Node> {
            nodes
                .iter()
                .filter(|n| n.kind != NodeKind::File)
                .map(|n| (n.node_ref(), n.clone()))
                .collect()
        };
        let mut old = symbols(old_nodes);
        let mut diff = ChangedSymbols::default();
        for (identity, node) in symbols(new_nodes) {
            match old.remove(&identity) {
                None => diff.added.push(node),
                Some(before) => {
                    let signature = before.signature != node.signature
                        || before.visibility != node.visibility
                        || before.has_explicit_return_type != node.has_explicit_return_type;
                    let inferred = matches!(node.kind, NodeKind::Function | NodeKind::Method)
                        && !node.has_explicit_return_type
                        && before.body_hash != node.body_hash;
                    if signature || inferred {
                        diff.changed.push(identity);
                    }
                }
            }
        }
        diff.removed = old.into_keys().collect();
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
