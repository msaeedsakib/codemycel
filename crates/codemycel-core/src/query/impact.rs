use std::collections::{BTreeMap, BTreeSet};

use super::{Answer, Budget, Direction, Line, NodeLine, Outcome, QueryEngine, Result, Section};
use crate::store::{StoreRead, StoredEdge};
use crate::{EdgeKind, NodeId, SemanticPath, StoredNode, UnresolvedReason};

/// The edge kinds `impact` follows when none are given.
pub const DEFAULT_KINDS: [EdgeKind; 6] = [
    EdgeKind::Calls,
    EdgeKind::References,
    EdgeKind::Extends,
    EdgeKind::Implements,
    EdgeKind::Overrides,
    EdgeKind::Imports,
];

pub const MAX_DEPTH: u8 = 5;

/// Reasons that leave open whether a candidate names the symbol; the others resolved elsewhere.
const UNDECIDED: [UnresolvedReason; 3] = [
    UnresolvedReason::NoDefinition,
    UnresolvedReason::ServerUnavailable,
    UnresolvedReason::Unmapped,
];

impl QueryEngine<'_> {
    /// What depends on `name`, walking incoming `kinds` edges up to `depth` (1 to 5) hops. Each
    /// dependent is listed once, at the first depth that reaches it, sorted by file. Callers of a
    /// parent the walked node overrides or implements are included, one step up, marked `via`.
    pub fn impact(
        &self,
        name: &str,
        depth: u8,
        kinds: &[EdgeKind],
        budget: Budget,
    ) -> Result<Answer> {
        let target = match self.lookup(name, budget)? {
            Ok(node) => node,
            Err(answer) => return Ok(answer),
        };
        let depth = depth.clamp(1, MAX_DEPTH);
        let kinds = if kinds.is_empty() {
            &DEFAULT_KINDS[..]
        } else {
            kinds
        };
        let mut seen = BTreeSet::from([target.id]);
        let mut frontier = vec![target.clone()];
        let mut levels = Vec::new();
        for _ in 0..depth {
            let found = self.dependents(&frontier, kinds, &seen)?;
            if found.is_empty() {
                break;
            }
            let mut lines = Vec::with_capacity(found.len());
            frontier = Vec::with_capacity(found.len());
            for (id, (edge, via)) in found {
                seen.insert(id);
                lines.push(self.edge_line(&edge, Direction::In, via)?);
                frontier.push(self.node(id)?);
            }
            lines.sort_by(|a, b| {
                (&a.node.file, &a.node.path, a.node.kind).cmp(&(
                    &b.node.file,
                    &b.node.path,
                    b.node.kind,
                ))
            });
            frontier.sort_by(|a, b| {
                (&a.node.file, &a.node.path, a.node.kind).cmp(&(
                    &b.node.file,
                    &b.node.path,
                    b.node.kind,
                ))
            });
            levels.push(lines);
        }
        let mut summary = vec![Line::Node(NodeLine::of(&target))];
        for (index, lines) in levels.iter().enumerate() {
            summary.push(Line::field(
                format!("depth {}", index + 1),
                plural(lines.len(), "node"),
            ));
        }
        let total = seen.len() - 1;
        summary.push(Line::field(
            "total",
            format!("{} within depth {depth}", plural(total, "distinct node")),
        ));
        summary.push(Line::field(
            "possible additional dependents",
            self.possible_dependents(&target, &seen)?,
        ));
        let mut sections = vec![Section::new("impact", 0, summary).without_header()];
        for (index, lines) in levels.into_iter().enumerate() {
            let priority = u8::try_from(index + 1).unwrap_or(u8::MAX);
            let lines = lines.into_iter().map(Line::Edge).collect();
            sections.push(Section::new(
                format!("depth {}", index + 1),
                priority,
                lines,
            ));
        }
        self.finish(
            Outcome::Ok,
            sections,
            budget,
            "raise max_tokens or narrow depth",
        )
    }

    /// The nodes not yet `seen` with a `kinds` edge into `frontier`, each with the first such
    /// edge and, for a dispatch-expanded one, the parent it goes through.
    fn dependents(
        &self,
        frontier: &[StoredNode],
        kinds: &[EdgeKind],
        seen: &BTreeSet<NodeId>,
    ) -> Result<BTreeMap<NodeId, (StoredEdge, Option<SemanticPath>)>> {
        let mut found = BTreeMap::new();
        for node in frontier {
            for edge in self.store().edges_to(node.id, kinds)? {
                if !seen.contains(&edge.src_id) {
                    found.entry(edge.src_id).or_insert((edge, None));
                }
            }
            for (edge, parent) in self.dispatch_expanded(node, kinds)? {
                if !seen.contains(&edge.src_id) {
                    found.entry(edge.src_id).or_insert((edge, Some(parent)));
                }
            }
        }
        Ok(found)
    }

    /// Candidates outside the walk whose text is the target's name and that no server settled.
    fn possible_dependents(
        &self,
        target: &StoredNode,
        walked: &BTreeSet<NodeId>,
    ) -> Result<String> {
        let mut files = BTreeSet::new();
        let mut count = 0;
        for stored in self.store().unresolved_with_text(&target.node.name)? {
            if UNDECIDED.contains(&stored.unresolved.reason) && !walked.contains(&stored.node_id) {
                count += 1;
                files.insert(stored.unresolved.candidate.file);
            }
        }
        Ok(format!(
            "{} named `{}` in {}",
            plural(count, "unresolved candidate"),
            target.node.name,
            plural(files.len(), "file")
        ))
    }
}

pub(super) fn plural(count: usize, noun: &str) -> String {
    if count == 1 {
        format!("1 {noun}")
    } else {
        format!("{count} {noun}s")
    }
}
