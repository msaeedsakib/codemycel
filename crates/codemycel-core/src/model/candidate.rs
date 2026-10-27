use serde::{Deserialize, Serialize};

use super::{CandidateRole, NodeRef, Occurrence, RepoPath, UnresolvedReason};

/// A reference site whose target is not yet known (spec §6.5).
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Candidate {
    pub file: RepoPath,
    /// The innermost named node containing the site; the file node at top level.
    pub enclosing: NodeRef,
    pub text: String,
    pub role: CandidateRole,
    pub at: Occurrence,
}

/// A resolver's answer for one candidate (spec §4). There is no third state.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "verdict")]
pub enum Verdict {
    Confirmed { target: NodeRef },
    Unresolved { reason: UnresolvedReason },
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Unresolved {
    pub candidate: Candidate,
    pub reason: UnresolvedReason,
}
