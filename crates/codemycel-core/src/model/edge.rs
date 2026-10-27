use serde::{Deserialize, Serialize};

use super::{EdgeKind, NodeRef, Occurrence, VerifiedBy};

/// At most one per (src, dst, kind) (spec §6.3). The edge's root is the root of `src`'s file.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Edge {
    pub src: NodeRef,
    pub dst: NodeRef,
    pub kind: EdgeKind,
    pub verified_by: VerifiedBy,
    pub occurrences: Vec<Occurrence>,
}
