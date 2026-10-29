use std::collections::BTreeSet;

use serde::Serialize;

use super::Freshness;
use crate::store::StoredNote;
use crate::{
    CandidateRole, EdgeKind, NodeKind, NoteKind, RepoPath, SemanticPath, StoredNode,
    UnresolvedReason, VerifiedBy,
};

/// How a tool call ended. The CLI exits 0, 2, and 1 respectively.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    #[default]
    Ok,
    /// A bare name matched several nodes; the answer lists them and nothing else.
    Ambiguous,
    /// A name, path, or scope matched nothing.
    NotFound,
}

/// One tool answer, rendered by `TextFormatter` and `JsonFormatter`.
#[derive(Clone, Debug, PartialEq)]
pub struct Answer {
    pub outcome: Outcome,
    pub sections: Vec<Section>,
    pub truncated: Option<Truncation>,
    pub freshness: Freshness,
}

/// A named list. The name is the text header (`callers:`) and the JSON key, so it is unique
/// within an answer.
#[derive(Clone, Debug, PartialEq)]
pub struct Section {
    pub name: String,
    /// Whether text prints the `name:` header line; the leading card of `symbol` has none.
    pub header: bool,
    /// 0 is primary. The budget empties the highest number first, later sections before
    /// earlier ones on a tie.
    pub priority: u8,
    pub lines: Vec<Line>,
    /// Entries before any limit or budget cut; more than `lines.len()` when truncated.
    pub total: usize,
}

impl Section {
    /// A section with a header whose lines are all of its entries.
    pub fn new(name: impl Into<String>, priority: u8, lines: Vec<Line>) -> Self {
        let total = lines.len();
        Self {
            name: name.into(),
            header: true,
            priority,
            lines,
            total,
        }
    }

    /// The first `limit` of `lines`, remembering how many there were.
    pub fn limited(
        name: impl Into<String>,
        priority: u8,
        mut lines: Vec<Line>,
        limit: usize,
    ) -> Self {
        let total = lines.len();
        lines.truncate(limit);
        Self {
            total,
            ..Self::new(name, priority, lines)
        }
    }

    pub fn without_header(mut self) -> Self {
        self.header = false;
        self
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Line {
    Card(Card),
    Edge(EdgeLine),
    /// A node named in one line: lookup candidates, god nodes, a file's symbols.
    Node(NodeLine),
    Community(CommunityLine),
    Unresolved(UnresolvedLine),
    Note(NoteLine),
    Source(SourceLine),
    /// `key  value`.
    Field {
        key: String,
        value: String,
    },
    Text {
        text: String,
    },
}

impl Line {
    pub fn field(key: impl Into<String>, value: impl ToString) -> Self {
        Self::Field {
            key: key.into(),
            value: value.to_string(),
        }
    }

    pub fn text(text: impl Into<String>) -> Self {
        Self::Text { text: text.into() }
    }

    /// The repository files this line shows facts from, for the freshness footer.
    pub(crate) fn files(&self) -> Vec<&RepoPath> {
        match self {
            Line::Card(card) => card.node.file.iter().collect(),
            Line::Edge(edge) => edge.node.file.iter().collect(),
            Line::Node(node) => node.file.iter().collect(),
            Line::Unresolved(line) => vec![&line.file],
            Line::Community(_)
            | Line::Note(_)
            | Line::Source(_)
            | Line::Field { .. }
            | Line::Text { .. } => Vec::new(),
        }
    }
}

/// A community in one line, `[key  ]derived:<label> (<path>)[  detail]` in text.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct CommunityLine {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    pub path: SemanticPath,
    pub label: String,
    /// Always set: communities are computed, not verified.
    pub derived: bool,
    pub detail: Option<String>,
}

impl CommunityLine {
    pub fn new(path: SemanticPath, label: impl Into<String>) -> Self {
        Self {
            key: None,
            path,
            label: label.into(),
            derived: true,
            detail: None,
        }
    }

    /// `derived:<label> (<path>)`.
    pub fn name(&self) -> String {
        format!("derived:{} ({})", self.label, self.path)
    }
}

/// A node's identity and location.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct NodeLine {
    pub path: SemanticPath,
    pub kind: NodeKind,
    /// Set for communities, which are computed, not verified.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub derived: bool,
    pub file: Option<RepoPath>,
    pub line_start: Option<u32>,
    pub line_end: Option<u32>,
    pub signature: Option<String>,
}

impl NodeLine {
    pub fn of(stored: &StoredNode) -> Self {
        let node = &stored.node;
        Self {
            path: node.path.clone(),
            kind: node.kind,
            derived: node.kind == NodeKind::Community,
            file: node.file.clone(),
            line_start: node.span.map(|span| span.line_start),
            line_end: node.span.map(|span| span.line_end),
            signature: node.signature.clone(),
        }
    }
}

/// The symbol card: identity on the first line, then signature, doc, community, counts, notes.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Card {
    #[serde(flatten)]
    pub node: NodeLine,
    pub visibility: Option<String>,
    /// The first line of the doc comment, without comment markers.
    pub doc: Option<String>,
    pub community: Option<CommunityRef>,
    pub counts: CardCounts,
    pub notes: Vec<CardNote>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CommunityRef {
    pub id: String,
    pub label: String,
    pub derived: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct CardCounts {
    pub callers: u64,
    pub callees: u64,
    pub impls: u64,
    pub overrides: u64,
    pub unresolved: u32,
    pub external: u32,
}

impl CardCounts {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// A note on a card: its first line and the day it was last updated.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CardNote {
    pub kind: NoteKind,
    pub first_line: String,
    pub updated: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    /// `←`: the line's node is the edge's source.
    In,
    /// `→`: the line's node is the edge's target.
    Out,
}

/// An edge seen from one end; `node` is the other end.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct EdgeLine {
    pub direction: Direction,
    pub kind: EdgeKind,
    pub node: NodeLine,
    /// Lines of the edge's sites, in the source node's file, ascending and distinct.
    pub sites: Vec<u32>,
    pub verified_by: VerifiedBy,
    /// The parent a dispatch-expanded caller reaches this node through.
    pub via: Option<SemanticPath>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct UnresolvedLine {
    pub file: RepoPath,
    pub line: u32,
    pub col: u32,
    pub role: CandidateRole,
    pub text: String,
    pub reason: UnresolvedReason,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct NoteLine {
    pub uuid: String,
    pub kind: NoteKind,
    /// The target as written; `repo` for the repository.
    pub target: String,
    /// Set when the target is a community, which is computed, not verified.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub derived: bool,
    pub orphaned: bool,
    pub updated: String,
    pub body: String,
}

impl NoteLine {
    pub fn of(stored: &StoredNote) -> Self {
        let note = &stored.note;
        Self {
            uuid: note.uuid.0.clone(),
            kind: note.kind,
            target: note.target_semantic_path.clone(),
            derived: note.target_semantic_path.starts_with("community:"),
            orphaned: stored.orphaned,
            updated: note.updated.clone(),
            body: note.body.clone(),
        }
    }
}

/// One line of a body read from disk.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SourceLine {
    pub line: u32,
    pub text: String,
}

/// Which sections the budget or a limit cut, and how to see more.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Truncation {
    pub sections: Vec<SectionCount>,
    pub hint: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SectionCount {
    pub name: String,
    pub shown: usize,
    pub total: usize,
}

impl Answer {
    /// Every repository file a line of the answer shows facts from.
    pub fn files(&self) -> BTreeSet<RepoPath> {
        self.sections
            .iter()
            .flat_map(|section| &section.lines)
            .flat_map(Line::files)
            .cloned()
            .collect()
    }
}
