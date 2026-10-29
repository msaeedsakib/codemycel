use super::answer::{Card, CardCounts, CardNote, CommunityRef, NodeLine};
use crate::store::{StoreError, StoreRead, StoredNote};
use crate::{EdgeKind, NodeKind, NoteId, StoredNode};

/// Builds symbol cards.
pub struct SymbolCard;

impl SymbolCard {
    pub fn for_node(store: &impl StoreRead, stored: &StoredNode) -> Result<Card, StoreError> {
        let id = stored.id;
        let node = &stored.node;
        let both = |kind| -> Result<u64, StoreError> {
            Ok(store.count_edges_to(id, kind)? + store.count_edges_from(id, kind)?)
        };
        let counts = CardCounts {
            callers: store.count_edges_to(id, EdgeKind::Calls)?,
            callees: store.count_edges_from(id, EdgeKind::Calls)?,
            impls: both(EdgeKind::Implements)?,
            overrides: both(EdgeKind::Overrides)?,
            unresolved: stored.unresolved_out,
            external: stored.external_out,
        };
        let community = match stored.community_id {
            Some(community) => store.node_by_id(community)?.map(|c| CommunityRef {
                id: c.node.path.name().to_owned(),
                label: c.node.name,
                derived: true,
            }),
            None => None,
        };
        let notes = Self::notes_on(store, stored)?
            .into_iter()
            .map(|stored| CardNote {
                kind: stored.note.kind,
                first_line: first_line(&stored.note.body).to_owned(),
                updated: stored.note.updated,
            })
            .collect();
        let doc = match node.kind {
            NodeKind::Note => {
                Self::note_of(store, stored)?.map(|note| first_line(&note.note.body).to_owned())
            }
            _ => node.doc_comment.as_deref().and_then(doc_first_line),
        };
        Ok(Card {
            node: NodeLine::of(stored),
            visibility: node.visibility.clone(),
            doc: doc.filter(|doc| !doc.is_empty()),
            community,
            counts,
            notes,
        })
    }

    /// The notes annotating `stored`, by uuid.
    pub fn notes_on(
        store: &impl StoreRead,
        stored: &StoredNode,
    ) -> Result<Vec<StoredNote>, StoreError> {
        let mut notes = Vec::new();
        for edge in store.edges_to(stored.id, &[EdgeKind::Annotates])? {
            let Some(note) = store.node_by_id(edge.src_id)? else {
                continue;
            };
            if let Some(note) = Self::note_of(store, &note)? {
                notes.push(note);
            }
        }
        notes.sort_by(|a, b| a.note.uuid.cmp(&b.note.uuid));
        Ok(notes)
    }

    /// The note row behind a `note` node.
    pub fn note_of(
        store: &impl StoreRead,
        stored: &StoredNode,
    ) -> Result<Option<StoredNote>, StoreError> {
        match stored.node.kind {
            NodeKind::Note => store.note_by_uuid(&NoteId(stored.node.path.name().to_owned())),
            _ => Ok(None),
        }
    }
}

fn first_line(text: &str) -> &str {
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("")
}

/// The first line of a raw doc comment with `///`, `//!`, `/**`, `*` and `*/` markers removed.
fn doc_first_line(raw: &str) -> Option<String> {
    raw.lines()
        .map(|line| {
            let line = line.trim();
            let line = ["///", "//!", "/**", "/*!", "//", "*/", "*"]
                .iter()
                .find_map(|marker| line.strip_prefix(marker))
                .unwrap_or(line);
            line.strip_suffix("*/").unwrap_or(line).trim()
        })
        .find(|line| !line.is_empty())
        .map(str::to_owned)
}
