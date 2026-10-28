//! Markdown documents and headings.

use std::collections::HashMap;

use codemycel_core::{
    AdapterError, ByteRange, Edge, EdgeKind, Node, NodeKind, NodeRef, Occurrence, RepoPath,
    SemanticPath, Span, VerifiedBy,
};
use tree_sitter::Node as TsNode;
use tree_sitter_md::{MarkdownParser, MarkdownTree};

const BOM: &str = "\u{feff}";

/// A backtick span or relative link inside a document, attributed to the heading it sits under
/// (or the document, before the first heading).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reference {
    pub from: NodeRef,
    /// For a mention, the span's text with a trailing `()` and a leading `.` stripped; for a
    /// link, the target resolved to a repository path.
    pub text: String,
    pub at: Occurrence,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocParse {
    pub doc: Node,
    pub headings: Vec<Node>,
    /// `contains` from the document to each heading.
    pub contains: Vec<Edge>,
    pub mentions: Vec<Reference>,
    pub links: Vec<Reference>,
}

/// Extracts one document. Fenced and indented code blocks are never read for mentions.
pub fn parse(path: &RepoPath, bytes: &[u8]) -> Result<DocParse, AdapterError> {
    let fail = |message: String| AdapterError::Parse {
        path: path.clone(),
        message,
    };
    let text = std::str::from_utf8(bytes).map_err(|e| fail(e.to_string()))?;
    let (offset, source) = match text.strip_prefix(BOM) {
        Some(rest) => (BOM.len(), rest),
        None => (0, text),
    };
    let tree = MarkdownParser::default()
        .parse(source.as_bytes(), None)
        .ok_or_else(|| fail("tree-sitter returned no tree".to_owned()))?;
    let doc_path = SemanticPath::for_file(path).map_err(|e| fail(e.to_string()))?;
    let document = Document {
        path,
        source,
        offset,
        tree: &tree,
        doc_ref: NodeRef::new(doc_path.clone(), NodeKind::Doc),
    };
    let mut blocks = Vec::new();
    preorder(tree.block_tree().root_node(), &mut blocks);
    let (headings, starts) = document.headings(&blocks).map_err(fail)?;
    let contains = headings
        .iter()
        .map(|heading| Edge {
            src: document.doc_ref.clone(),
            dst: heading.node_ref(),
            kind: EdgeKind::Contains,
            verified_by: VerifiedBy::Syntactic,
            occurrences: Vec::new(),
        })
        .collect();
    let (mentions, links) = document.references(&blocks, &starts).map_err(fail)?;
    let lines = text.matches('\n').count() + usize::from(!text.is_empty() && !text.ends_with('\n'));
    Ok(DocParse {
        doc: Node {
            path: doc_path,
            kind: NodeKind::Doc,
            name: path.as_str().to_owned(),
            file: Some(path.clone()),
            span: Some(Span {
                bytes: ByteRange {
                    start: 0,
                    end: to_u32(bytes.len()).map_err(fail)?,
                },
                line_start: 1,
                line_end: to_u32(lines.max(1)).map_err(fail)?,
            }),
            selection: None,
            signature: None,
            has_explicit_return_type: false,
            body_hash: None,
            doc_comment: None,
            visibility: None,
        },
        headings,
        contains,
        mentions,
        links,
    })
}

/// A heading's start byte and identity, for attributing the references below it.
type HeadingStart = (usize, NodeRef);

struct Document<'a> {
    path: &'a RepoPath,
    source: &'a str,
    offset: usize,
    tree: &'a MarkdownTree,
    doc_ref: NodeRef,
}

