use std::collections::HashMap;
use std::ops::Range;

use codemycel_core::{
    AdapterError, ByteRange, Candidate, CandidateRole, Edge, EdgeKind, ImplRecord, Node, NodeKind,
    NodeRef, Occurrence, ParsedFile, RepoPath, SemanticPath, Span, VerifiedBy,
};
use tree_sitter::{Node as TsNode, Parser, Query, QueryCursor, StreamingIterator, Tree};

use super::Driver;
use super::doc_comment;
use super::locals::Locals;
use super::queries::{LocalCapture, SymbolCapture};
use super::signature::{normalise_signature, normalise_type};

const BOM: &[u8] = b"\xEF\xBB\xBF";

const ROLE_PRIORITY: [CandidateRole; 7] = [
    CandidateRole::Export,
    CandidateRole::Import,
    CandidateRole::Extends,
    CandidateRole::Implements,
    CandidateRole::Call,
    CandidateRole::Type,
    CandidateRole::Value,
];

pub(super) fn run(
    driver: &Driver,
    path: &RepoPath,
    bytes: &[u8],
) -> Result<ParsedFile, AdapterError> {
    let fail = |message: String| AdapterError::Parse {
        path: path.clone(),
        message,
    };
    let (offset, body) = match bytes.strip_prefix(BOM) {
        Some(rest) => (BOM.len(), rest),
        None => (0, bytes),
    };
    let source = std::str::from_utf8(body).map_err(|error| fail(error.to_string()))?;
    let file_node = SemanticPath::for_file(path)
        .map(|file| NodeRef::new(file, NodeKind::File))
        .map_err(|error| fail(error.to_string()))?;
    let mut parser = Parser::new();
    parser
        .set_language(&driver.language)
        .map_err(|error| fail(error.to_string()))?;
    let tree = parser
        .parse(source, None)
        .ok_or_else(|| fail("tree-sitter returned no tree".to_owned()))?;
    let file = File {
        path,
        source,
        offset,
        file_node,
    };
    let mut parsed = file.extract(driver, &tree).map_err(fail)?;
    driver.hooks.finish(&mut parsed, tree.root_node(), source);
    Ok(parsed)
}

struct File<'a> {
    path: &'a RepoPath,
    source: &'a str,
    offset: usize,
    file_node: NodeRef,
}

#[derive(Default)]
struct DefCaptures<'t> {
    name: Option<TsNode<'t>>,
    body: Option<TsNode<'t>>,
    visibility: Option<TsNode<'t>>,
    return_type: bool,
    overload: bool,
}

#[derive(Default)]
struct ImplCaptures<'t> {
    self_type: Option<TsNode<'t>>,
    type_name: Option<TsNode<'t>>,
    trait_type: Option<TsNode<'t>>,
    body: Option<TsNode<'t>>,
}

enum Shape<'t> {
    Def {
        kind: NodeKind,
        captures: DefCaptures<'t>,
    },
    Impl(ImplCaptures<'t>),
}

struct Element<'t> {
    node: TsNode<'t>,
    shape: Shape<'t>,
    depth: usize,
    parent: Option<usize>,
    containers: Vec<String>,
    segment: String,
    suffix: Option<u32>,
    replaced_by: Option<usize>,
    node_ref: Option<NodeRef>,
}

impl<'a> File<'a> {
    fn extract(&self, driver: &Driver, tree: &Tree) -> Result<ParsedFile, String> {
        let mut elements = self.collect_elements(driver, tree);
        elements.sort_by_key(|e| {
            (
                e.node.start_byte(),
                std::cmp::Reverse(e.node.end_byte()),
                e.depth,
            )
        });
        let by_node: HashMap<usize, usize> = elements
            .iter()
            .enumerate()
            .rev()
            .map(|(index, e)| (e.node.id(), index))
            .collect();
        for element in elements.iter_mut() {
            element.parent = enclosing_element(element.node, &by_node, true);
        }
        self.assign_paths(driver, &mut elements);

        let mut parsed = ParsedFile::default();
        for index in 0..elements.len() {
            let element = &elements[index];
            match &element.shape {
                Shape::Def { kind, captures } => {
                    let Some(node_ref) = element.node_ref.clone() else {
                        continue;
                    };
                    parsed.nodes.push(self.node(
                        driver,
                        element.node,
                        *kind,
                        captures,
                        &node_ref,
                    )?);
                    let (parent_ref, parent_kind) = ancestors(&elements, index)
                        .find_map(|e| Some((e.node_ref.clone()?, e.node_ref.as_ref()?.kind)))
                        .unwrap_or((self.file_node.clone(), NodeKind::File));
                    parsed.structural_edges.push(Edge {
                        src: parent_ref,
                        dst: node_ref,
                        kind: if parent_kind == NodeKind::Module {
                            EdgeKind::Defines
                        } else {
                            EdgeKind::Contains
                        },
                        verified_by: VerifiedBy::Syntactic,
                        occurrences: Vec::new(),
                    });
                }
                Shape::Impl(captures) => parsed
                    .impl_records
                    .push(self.impl_record(element.node, captures)?),
            }
        }
        parsed.candidates = self.candidates(driver, tree, &elements, &by_node)?;
        Ok(parsed)
    }

