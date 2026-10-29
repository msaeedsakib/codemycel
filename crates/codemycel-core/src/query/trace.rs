use std::collections::{BTreeMap, VecDeque};

use super::{Answer, Budget, Direction, Line, NodeLine, Outcome, QueryEngine, Result, Section};
use crate::store::{StoreRead, StoredEdge};
use crate::{EdgeKind, NodeId, StoredNode};

/// The edge kinds a resolver verifies, which `trace` follows when none are given.
pub const VERIFIED_KINDS: [EdgeKind; 7] = [
    EdgeKind::Imports,
    EdgeKind::Exports,
    EdgeKind::Calls,
    EdgeKind::References,
    EdgeKind::Extends,
    EdgeKind::Implements,
    EdgeKind::Overrides,
];

/// Reached nodes listed when there is no path.
const NEAREST: usize = 10;

/// Edges walked backwards from a parent to the methods and types that override or implement
/// it, so a path can pass through dynamic dispatch.
const DISPATCH: [EdgeKind; 2] = [EdgeKind::Overrides, EdgeKind::Implements];

/// How the walk reached a node: along `edge`, or against it when `down` steps from a parent to
/// an implementation.
#[derive(Clone)]
struct Step {
    edge: StoredEdge,
    down: bool,
}

impl QueryEngine<'_> {
    /// The shortest directed path from `from` to `to` over `kinds` edges, at most `max_depth`
    /// hops, as a chain of outgoing edge lines. When `kinds` holds `overrides` or `implements`,
    /// the walk also steps from a trait or interface member down to what overrides or implements
    /// it, shown as that incoming edge marked `via` the parent, so a call through dynamic dispatch
    /// reaches the implementations. Without a path, the reached nodes nearest to `to`:
    /// those sharing the longest leading run of path segments with it, then the fewest hops away.
    pub fn trace(
        &self,
        from: &str,
        to: &str,
        kinds: &[EdgeKind],
        max_depth: u32,
        budget: Budget,
    ) -> Result<Answer> {
        let start = match self.lookup(from, budget)? {
            Ok(node) => node,
            Err(answer) => return Ok(answer),
        };
        let goal = match self.lookup(to, budget)? {
            Ok(node) => node,
            Err(answer) => return Ok(answer),
        };
        let kinds = if kinds.is_empty() {
            &VERIFIED_KINDS[..]
        } else {
            kinds
        };
        let dispatch: Vec<EdgeKind> = DISPATCH
            .into_iter()
            .filter(|kind| kinds.contains(kind))
            .collect();
        let mut reached: BTreeMap<NodeId, (u32, Option<Step>)> = BTreeMap::new();
        reached.insert(start.id, (0, None));
        let mut queue = VecDeque::from([(start.id, 0)]);
        while let Some((id, hops)) = queue.pop_front() {
            if id == goal.id || hops == max_depth {
                continue;
            }
            let forward = self.store().edges_from(id, kinds)?.into_iter().map(|edge| {
                let next = edge.dst_id;
                (next, Step { edge, down: false })
            });
            let down = if dispatch.is_empty() {
                Vec::new()
            } else {
                self.store().edges_to(id, &dispatch)?
            }
            .into_iter()
            .map(|edge| (edge.src_id, Step { edge, down: true }));
            for (next, step) in forward.chain(down) {
                if let std::collections::btree_map::Entry::Vacant(slot) = reached.entry(next) {
                    slot.insert((hops + 1, Some(step)));
                    queue.push_back((next, hops + 1));
                }
            }
            if reached.contains_key(&goal.id) {
                break;
            }
        }
        if reached.contains_key(&goal.id) {
            let mut chain = Vec::new();
            let mut at = goal.id;
            while let Some((_, Some(step))) = reached.get(&at) {
                chain.push(step.clone());
                at = if step.down {
                    step.edge.dst_id
                } else {
                    step.edge.src_id
                };
            }
            chain.reverse();
            let mut lines = vec![Line::Node(NodeLine::of(&start))];
            for step in &chain {
                let line = if step.down {
                    let parent = step.edge.edge.dst.path.clone();
                    self.edge_line(&step.edge, Direction::In, Some(parent))?
                } else {
                    self.edge_line(&step.edge, Direction::Out, None)?
                };
                lines.push(Line::Edge(line));
            }
            let section = Section::new("path", 0, lines);
            return self.finish(Outcome::Ok, vec![section], budget, "raise max_tokens");
        }
        let result = Section::new(
            "result",
            0,
            vec![Line::text(format!("no path within depth {max_depth}"))],
        )
        .without_header();
        let mut nearest = Vec::new();
        for (&id, &(hops, _)) in &reached {
            if id != start.id {
                let node = self.node(id)?;
                nearest.push((shared_segments(&node, &goal), hops, node));
            }
        }
        nearest.sort_by(|a, b| {
            b.0.cmp(&a.0)
                .then(a.1.cmp(&b.1))
                .then_with(|| (&a.2.node.path, a.2.node.kind).cmp(&(&b.2.node.path, b.2.node.kind)))
        });
        let lines = nearest
            .iter()
            .take(NEAREST)
            .map(|(_, _, node)| Line::Node(NodeLine::of(node)))
            .collect();
        let nearest = Section::new("nearest", 1, lines);
        self.finish(
            Outcome::Ok,
            vec![result, nearest],
            budget,
            "raise max_tokens",
        )
    }
}

/// How many leading path segments, split at `/` and `::`, the two nodes' semantic paths share.
fn shared_segments(a: &StoredNode, b: &StoredNode) -> usize {
    let segments = |node: &StoredNode| -> Vec<String> {
        node.node
            .path
            .as_str()
            .split("::")
            .flat_map(|part| part.split('/'))
            .map(str::to_owned)
            .collect()
    };
    segments(a)
        .iter()
        .zip(segments(b).iter())
        .take_while(|(x, y)| x == y)
        .count()
}
