use tree_sitter::Node;

use super::LanguageHooks;

/// The doc comments directly before `def` (or before its outermost wrapper), in source order,
/// joined by newlines, with CRLF read as LF.
pub(super) fn attached(def: Node<'_>, source: &str, hooks: &dyn LanguageHooks) -> Option<String> {
    let mut anchor = def;
    while let Some(parent) = anchor.parent() {
        if !hooks.doc_wrapper_kinds().contains(&parent.kind()) {
            break;
        }
        anchor = parent;
    }
    let mut comments = Vec::new();
    let mut sibling = anchor.prev_sibling();
    while let Some(node) = sibling {
        if node.is_extra() {
            let text = source.get(node.byte_range())?;
            if hooks.is_doc_comment(text) {
                comments.push(text.replace("\r\n", "\n").trim_end().to_owned());
            }
        } else if !hooks.doc_skip_kinds().contains(&node.kind()) {
            break;
        }
        sibling = node.prev_sibling();
    }
    if comments.is_empty() {
        return None;
    }
    comments.reverse();
    Some(comments.join("\n"))
}
