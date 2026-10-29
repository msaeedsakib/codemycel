use std::collections::HashSet;

use crate::store::{StoreError, StoreRead};
use crate::{EdgeKind, NodeId, NodeKind, SemanticPath, StoredNode};

const ANCESTOR_LEVELS: usize = 4;

pub enum Resolved {
    One(Box<StoredNode>),
    /// Several nodes match; the caller lists them and answers nothing else.
    Many(Vec<StoredNode>),
    None,
}

/// Turns a tool's name parameter into a node.
pub struct Lookup;

impl Lookup {
    /// `name` is a semantic path, optionally followed by ` (<kind>)` as answers print it, a bare
    /// name, or a qualified name as code writes it (`Router.match`, `Store::put`). A path is tried
    /// first, then the bare name, both exact and case-sensitive. Failing both, a qualified name
    /// matches nodes named by its last segment whose innermost containers carry the names before
    /// it: an impl container `<Type as Trait>` answers to `Type` or `Trait`, generic arguments are
    /// ignored, and an exact container wins over one matched by name. With no such node, members
    /// of the innermost container's `extends` ancestors are tried, nearest first.
    /// Notes and communities never match, being named after other things.
    pub fn resolve(store: &impl StoreRead, name: &str) -> Result<Resolved, StoreError> {
        let name = name.trim();
        let (name, kind) = split_kind(name);
        if let Ok(path) = name.parse::<SemanticPath>() {
            let by_path = store.node_by_semantic_path(&path, kind)?;
            if !by_path.is_empty() {
                return Ok(Self::pick(by_path));
            }
        }
        let wanted = |stored: &StoredNode| {
            !matches!(stored.node.kind, NodeKind::Note | NodeKind::Community)
                && kind.is_none_or(|kind| stored.node.kind == kind)
        };
        let by_name: Vec<StoredNode> = store
            .nodes_named(name)?
            .into_iter()
            .filter(wanted)
            .collect();
        if !by_name.is_empty() {
            return Ok(Self::pick(by_name));
        }
        let Some((containers, member)) = split_qualified(name) else {
            return Ok(Resolved::None);
        };
        let members: Vec<StoredNode> = store
            .nodes_named(member)?
            .into_iter()
            .filter(wanted)
            .collect();
        let qualified = best_fits(members.iter(), &containers);
        if !qualified.is_empty() {
            return Ok(Self::pick(qualified));
        }
        Ok(Self::pick(inherited(store, &members, &containers)?))
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

/// `A.B::c` into its containers, outermost first, and its last segment; `None` for a single
/// segment or an empty one. Separators inside brackets do not split.
fn split_qualified(name: &str) -> Option<(Vec<&str>, &str)> {
    let bytes = name.as_bytes();
    let mut segments = Vec::new();
    let mut depth = 0usize;
    let mut start = 0;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'<' | b'(' | b'[' | b'{' => depth += 1,
            b'>' | b')' | b']' | b'}' => depth = depth.saturating_sub(1),
            b'.' if depth == 0 => {
                segments.push(&name[start..i]);
                start = i + 1;
            }
            b':' if depth == 0 && bytes.get(i + 1) == Some(&b':') => {
                segments.push(&name[start..i]);
                i += 2;
                start = i;
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    segments.push(&name[start..]);
    let segments: Vec<&str> = segments.into_iter().map(str::trim).collect();
    if segments.len() < 2 || segments.iter().any(|segment| segment.is_empty()) {
        return None;
    }
    let member = segments[segments.len() - 1];
    Some((segments[..segments.len() - 1].to_vec(), member))
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Fit {
    Exact,
    SelfType,
    Trait,
}

/// The nodes among `nodes` whose containers fit `containers` best.
fn best_fits<'a>(
    nodes: impl Iterator<Item = &'a StoredNode>,
    containers: &[&str],
) -> Vec<StoredNode> {
    let fitted: Vec<(Fit, &StoredNode)> = nodes
        .filter_map(|node| Some((path_fit(&node.node.path, containers)?, node)))
        .collect();
    let Some(best) = fitted.iter().map(|(fit, _)| *fit).min() else {
        return Vec::new();
    };
    fitted
        .into_iter()
        .filter(|(fit, _)| *fit == best)
        .map(|(_, node)| node.clone())
        .collect()
}

/// How well the innermost containers of `path` carry `containers`; the worst segment counts.
fn path_fit(path: &SemanticPath, containers: &[&str]) -> Option<Fit> {
    let stored = path.containers();
    if stored.len() < containers.len() {
        return None;
    }
    stored[stored.len() - containers.len()..]
        .iter()
        .zip(containers)
        .try_fold(Fit::Exact, |worst, (stored, wanted)| {
            Some(worst.max(segment_fit(stored, wanted)?))
        })
}

fn segment_fit(stored: &str, wanted: &str) -> Option<Fit> {
    if stored == wanted {
        return Some(Fit::Exact);
    }
    let (wanted, _) = type_names(wanted);
    let (self_type, trait_name) = type_names(stored);
    if self_type == wanted {
        Some(Fit::SelfType)
    } else if trait_name == Some(wanted) {
        Some(Fit::Trait)
    } else {
        None
    }
}

/// The base names a container answers to: its self type and, for `<Type as Trait>`, the trait.
fn type_names(segment: &str) -> (&str, Option<&str>) {
    let Some(inner) = segment
        .strip_prefix('<')
        .and_then(|rest| rest.strip_suffix('>'))
    else {
        return (base_name(segment), None);
    };
    match top_level_as(inner) {
        Some(at) => (
            base_name(&inner[..at]),
            Some(base_name(&inner[at + " as ".len()..])),
        ),
        None => (base_name(inner), None),
    }
}

fn top_level_as(text: &str) -> Option<usize> {
    let mut depth = 0usize;
    for (i, byte) in text.bytes().enumerate() {
        match byte {
            b'<' | b'(' | b'[' => depth += 1,
            b'>' | b')' | b']' => depth = depth.saturating_sub(1),
            b' ' if depth == 0 && text[i..].starts_with(" as ") => return Some(i),
            _ => {}
        }
    }
    None
}

/// `&mut super::Foo<T>` to `Foo`.
fn base_name(text: &str) -> &str {
    let mut text = text.trim().trim_start_matches('&').trim_start();
    for prefix in ["mut ", "dyn ", "impl "] {
        text = text.strip_prefix(prefix).unwrap_or(text);
    }
    let text = text.split('<').next().unwrap_or(text).trim();
    text.rsplit("::").next().unwrap_or(text)
}

/// Members of the innermost container's `extends` ancestors, from the nearest level that has any.
fn inherited(
    store: &impl StoreRead,
    members: &[StoredNode],
    containers: &[&str],
) -> Result<Vec<StoredNode>, StoreError> {
    let Some((owner, outer)) = containers.split_last() else {
        return Ok(Vec::new());
    };
    let mut level: Vec<StoredNode> = store
        .nodes_named(type_names(owner).0)?
        .into_iter()
        .filter(|node| path_fit(&node.node.path, outer).is_some())
        .collect();
    let mut seen: HashSet<NodeId> = level.iter().map(|node| node.id).collect();
    for _ in 0..ANCESTOR_LEVELS {
        let mut parents = Vec::new();
        for node in &level {
            for edge in store.edges_from(node.id, &[EdgeKind::Extends])? {
                if seen.insert(edge.dst_id)
                    && let Some(parent) = store.node_by_id(edge.dst_id)?
                {
                    parents.push(parent);
                }
            }
        }
        if parents.is_empty() {
            break;
        }
        let found: Vec<StoredNode> = members
            .iter()
            .filter(|member| {
                parents
                    .iter()
                    .any(|parent| is_member_of(&member.node.path, &parent.node.path))
            })
            .cloned()
            .collect();
        if !found.is_empty() {
            return Ok(found);
        }
        level = parents;
    }
    Ok(Vec::new())
}

fn is_member_of(member: &SemanticPath, parent: &SemanticPath) -> bool {
    parent.as_str().contains("::")
        && member.containers().len() == parent.containers().len() + 1
        && member
            .as_str()
            .strip_prefix(parent.as_str())
            .is_some_and(|rest| rest.starts_with("::"))
}
