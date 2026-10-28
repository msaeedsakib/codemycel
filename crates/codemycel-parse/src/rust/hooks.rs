use codemycel_core::{Candidate, CandidateRole, ParsedFile};
use tree_sitter::Node;

use crate::driver::LanguageHooks;

pub(super) struct RustHooks;

impl LanguageHooks for RustHooks {
    fn impl_container(&self, self_type: &str, trait_text: Option<&str>) -> String {
        match trait_text {
            Some(trait_text) => format!("<{self_type} as {trait_text}>"),
            None if self_type.contains("::") => format!("<{self_type}>"),
            None => self_type.to_owned(),
        }
    }

    fn is_doc_comment(&self, text: &str) -> bool {
        (text.starts_with("///") && !text.starts_with("////"))
            || (text.starts_with("/**") && !text.starts_with("/***") && !text.starts_with("/**/"))
    }

    fn doc_skip_kinds(&self) -> &'static [&'static str] {
        &["attribute_item"]
    }

    fn finish(&self, parsed: &mut ParsedFile, root: Node<'_>, source: &str) {
        let line_starts: Vec<usize> = std::iter::once(0)
            .chain(source.match_indices('\n').map(|(at, _)| at + 1))
            .collect();
        parsed.candidates.retain_mut(|candidate| {
            match site(root, source, &line_starts, candidate) {
                Some(node) => refine(candidate, node),
                None => true,
            }
        });
    }
}

/// The identifier node a candidate was taken from.
fn site<'t>(
    root: Node<'t>,
    source: &str,
    line_starts: &[usize],
    candidate: &Candidate,
) -> Option<Node<'t>> {
    let row = usize::try_from(candidate.at.line).ok()?.checked_sub(1)?;
    let line_start = *line_starts.get(row)?;
    let col = usize::try_from(candidate.at.col).ok()?;
    let (offset, _) = source.get(line_start..)?.char_indices().nth(col)?;
    let start = line_start + offset;
    let end = start + candidate.text.len();
    root.named_descendant_for_byte_range(start, end)
        .filter(|node| node.start_byte() == start && node.end_byte() == end)
}

/// Drops candidates inside attributes and `macro_rules!` bodies, turns the leaves of a `pub use`
/// into exports, and classifies identifiers inside macro token trees. Returns whether to keep it.
fn refine(candidate: &mut Candidate, node: Node<'_>) -> bool {
    let mut ancestor = node.parent();
    while let Some(current) = ancestor {
        match current.kind() {
            "attribute_item" | "inner_attribute_item" | "macro_definition" => return false,
            "use_declaration"
                if candidate.role == CandidateRole::Import
                    && has_child(current, "visibility_modifier") =>
            {
                candidate.role = CandidateRole::Export;
            }
            _ => {}
        }
        ancestor = current.parent();
    }
    if node
        .parent()
        .is_some_and(|parent| parent.kind() == "token_tree")
    {
        let Some(role) = token_role(node) else {
            return false;
        };
        candidate.role = role;
    }
    true
}

/// An identifier inside a macro's token tree is a call when a `(…)` group follows it and a value
/// otherwise. Path prefixes (`a` in `a::b`), macro names (`m!`), field names (`x:`), field
/// accesses (`.x`), and lifetimes (`'a`) are not candidates.
fn token_role(node: Node<'_>) -> Option<CandidateRole> {
    let next = node.next_sibling();
    let prev = node.prev_sibling().map(|prev| prev.kind());
    match next.map(|next| next.kind()) {
        Some("::" | "!" | ":") => None,
        _ if prev == Some("'") => None,
        Some("token_tree")
            if next
                .and_then(|n| n.child(0))
                .is_some_and(|c| c.kind() == "(") =>
        {
            Some(CandidateRole::Call)
        }
        _ if prev == Some(".") => None,
        _ => Some(CandidateRole::Value),
    }
}

fn has_child(node: Node<'_>, kind: &str) -> bool {
    let mut cursor = node.walk();
    node.children(&mut cursor).any(|child| child.kind() == kind)
}
