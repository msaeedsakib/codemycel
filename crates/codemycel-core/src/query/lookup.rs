use crate::store::{StoreError, StoreRead};
use crate::{NodeKind, SemanticPath, StoredNode};

pub enum Resolved {
    One(Box<StoredNode>),
    /// Several nodes match; the caller lists them and answers nothing else.
    Many(Vec<StoredNode>),
    None,
}

/// Turns a tool's name parameter into a node.
pub struct Lookup;

impl Lookup {
    /// `name` is a semantic path, optionally followed by ` (<kind>)` as answers print it, or a
    /// bare name. A path is tried first, then the name; both match exactly and case-sensitively.
    /// Bare names never match notes or communities, which are named after other things.
    pub fn resolve(store: &impl StoreRead, name: &str) -> Result<Resolved, StoreError> {
        let name = name.trim();
        let (name, kind) = split_kind(name);
        if let Ok(path) = name.parse::<SemanticPath>() {
            let by_path = store.node_by_semantic_path(&path, kind)?;
            if !by_path.is_empty() {
                return Ok(Self::pick(by_path));
            }
        }
        let by_name = store
            .nodes_named(name)?
            .into_iter()
            .filter(|stored| {
                !matches!(stored.node.kind, NodeKind::Note | NodeKind::Community)
                    && kind.is_none_or(|kind| stored.node.kind == kind)
            })
            .collect();
        Ok(Self::pick(by_name))
    }

    fn pick(mut nodes: Vec<StoredNode>) -> Resolved {
        match nodes.len() {
            0 => Resolved::None,
            1 => Resolved::One(Box::new(nodes.remove(0))),
            _ => Resolved::Many(nodes),
        }
    }
}

/// `path (kind)` into the path and the kind; the text unchanged without a known kind suffix.
fn split_kind(name: &str) -> (&str, Option<NodeKind>) {
    if let Some(open) = name.strip_suffix(')').and_then(|rest| rest.rfind(" ("))
        && let Ok(kind) = name[open + 2..name.len() - 1].parse::<NodeKind>()
    {
        return (&name[..open], Some(kind));
    }
    (name, None)
}
