//! The query engine behind every tool: one method per read-only tool, each returning an
//! `Answer` cut to its budget and ending in a freshness footer.

mod answer;
mod budget;
mod card;
pub mod format;
mod freshness;
mod lookup;

pub mod impact;
pub mod notes;
pub mod overview;
pub mod search;
pub mod status;
pub mod symbol;
pub mod trace;
pub mod unresolved;

use std::path::{Path, PathBuf};

use thiserror::Error;

pub use answer::{
    Answer, Card, CardCounts, CardNote, CommunityLine, CommunityRef, Direction, EdgeLine, Line,
    NodeLine, NoteLine, Outcome, Section, SectionCount, SourceLine, Truncation, UnresolvedLine,
};
pub use budget::Budget;
pub use card::SymbolCard;
pub use format::{JsonFormatter, TextFormatter};
pub use freshness::{CoverageState, Freshness, LanguageCoverage};
pub use lookup::{Lookup, Resolved};

use crate::store::{Store, StoreError, StoreRead, StoredEdge};
use crate::{EdgeKind, LanguageAdapter, NodeId, SemanticPath, StoredNode};

#[derive(Debug, Error)]
pub enum QueryError {
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("reading {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

pub type Result<T, E = QueryError> = std::result::Result<T, E>;

/// Answers tools from an index opened read-only, reading source files on demand. The adapters
/// re-parse a file that changed since indexing, so bodies come from its current text.
pub struct QueryEngine<'a> {
    repo_root: PathBuf,
    store: Store,
    adapters: &'a [Box<dyn LanguageAdapter>],
}

impl<'a> QueryEngine<'a> {
    pub fn open(repo_root: &Path, store: Store, adapters: &'a [Box<dyn LanguageAdapter>]) -> Self {
        Self {
            repo_root: repo_root.to_owned(),
            store,
            adapters,
        }
    }

    pub fn store(&self) -> &Store {
        &self.store
    }

    pub fn repo_root(&self) -> &Path {
        &self.repo_root
    }

    pub fn adapters(&self) -> &[Box<dyn LanguageAdapter>] {
        self.adapters
    }

    /// Computes the freshness footer over the files the sections show, cuts the sections to
    /// `budget`, and drops stale files whose lines were cut. `hint` says how to see more.
    pub fn finish(
        &self,
        outcome: Outcome,
        sections: Vec<Section>,
        budget: Budget,
        hint: &str,
    ) -> Result<Answer> {
        let mut answer = Answer {
            outcome,
            sections,
            truncated: None,
            freshness: Freshness::default(),
        };
        answer.freshness = Freshness::compute(&self.store, &self.repo_root, &answer.files())?;
        budget.apply(&mut answer, hint);
        answer.freshness.retain_stale(&answer.files());
        Ok(answer)
    }

    /// An answer holding one line of text, for a name or scope that matched nothing.
    pub fn not_found(&self, message: impl Into<String>, budget: Budget) -> Result<Answer> {
        let section = Section::new("result", 0, vec![Line::text(message)]).without_header();
        self.finish(Outcome::NotFound, vec![section], budget, "")
    }

    /// The node `name` names, or the answer to give instead: the candidates when several match
    /// (and nothing else), or a not-found line.
    pub fn lookup(&self, name: &str, budget: Budget) -> Result<Result<StoredNode, Answer>> {
        match Lookup::resolve(&self.store, name)? {
            Resolved::One(node) => Ok(Ok(*node)),
            Resolved::Many(nodes) => {
                let lines = nodes.iter().map(|n| Line::Node(NodeLine::of(n))).collect();
                let section = Section::new("candidates", 0, lines);
                let answer = self.finish(
                    Outcome::Ambiguous,
                    vec![section],
                    budget,
                    "pass a semantic path",
                )?;
                Ok(Err(answer))
            }
            Resolved::None => Ok(Err(
                self.not_found(format!("no node named `{name}`"), budget)?
            )),
        }
    }

    pub fn node(&self, id: NodeId) -> Result<StoredNode> {
        self.store
            .node_by_id(id)?
            .ok_or_else(|| StoreError::OutOfRange("edge endpoint").into())
    }

    /// `stored` seen from the end that is not `direction`'s line node: an incoming edge shows its
    /// source, an outgoing one its target. Sites are the lines of its occurrences.
    pub fn edge_line(
        &self,
        stored: &StoredEdge,
        direction: Direction,
        via: Option<SemanticPath>,
    ) -> Result<EdgeLine> {
        let other = match direction {
            Direction::In => stored.src_id,
            Direction::Out => stored.dst_id,
        };
        let mut sites: Vec<u32> = stored.edge.occurrences.iter().map(|o| o.line).collect();
        sites.sort_unstable();
        sites.dedup();
        Ok(EdgeLine {
            direction,
            kind: stored.edge.kind,
            node: NodeLine::of(&self.node(other)?),
            sites,
            verified_by: stored.edge.verified_by,
            via,
        })
    }

    /// Dispatch expansion: the dependents of each parent `node` overrides or implements, through
    /// incoming `kinds` edges, one step up and no further, each with the parent it goes through.
    /// The parent's other overriders and implementers are siblings, not dependents, so
    /// `overrides` and `implements` are never followed into it.
    pub fn dispatch_expanded(
        &self,
        node: &StoredNode,
        kinds: &[EdgeKind],
    ) -> Result<Vec<(StoredEdge, SemanticPath)>> {
        const DISPATCH: [EdgeKind; 2] = [EdgeKind::Overrides, EdgeKind::Implements];
        let kinds: Vec<EdgeKind> = kinds
            .iter()
            .copied()
            .filter(|kind| !DISPATCH.contains(kind))
            .collect();
        if kinds.is_empty() {
            return Ok(Vec::new());
        }
        let mut expanded = Vec::new();
        for parent in self.store.edges_from(node.id, &DISPATCH)? {
            for edge in self.store.edges_to(parent.dst_id, &kinds)? {
                if edge.src_id != node.id {
                    expanded.push((edge, parent.edge.dst.path.clone()));
                }
            }
        }
        Ok(expanded)
    }
}
