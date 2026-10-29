mod identity;
mod labels;
mod leiden;

use std::collections::BTreeMap;

use codemycel_core::store::{StoreRead, Tx};
use codemycel_core::{
    Community, EdgeKind, LabelSource, NodeKind, NodeRef, NoteKind, SemanticPath, VerifiedBy,
};
use petgraph::graphmap::UnGraphMap;

use crate::IndexError;
use labels::Member;

const MEMBER_KINDS: [NodeKind; 8] = [
    NodeKind::Function,
    NodeKind::Method,
    NodeKind::Class,
    NodeKind::Struct,
    NodeKind::Enum,
    NodeKind::Trait,
    NodeKind::Interface,
    NodeKind::TypeAlias,
];
const EDGE_KINDS: [EdgeKind; 5] = [
    EdgeKind::Calls,
    EdgeKind::References,
    EdgeKind::Extends,
    EdgeKind::Implements,
    EdgeKind::Overrides,
];
const VERIFIED: [VerifiedBy; 2] = [VerifiedBy::Scip, VerifiedBy::Lsp];
const RESOLUTION: f64 = 1.0;
const OVERSIZE_RESOLUTION: f64 = 2.0;
const MIN_MEMBERS: usize = 2;

pub struct CommunityDetector;

impl CommunityDetector {
    /// Partitions the verified graph and replaces the stored communities, keeping the stable id
    /// of each community in `previous`, the partition before this sync wrote anything, that a
    /// new community overlaps enough.
    ///
    /// Members are functions, methods, and types; links are their `scip`- or `lsp`-verified
    /// calls, references, extends, implements, and overrides edges, one unit of weight each. A
    /// community holding more than a quarter of the members is partitioned once more at a finer
    /// resolution, still measured against the whole graph, unless that would scatter it into
    /// single members. Communities of a single member are not stored.
    pub fn run(tx: &mut Tx<'_>, previous: &[Community]) -> Result<(), IndexError> {
        let nodes = tx.nodes_of_kinds(&MEMBER_KINDS)?;
        let index: BTreeMap<NodeRef, u32> = nodes
            .iter()
            .zip(0u32..)
            .map(|(stored, i)| (stored.node.node_ref(), i))
            .collect();
        let mut weights: BTreeMap<(u32, u32), f64> = BTreeMap::new();
        for stored in tx.edges_of_kinds(&EDGE_KINDS, &VERIFIED)? {
            let (Some(&a), Some(&b)) = (index.get(&stored.edge.src), index.get(&stored.edge.dst))
            else {
                continue;
            };
            if a != b {
                *weights.entry((a.min(b), a.max(b))).or_insert(0.0) += 1.0;
            }
        }
        let all: Vec<u32> = index.values().copied().collect();
        let mut graph = UnGraphMap::with_capacity(all.len(), weights.len());
        for &node in &all {
            graph.add_node(node);
        }
        for (&(a, b), &weight) in &weights {
            graph.add_edge(a, b, weight);
        }
        let mut groups = Vec::new();
        for group in leiden::partition(&graph, &all, RESOLUTION) {
            if group.len() * 4 > all.len() {
                let finer = leiden::partition(&graph, &group, OVERSIZE_RESOLUTION);
                if finer.iter().any(|part| part.len() >= MIN_MEMBERS) {
                    groups.extend(finer);
                } else {
                    groups.push(group);
                }
            } else {
                groups.push(group);
            }
        }
        groups.retain(|group| group.len() >= MIN_MEMBERS);
        groups.sort_unstable_by_key(|group| group[0]);
        let member = |i: u32| &nodes[i as usize].node;
        let members: Vec<Vec<NodeRef>> = groups
            .iter()
            .map(|group| group.iter().map(|&i| member(i).node_ref()).collect())
            .collect();
        let ids = identity::assign(&members, previous);
        let label_notes = label_notes(tx)?;
        let mut communities: Vec<Community> = groups
            .iter()
            .zip(members)
            .zip(ids)
            .map(|((group, members), stable_id)| {
                let (label, label_source) = match label_notes.get(&stable_id) {
                    Some(label) => (label.clone(), LabelSource::Note),
                    None => {
                        let named: Vec<Member<'_>> = group
                            .iter()
                            .map(|&i| Member {
                                file: member(i).file.as_ref().map(|f| f.as_str()),
                                name: &member(i).name,
                            })
                            .collect();
                        labels::label(&named, &stable_id)
                    }
                };
                Community {
                    stable_id,
                    label,
                    label_source,
                    members,
                }
            })
            .collect();
        communities.sort_unstable_by(|a, b| a.stable_id.cmp(&b.stable_id));
        tx.replace_communities(&communities)?;
        Ok(())
    }
}

/// Gives the stored community `stable_id` the label its `label` notes give it, the one the next
/// sync would; leaves it as it is when they give none.
pub(crate) fn apply_label_notes(tx: &mut Tx<'_>, stable_id: &str) -> Result<(), IndexError> {
    if let Some(label) = label_notes(tx)?.get(stable_id) {
        tx.set_community_label(stable_id, label, LabelSource::Note)?;
    }
    Ok(())
}

/// The label each community's `label` notes give it, by stable id: the first non-blank line of
/// the most recently updated note, ties going to the greater uuid.
fn label_notes(tx: &Tx<'_>) -> Result<BTreeMap<String, String>, IndexError> {
    let mut best: BTreeMap<String, (Option<jiff::Timestamp>, String, String)> = BTreeMap::new();
    for stored in tx.notes()? {
        let note = stored.note;
        if note.kind != NoteKind::Label {
            continue;
        }
        let Ok(target) = note.target_semantic_path.parse::<SemanticPath>() else {
            continue;
        };
        let Some(id) = target.as_str().strip_prefix("community:") else {
            continue;
        };
        let Some(line) = note.body.lines().map(str::trim).find(|l| !l.is_empty()) else {
            continue;
        };
        let candidate = (
            note.updated.parse::<jiff::Timestamp>().ok(),
            note.uuid.0,
            line.to_owned(),
        );
        let newer = best
            .get(id)
            .is_none_or(|current| (&candidate.0, &candidate.1) > (&current.0, &current.1));
        if newer {
            best.insert(id.to_owned(), candidate);
        }
    }
    Ok(best
        .into_iter()
        .map(|(id, (_, _, label))| (id, label))
        .collect())
}