impl Document<'_> {
    /// Heading nodes in document order, and their start bytes for attributing references. A
    /// heading's span runs to the next heading of the same or a higher level.
    fn headings(&self, blocks: &[TsNode<'_>]) -> Result<(Vec<Node>, Vec<HeadingStart>), String> {
        let mut found = Vec::new();
        for &block in blocks {
            let content = match block.kind() {
                "atx_heading" => block.child_by_field_name("heading_content"),
                "setext_heading" => block
                    .child_by_field_name("heading_content")
                    .and_then(|paragraph| child_of_kind(paragraph, "inline")),
                _ => continue,
            };
            if let (Some(content), Some(level)) = (content, heading_level(block)) {
                found.push((block, content, level));
            }
        }
        let mut seen: HashMap<String, u32> = HashMap::new();
        let mut headings = Vec::new();
        let mut starts = Vec::new();
        for (index, &(block, content, level)) in found.iter().enumerate() {
            let end = found[index + 1..]
                .iter()
                .find(|(_, _, next)| *next <= level)
                .map_or(self.source.len(), |(next, _, _)| next.start_byte());
            let name = self.heading_text(content, block.kind() == "atx_heading");
            let slug = slugify(&name);
            if slug.is_empty() {
                continue;
            }
            let count = seen.entry(slug.clone()).or_insert(0);
            let suffix = (*count > 0).then_some(*count);
            *count += 1;
            let Ok(path) = SemanticPath::heading(self.path, &slug, suffix) else {
                continue;
            };
            let node = Node {
                path,
                kind: NodeKind::Heading,
                name,
                file: Some(self.path.clone()),
                span: Some(Span {
                    bytes: self.range(block.start_byte(), end)?,
                    line_start: self.line_of(block.start_byte())?,
                    line_end: self.line_of(end.saturating_sub(1).max(block.start_byte()))?,
                }),
                selection: Some(self.range(content.start_byte(), content.end_byte())?),
                signature: None,
                has_explicit_return_type: false,
                body_hash: None,
                doc_comment: None,
                visibility: None,
            };
            starts.push((block.start_byte(), node.node_ref()));
            headings.push(node);
        }
        Ok((headings, starts))
    }

    /// The heading's visible text: a link reduced to its text, an image's destination and title
    /// dropped, an ATX closing sequence removed, whitespace trimmed.
    fn heading_text(&self, content: TsNode<'_>, atx: bool) -> String {
        let mut hidden = Vec::new();
        if let Some(inline) = self.tree.inline_tree(&content) {
            let mut nodes = Vec::new();
            preorder(inline.root_node(), &mut nodes);
            for node in nodes {
                match node.kind() {
                    "inline_link"
                    | "full_reference_link"
                    | "collapsed_reference_link"
                    | "shortcut_link" => match child_of_kind(node, "link_text") {
                        Some(text) => {
                            hidden.push(node.start_byte()..text.start_byte());
                            hidden.push(text.end_byte()..node.end_byte());
                        }
                        None => hidden.push(node.byte_range()),
                    },
                    "link_destination" | "link_title" => hidden.push(node.byte_range()),
                    _ => {}
                }
            }
        }
        hidden.sort_by_key(|range| range.start);
        let mut text = String::new();
        let mut at = content.start_byte();
        for range in hidden {
            text.push_str(self.source.get(at..range.start).unwrap_or_default());
            at = range.end.max(at);
        }
        text.push_str(self.source.get(at..content.end_byte()).unwrap_or_default());
        let mut text = text.trim();
        if atx {
            let without_closing = text.trim_end_matches('#');
            if without_closing.is_empty() || without_closing.ends_with([' ', '\t']) {
                text = without_closing.trim_end();
            }
        }
        text.to_owned()
    }

    fn references(
        &self,
        blocks: &[TsNode<'_>],
        starts: &[HeadingStart],
    ) -> Result<(Vec<Reference>, Vec<Reference>), String> {
        let definitions = self.definitions(blocks);
        let mut mentions = Vec::new();
        let mut links = Vec::new();
        for block in blocks
            .iter()
            .filter(|b| matches!(b.kind(), "inline" | "pipe_table_cell"))
        {
            let Some(inline) = self.tree.inline_tree(block) else {
                continue;
            };
            let mut nodes = Vec::new();
            preorder(inline.root_node(), &mut nodes);
            for node in nodes {
                let (text, target) = match node.kind() {
                    "code_span" => (mention_text(self.code_span_text(node)), &mut mentions),
                    "inline_link" => (
                        child_of_kind(node, "link_destination").and_then(|d| self.link_target(d)),
                        &mut links,
                    ),
                    "full_reference_link" => (
                        self.reference_target(child_of_kind(node, "link_label"), &definitions),
                        &mut links,
                    ),
                    "collapsed_reference_link" | "shortcut_link" => (
                        self.reference_target(child_of_kind(node, "link_text"), &definitions),
                        &mut links,
                    ),
                    _ => continue,
                };
                let Some(text) = text else {
                    continue;
                };
                let from = starts
                    .iter()
                    .take_while(|(start, _)| *start <= node.start_byte())
                    .last()
                    .map_or_else(|| self.doc_ref.clone(), |(_, heading)| heading.clone());
                target.push(Reference {
                    from,
                    text,
                    at: self.occurrence(node)?,
                });
            }
        }
        Ok((mentions, links))
    }

    /// Each link reference definition's destination by normalised label; the first definition of
    /// a label wins.
    fn definitions<'t>(&self, blocks: &[TsNode<'t>]) -> HashMap<String, TsNode<'t>> {
        let mut definitions = HashMap::new();
        for block in blocks
            .iter()
            .filter(|b| b.kind() == "link_reference_definition")
        {
            if let (Some(label), Some(destination)) = (
                child_of_kind(*block, "link_label"),
                child_of_kind(*block, "link_destination"),
            ) {
                definitions
                    .entry(self.label_key(label))
                    .or_insert(destination);
            }
        }
        definitions
    }

    fn reference_target(
        &self,
        label: Option<TsNode<'_>>,
        definitions: &HashMap<String, TsNode<'_>>,
    ) -> Option<String> {
        let destination = definitions.get(&self.label_key(label?))?;
        self.link_target(*destination)
    }

    /// A `link_label` or `link_text` matched as CommonMark does: brackets dropped, case folded,
    /// whitespace collapsed.
    fn label_key(&self, label: TsNode<'_>) -> String {
        let text = self.source.get(label.byte_range()).unwrap_or_default();
        let text = match label.kind() {
            "link_label" => text
                .strip_prefix('[')
                .and_then(|t| t.strip_suffix(']'))
                .unwrap_or(text),
            _ => text,
        };
        text.split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase()
    }

    fn code_span_text(&self, span: TsNode<'_>) -> &str {
        let mut cursor = span.walk();
        let delimiters: Vec<TsNode<'_>> = span
            .children(&mut cursor)
            .filter(|c| c.kind() == "code_span_delimiter")
            .collect();
        match (delimiters.first(), delimiters.last()) {
            (Some(open), Some(close)) if open.id() != close.id() => self
                .source
                .get(open.end_byte()..close.start_byte())
                .unwrap_or_default(),
            _ => "",
        }
    }

    /// A relative link target resolved against the document's directory; `None` for URLs,
    /// absolute paths, fragments, and paths leaving the repository.
    fn link_target(&self, destination: TsNode<'_>) -> Option<String> {
        let raw = self.source.get(destination.byte_range())?.trim();
        let raw = raw
            .strip_prefix('<')
            .and_then(|r| r.strip_suffix('>'))
            .unwrap_or(raw);
        let is_url = raw.contains("://") || raw.starts_with("mailto:");
        if raw.is_empty() || is_url || raw.starts_with(['#', '/', '\\']) {
            return None;
        }
        let raw = raw.split(['#', '?']).next()?;
        let decoded = percent_decode(raw);
        let mut segments: Vec<&str> = self.path.as_str().split('/').collect();
        segments.pop();
        for segment in decoded.split('/') {
            match segment {
                "" | "." => {}
                ".." => {
                    segments.pop()?;
                }
                other => segments.push(other),
            }
        }
        let joined = segments.join("/");
        let path = RepoPath::new(&joined).ok()?;
        (!path.is_root()).then(|| path.as_str().to_owned())
    }

    fn range(&self, start: usize, end: usize) -> Result<ByteRange, String> {
        Ok(ByteRange {
            start: to_u32(start + self.offset)?,
            end: to_u32(end + self.offset)?,
        })
    }

    /// The 1-based line holding `byte`.
    fn line_of(&self, byte: usize) -> Result<u32, String> {
        let before = self.source.get(..byte).unwrap_or(self.source);
        to_u32(before.matches('\n').count() + 1)
    }

    fn occurrence(&self, node: TsNode<'_>) -> Result<Occurrence, String> {
        let at = node.start_position();
        let line_start = node.start_byte() - at.column;
        let col = self
            .source
            .get(line_start..node.start_byte())
            .map_or(at.column, |prefix| prefix.chars().count());
        Ok(Occurrence {
            line: to_u32(at.row + 1)?,
            col: to_u32(col)?,
        })
    }
}

