use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use super::{LabelSource, NodeRef, NoteId, NoteKind};

/// Spec §6.6. `stable_id` is the 8-hex id in the community's semantic path.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Community {
    pub stable_id: String,
    pub label: String,
    pub label_source: LabelSource,
    pub members: Vec<NodeRef>,
}

/// Spec §6.8. Timestamps are the RFC 3339 strings of the note file's front matter.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Note {
    pub uuid: NoteId,
    pub target_semantic_path: String,
    pub kind: NoteKind,
    pub body: String,
    pub created: String,
    pub updated: String,
}

/// The process holding `.codemycel/session.lock` or `sync.lock` (spec §8.4).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncOwner {
    pub pid: u32,
    pub since: SystemTime,
}
