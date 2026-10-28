use std::collections::BTreeSet;

use super::{
    Answer, Budget, Direction, Line, NoteLine, Outcome, QueryEngine, Result, Section, SourceLine,
    SymbolCard, UnresolvedLine,
};
use crate::store::{StoreRead, StoredEdge};
use crate::{ByteRange, EdgeKind, RepoPath, StoredNode};

/// What `symbol` includes besides the card.
#[derive(Clone, Copy, Debug)]
pub struct SymbolOptions {
    pub include_body: bool,
    pub callers: bool,
    pub callees: bool,
    /// Entries per list.
    pub limit: u32,
}

impl QueryEngine<'_> {
    /// The card of `name`, its implementation relations, callers (dispatch-expanded one step,
    /// marked `via`), callees, unresolved candidates, notes, and optionally its body as on disk.
    pub fn symbol(&self, name: &str, options: SymbolOptions, budget: Budget) -> Result<Answer> {
        let node = match self.lookup(name, budget)? {
            Ok(node) => node,
            Err(answer) => return Ok(answer),
        };
        let limit = options.limit as usize;
        let store = self.store();
        let card = SymbolCard::for_node(store, &node)?;
        let mut sections = vec![Section::new("symbol", 0, vec![Line::Card(card)]).without_header()];
        let mut implementers = self.lines(
            &store.edges_to(node.id, &[EdgeKind::Implements, EdgeKind::Overrides])?,
            Direction::In,
        )?;
        implementers.sort_by(|a, b| sort_key(a).cmp(&sort_key(b)));
        sections.push(Section::limited("implementers", 1, implementers, limit));
        let implements = store.edges_from(node.id, &[EdgeKind::Implements])?;
        sections.push(Section::limited(
            "implements",
            1,
            self.lines(&implements, Direction::Out)?,
            limit,
        ));
        let overrides = store.edges_from(node.id, &[EdgeKind::Overrides])?;
        sections.push(Section::limited(
            "overrides",
            1,
            self.lines(&overrides, Direction::Out)?,
            limit,
        ));
        if options.callers {
            sections.push(Section::limited("callers", 2, self.callers(&node)?, limit));
        }
        if options.callees {
            let callees = store.edges_from(node.id, &[EdgeKind::Calls])?;
            sections.push(Section::limited(
                "callees",
                3,
                self.lines(&callees, Direction::Out)?,
                limit,
            ));
        }
        let unresolved = store
            .unresolved_for_node(node.id)?
            .into_iter()
            .map(|stored| {
                let candidate = stored.unresolved.candidate;
                Line::Unresolved(UnresolvedLine {
                    file: candidate.file,
                    line: candidate.at.line,
                    col: candidate.at.col,
                    role: candidate.role,
                    text: candidate.text,
                    reason: stored.unresolved.reason,
                })
            })
            .collect();
        sections.push(Section::limited("unresolved", 4, unresolved, limit));
        let notes = SymbolCard::notes_on(store, &node)?
            .iter()
            .map(|note| Line::Note(NoteLine::of(note)))
            .collect();
        sections.push(Section::new("notes", 4, notes));
        if options.include_body {
            sections.push(Section::new("body", 1, self.body(&node)?));
        }
        self.finish(Outcome::Ok, sections, budget, "raise max_tokens or limit")
    }

    fn lines(&self, edges: &[StoredEdge], direction: Direction) -> Result<Vec<Line>> {
        edges
            .iter()
            .map(|edge| Ok(Line::Edge(self.edge_line(edge, direction, None)?)))
            .collect()
    }

    /// Direct callers, then callers of the parents `node` overrides or implements that are not
    /// already direct callers.
    fn callers(&self, node: &StoredNode) -> Result<Vec<Line>> {
        let direct = self.store().edges_to(node.id, &[EdgeKind::Calls])?;
        let seen: BTreeSet<_> = direct.iter().map(|edge| edge.src_id).collect();
        let mut lines = self.lines(&direct, Direction::In)?;
        for (edge, parent) in self.dispatch_expanded(node, &[EdgeKind::Calls])? {
            if !seen.contains(&edge.src_id) {
                lines.push(Line::Edge(self.edge_line(
                    &edge,
                    Direction::In,
                    Some(parent),
                )?));
            }
        }
        Ok(lines)
    }

    /// The node's text from the file as it is now, numbered. When the file changed since
    /// indexing, the node is found again by re-parsing it, and a `stale` line leads.
    fn body(&self, node: &StoredNode) -> Result<Vec<Line>> {
        let (Some(file), Some(span)) = (&node.node.file, node.node.span) else {
            return Ok(vec![Line::text("no body: the node has no source range")]);
        };
        let Some(indexed) = self.store().file_by_path(file)? else {
            return Ok(vec![Line::text("no body: the file is not indexed")]);
        };
        let Ok(bytes) = std::fs::read(self.repo_root().join(file.as_str())) else {
            return Ok(vec![Line::field("stale", "the file is gone")]);
        };
        if blake3::hash(&bytes) == indexed.record.hash {
            return Ok(numbered(&bytes, span.bytes, span.line_start));
        }
        let stale = Line::field(
            "stale",
            "the file changed since indexing; showing its current text",
        );
        match self.reparse(file, &bytes, node) {
            Some((range, line_start)) => {
                let mut lines = vec![stale];
                lines.extend(numbered(&bytes, range, line_start));
                Ok(lines)
            }
            None => Ok(vec![
                stale,
                Line::text(format!("`{}` is no longer in this file", node.node.name)),
            ]),
        }
    }

    /// The node's current byte range and first line in `bytes`.
    fn reparse(
        &self,
        file: &RepoPath,
        bytes: &[u8],
        node: &StoredNode,
    ) -> Option<(ByteRange, u32)> {
        let extension = file.as_str().rsplit_once('.')?.1;
        let adapter = self
            .adapters()
            .iter()
            .find(|adapter| adapter.extensions().contains(&extension))?;
        let parsed = adapter.parse(file, bytes).ok()?;
        let found = parsed
            .nodes
            .into_iter()
            .find(|n| n.path == node.node.path && n.kind == node.node.kind)?;
        found.span.map(|span| (span.bytes, span.line_start))
    }
}

fn sort_key(line: &Line) -> Option<(&str, &str)> {
    match line {
        Line::Edge(edge) => Some((edge.node.path.as_str(), edge.kind.as_str())),
        _ => None,
    }
}

fn numbered(bytes: &[u8], range: ByteRange, line_start: u32) -> Vec<Line> {
    let start = (range.start as usize).min(bytes.len());
    let end = (range.end as usize).clamp(start, bytes.len());
    String::from_utf8_lossy(&bytes[start..end])
        .split('\n')
        .zip(line_start..)
        .map(|(text, line)| {
            Line::Source(SourceLine {
                line,
                text: text.trim_end_matches('\r').to_owned(),
            })
        })
        .collect()
}