fn heading_level(heading: TsNode<'_>) -> Option<u8> {
    let mut cursor = heading.walk();
    heading.children(&mut cursor).find_map(|c| match c.kind() {
        "atx_h1_marker" | "setext_h1_underline" => Some(1),
        "atx_h2_marker" | "setext_h2_underline" => Some(2),
        "atx_h3_marker" => Some(3),
        "atx_h4_marker" => Some(4),
        "atx_h5_marker" => Some(5),
        "atx_h6_marker" => Some(6),
        _ => None,
    })
}

/// A backtick span's text with a leading `.` and a trailing `()` stripped.
fn mention_text(span: &str) -> Option<String> {
    let text = span.trim();
    let text = text.strip_prefix('.').unwrap_or(text);
    let text = text.strip_suffix("()").unwrap_or(text);
    (!text.is_empty()).then(|| text.to_owned())
}

/// GitHub's heading anchor: lowercase, keep letters, digits, `-`, `_`, and spaces, spaces to `-`.
fn slugify(text: &str) -> String {
    text.to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | ' '))
        .map(|c| if c == ' ' { '-' } else { c })
        .collect()
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = bytes
            .get(i + 1..i + 3)
            .and_then(|h| std::str::from_utf8(h).ok())
            .and_then(|h| u8::from_str_radix(h, 16).ok());
        match (bytes[i], hex) {
            (b'%', Some(byte)) => {
                out.push(byte);
                i += 3;
            }
            (byte, _) => {
                out.push(byte);
                i += 1;
            }
        }
    }
    String::from_utf8(out).unwrap_or_else(|_| text.to_owned())
}

fn preorder<'t>(root: TsNode<'t>, out: &mut Vec<TsNode<'t>>) {
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        out.push(node);
        let mut cursor = node.walk();
        let children: Vec<TsNode<'t>> = node.children(&mut cursor).collect();
        stack.extend(children.into_iter().rev());
    }
}

fn child_of_kind<'t>(node: TsNode<'t>, kind: &str) -> Option<TsNode<'t>> {
    let mut cursor = node.walk();
    node.children(&mut cursor).find(|c| c.kind() == kind)
}

fn to_u32(value: usize) -> Result<u32, String> {
    u32::try_from(value).map_err(|_| format!("{value} exceeds u32"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs_follow_github() {
        assert_eq!(slugify("Getting Started: v2.0!"), "getting-started-v20");
        assert_eq!(slugify("The `foo_bar` API"), "the-foo_bar-api");
        assert_eq!(slugify("Über uns"), "über-uns");
    }

    #[test]
    fn percent_escapes_decode() {
        assert_eq!(percent_decode("my%20file.rs"), "my file.rs");
        assert_eq!(percent_decode("100%"), "100%");
    }
}
