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

impl QueryEngine<'_> {
    /// The shortest directed path from `from` to `to` over `kinds` edges, at most `max_depth`
    /// hops, as a chain of outgoing edge lines. Without one, the reached nodes nearest to `to`:
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
        let mut reached: BTreeMap<NodeId, (u32, Option<StoredEdge>)> = BTreeMap::new();
        reached.insert(start.id, (0, None));
        let mut queue = VecDeque::from([(start.id, 0)]);
        while let Some((id, hops)) = queue.pop_front() {
            if id == goal.id || hops == max_depth {
                continue;
            }
            for edge in self.store().edges_from(id, kinds)? {
                let dst = edge.dst_id;
                if let std::collections::btree_map::Entry::Vacant(slot) = reached.entry(dst) {
                    slot.insert((hops + 1, Some(edge)));
                    queue.push_back((dst, hops + 1));
                }
            }
            if reached.contains_key(&goal.id) {
                break;
            }
        }
        if reached.contains_key(&goal.id) {
            let mut chain = Vec::new();
            let mut at = goal.id;
            while let Some((_, Some(edge))) = reached.get(&at) {
                chain.push(edge.clone());
                at = edge.src_id;
            }
            chain.reverse();
            let mut lines = vec![Line::Node(NodeLine::of(&start))];
            for edge in &chain {
                lines.push(Line::Edge(self.edge_line(edge, Direction::Out, None)?));
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