    fn collect_elements<'t>(&self, driver: &Driver, tree: &'t Tree) -> Vec<Element<'t>> {
        let queries = &driver.queries;
        let mut elements: Vec<Element<'t>> = Vec::new();
        let mut defs: HashMap<(usize, NodeKind), usize> = HashMap::new();
        let mut impls: HashMap<usize, usize> = HashMap::new();
        let mut cursor = QueryCursor::new();
        let mut matches =
            cursor.matches(&queries.symbols, tree.root_node(), self.source.as_bytes());
        while let Some(found) = matches.next() {
            let capture_of = |index: u32| queries.symbol_captures[index as usize];
            let def = found
                .captures()
                .iter()
                .find_map(|c| match capture_of(c.index) {
                    SymbolCapture::Def(kind) => Some((c.node, kind)),
                    _ => None,
                });
            let block = found
                .captures()
                .iter()
                .find(|c| capture_of(c.index) == SymbolCapture::Impl)
                .map(|c| c.node);
            if let Some((node, kind)) = def {
                let index = *defs.entry((node.id(), kind)).or_insert_with(|| {
                    elements.push(Element::new(
                        node,
                        Shape::Def {
                            kind,
                            captures: DefCaptures::default(),
                        },
                    ));
                    elements.len() - 1
                });
                let Shape::Def { captures, .. } = &mut elements[index].shape else {
                    continue;
                };
                for capture in found.captures() {
                    let node = Some(capture.node);
                    match capture_of(capture.index) {
                        SymbolCapture::Name => captures.name = captures.name.or(node),
                        SymbolCapture::Body => captures.body = captures.body.or(node),
                        SymbolCapture::Visibility => {
                            captures.visibility = captures.visibility.or(node)
                        }
                        SymbolCapture::ReturnType => captures.return_type = true,
                        SymbolCapture::Overload => captures.overload = true,
                        _ => {}
                    }
                }
            } else if let Some(node) = block {
                let index = *impls.entry(node.id()).or_insert_with(|| {
                    elements.push(Element::new(node, Shape::Impl(ImplCaptures::default())));
                    elements.len() - 1
                });
                let Shape::Impl(captures) = &mut elements[index].shape else {
                    continue;
                };
                for capture in found.captures() {
                    let node = Some(capture.node);
                    match capture_of(capture.index) {
                        SymbolCapture::ImplType => captures.self_type = captures.self_type.or(node),
                        SymbolCapture::ImplTypeName => {
                            captures.type_name = captures.type_name.or(node)
                        }
                        SymbolCapture::ImplTrait => {
                            captures.trait_type = captures.trait_type.or(node)
                        }
                        SymbolCapture::ImplBody => captures.body = captures.body.or(node),
                        _ => {}
                    }
                }
            }
        }
        elements.retain(|e| match &e.shape {
            Shape::Def { captures, .. } => captures.name.is_some(),
            Shape::Impl(captures) => captures.self_type.is_some() && captures.body.is_some(),
        });
        elements
    }

    /// Containers, `[n]` suffixes, overload collapsing, and semantic paths, in source order.
    fn assign_paths(&self, driver: &Driver, elements: &mut [Element<'_>]) {
        let mut seen: HashMap<(Vec<String>, String, NodeKind), u32> = HashMap::new();
        let mut first: HashMap<(Vec<String>, String, NodeKind), usize> = HashMap::new();
        let mut overloads = Vec::new();
        for index in 0..elements.len() {
            let containers = match elements[index].parent {
                Some(parent) => {
                    let parent = &elements[parent];
                    let mut chain = parent.containers.clone();
                    chain.push(parent.segment.clone());
                    chain
                }
                None => Vec::new(),
            };
            let element = &mut elements[index];
            element.containers = containers;
            match &element.shape {
                Shape::Impl(captures) => {
                    let self_type = captures.self_type.map(|n| normalise_type(self.text(n)));
                    let trait_text = captures.trait_type.map(|n| normalise_type(self.text(n)));
                    element.segment = driver
                        .hooks
                        .impl_container(&self_type.unwrap_or_default(), trait_text.as_deref());
                }
                Shape::Def { kind, captures } => {
                    let name = captures.name.map(|n| self.text(n)).unwrap_or_default();
                    let key = (element.containers.clone(), name.to_owned(), *kind);
                    element.segment = name.to_owned();
                    if captures.overload {
                        overloads.push((index, key));
                        continue;
                    }
                    let count = seen.entry(key.clone()).or_insert(0);
                    if *count > 0 {
                        element.suffix = Some(*count);
                        element.segment = format!("{name}[{count}]");
                    }
                    *count += 1;
                    first.entry(key).or_insert(index);
                }
            }
        }
        for (index, key) in overloads {
            match first.get(&key) {
                Some(&kept) => elements[index].replaced_by = Some(kept),
                None => {
                    first.insert(key, index);
                }
            }
        }
        for element in elements.iter_mut() {
            let Shape::Def { kind, captures } = &element.shape else {
                continue;
            };
            if element.replaced_by.is_some() {
                continue;
            }
            let name = captures.name.map(|n| self.text(n)).unwrap_or_default();
            element.node_ref =
                SemanticPath::symbol(self.path, &element.containers, name, element.suffix)
                    .ok()
                    .map(|path| NodeRef::new(path, *kind));
        }
    }

    fn node(
        &self,
        driver: &Driver,
        def: TsNode<'_>,
        kind: NodeKind,
        captures: &DefCaptures<'_>,
        node_ref: &NodeRef,
    ) -> Result<Node, String> {
        let signature_end = captures.body.map_or(def.end_byte(), |b| b.start_byte());
        let signature = self
            .source
            .get(def.start_byte()..signature_end)
            .map(normalise_signature)
            .filter(|s| !s.is_empty());
        Ok(Node {
            path: node_ref.path.clone(),
            kind,
            name: captures
                .name
                .map(|n| self.text(n))
                .unwrap_or_default()
                .to_owned(),
            file: Some(self.path.clone()),
            span: Some(self.span(def)?),
            selection: captures
                .name
                .map(|n| self.range(n.byte_range()))
                .transpose()?,
            signature,
            has_explicit_return_type: captures.return_type,
            body_hash: captures
                .body
                .map(|b| blake3::hash(&self.source.as_bytes()[b.byte_range()])),
            doc_comment: doc_comment::attached(def, self.source, driver.hooks.as_ref()),
            visibility: captures
                .visibility
                .map(|v| normalise_type(self.text(v)))
                .filter(|v| !v.is_empty()),
        })
    }

    fn impl_record(
        &self,
        block: TsNode<'_>,
        captures: &ImplCaptures<'_>,
    ) -> Result<ImplRecord, String> {
        let (Some(self_type), Some(body)) = (captures.self_type, captures.body) else {
            return Err("impl block without type or body".to_owned());
        };
        Ok(ImplRecord {
            file: self.path.clone(),
            self_type_text: normalise_type(self.text(self_type)),
            trait_text: captures.trait_type.map(|t| normalise_type(self.text(t))),
            span: self.span(block)?,
            header: self.range(block.start_byte()..body.start_byte())?,
            body: self.range(body.byte_range())?,
            self_type_selection: self
                .range(captures.type_name.unwrap_or(self_type).byte_range())?,
        })
    }

    fn candidates(
        &self,
        driver: &Driver,
        tree: &Tree,
        elements: &[Element<'_>],
        by_node: &HashMap<usize, usize>,
    ) -> Result<Vec<Candidate>, String> {
        let queries = &driver.queries;
        let locals = self.locals(&queries.locals, &queries.local_captures, tree);
        let mut sites: HashMap<(usize, usize), (TsNode<'_>, CandidateRole)> = HashMap::new();
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(
            &queries.candidates,
            tree.root_node(),
            self.source.as_bytes(),
        );
        while let Some(found) = matches.next() {
            for capture in found.captures() {
                let Some(role) = queries.candidate_captures[capture.index as usize] else {
                    continue;
                };
                let node = capture.node;
                let site = sites
                    .entry((node.start_byte(), node.end_byte()))
                    .or_insert((node, role));
                if priority(role) < priority(site.1) {
                    site.1 = role;
                }
            }
        }
        let mut candidates = Vec::with_capacity(sites.len());
        for (node, role) in sites.into_values() {
            let text = self.text(node);
            if text.is_empty() || locals.is_local(text, &node.byte_range()) {
                continue;
            }
            let enclosing = enclosing_element(node, by_node, false)
                .and_then(|index| resolve(elements, index))
                .unwrap_or_else(|| self.file_node.clone());
            candidates.push(Candidate {
                file: self.path.clone(),
                enclosing,
                text: text.to_owned(),
                role,
                at: self.occurrence(node)?,
            });
        }
        candidates.sort_by_key(|c| (c.at, priority(c.role)));
        Ok(candidates)
    }

    fn locals(&self, query: &Query, captures: &[LocalCapture], tree: &Tree) -> Locals {
        let mut scopes = Vec::new();
        let mut definitions = Vec::new();
        let mut exclusions = Vec::new();
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(query, tree.root_node(), self.source.as_bytes());
        while let Some(found) = matches.next() {
            for capture in found.captures() {
                let node = capture.node;
                match captures[capture.index as usize] {
                    LocalCapture::Scope => scopes.push(node.byte_range()),
                    LocalCapture::Definition => {
                        definitions.push((node.byte_range(), self.text(node).to_owned()))
                    }
                    LocalCapture::Exclude => {
                        let parent = node.parent().unwrap_or(node);
                        exclusions.push((node.byte_range(), parent.start_byte()));
                    }
                    LocalCapture::Ignored => {}
                }
            }
        }
        Locals::new(scopes, definitions, exclusions)
    }

    fn text(&self, node: TsNode<'_>) -> &'a str {
        self.source.get(node.byte_range()).unwrap_or_default()
    }

    fn range(&self, range: Range<usize>) -> Result<ByteRange, String> {
        let convert = |at: usize| {
            u32::try_from(at + self.offset).map_err(|_| format!("byte offset {at} exceeds u32"))
        };
        Ok(ByteRange {
            start: convert(range.start)?,
            end: convert(range.end)?,
        })
    }

    /// 1-based lines; a node ending at the start of a line ends on the line before.
    fn span(&self, node: TsNode<'_>) -> Result<Span, String> {
        let start = node.start_position();
        let end = node.end_position();
        let end_row = if end.column == 0 && end.row > start.row {
            end.row - 1
        } else {
            end.row
        };
        Ok(Span {
            bytes: self.range(node.byte_range())?,
            line_start: line(start.row)?,
            line_end: line(end_row)?,
        })
    }

    fn occurrence(&self, node: TsNode<'_>) -> Result<Occurrence, String> {
        let at = node.start_position();
        let line_start = node.start_byte() - at.column;
        let col = self
            .source
            .get(line_start..node.start_byte())
            .map_or(at.column, |prefix| prefix.chars().count());
        Ok(Occurrence {
            line: line(at.row)?,
            col: u32::try_from(col).map_err(|_| "column exceeds u32".to_owned())?,
        })
    }
}

impl<'t> Element<'t> {
    fn new(node: TsNode<'t>, shape: Shape<'t>) -> Self {
        let mut depth = 0;
        let mut up = node.parent();
        while let Some(parent) = up {
            depth += 1;
            up = parent.parent();
        }
        Self {
            node,
            shape,
            depth,
            parent: None,
            containers: Vec::new(),
            segment: String::new(),
            suffix: None,
            replaced_by: None,
            node_ref: None,
        }
    }
}

fn line(row: usize) -> Result<u32, String> {
    u32::try_from(row + 1).map_err(|_| "line exceeds u32".to_owned())
}

fn priority(role: CandidateRole) -> usize {
    ROLE_PRIORITY
        .iter()
        .position(|r| *r == role)
        .unwrap_or(ROLE_PRIORITY.len())
}

/// The nearest element at or above `node` in the tree (strictly above when `strict`).
fn enclosing_element(
    node: TsNode<'_>,
    by_node: &HashMap<usize, usize>,
    strict: bool,
) -> Option<usize> {
    let mut current = if strict { node.parent() } else { Some(node) };
    while let Some(n) = current {
        if let Some(&index) = by_node.get(&n.id()) {
            return Some(index);
        }
        current = n.parent();
    }
    None
}

fn ancestors<'e, 't>(
    elements: &'e [Element<'t>],
    index: usize,
) -> impl Iterator<Item = &'e Element<'t>> {
    std::iter::successors(elements[index].parent, |&i| elements[i].parent).map(|i| &elements[i])
}

/// The node a site inside `index` attaches to: the element's own node, the implementation an
/// overload collapsed into, or the nearest enclosing node.
fn resolve(elements: &[Element<'_>], index: usize) -> Option<NodeRef> {
    let mut current = Some(index);
    while let Some(i) = current {
        let element = &elements[i];
        if let Some(node_ref) = &element.node_ref {
            return Some(node_ref.clone());
        }
        current = element.replaced_by.or(element.parent);
    }
    None
}
