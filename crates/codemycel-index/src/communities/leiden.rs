use std::collections::{BTreeMap, VecDeque};

use petgraph::graphmap::UnGraphMap;

const SEED: u64 = 0x636f_6465_6d79_6365;

/// Partitions the nodes `within` of `graph` with the Leiden algorithm, maximising modularity at
/// `resolution` measured against the whole graph: only edges between nodes `within` link them,
/// but degrees and the total weight are those of `graph`. Returns the communities as sorted
/// node lists, ordered by their smallest node.
///
/// The result depends only on the node set, the edge weights, and `resolution`: nodes are
/// visited in an order shuffled by a fixed seed over the sorted node list, ties go to the
/// first candidate in a deterministic scan, and with integral weights every gain is compared
/// exactly.
pub(crate) fn partition(
    graph: &UnGraphMap<u32, f64>,
    within: &[u32],
    resolution: f64,
) -> Vec<Vec<u32>> {
    let mut nodes: Vec<u32> = within.to_vec();
    nodes.sort_unstable();
    nodes.dedup();
    let base = Graph::from_petgraph(graph, &nodes);
    let mut rng = SplitMix(SEED);
    let assignment = leiden(&base, resolution, &mut rng);
    let mut groups: Vec<Vec<u32>> = vec![Vec::new(); base.len()];
    for (index, &community) in assignment.iter().enumerate() {
        groups[community].push(nodes[index]);
    }
    let mut communities: Vec<Vec<u32>> = groups.into_iter().filter(|g| !g.is_empty()).collect();
    communities.sort_unstable_by_key(|c| c[0]);
    communities
}

struct Graph {
    /// Neighbours of each node, sorted, without self loops.
    adjacency: Vec<Vec<(usize, f64)>>,
    /// Weighted degree, counting a self loop twice.
    degree: Vec<f64>,
    /// Twice the total edge weight.
    total: f64,
}

impl Graph {
    /// The nodes `nodes` (sorted) of `graph`, linked by the edges among them, with the degrees
    /// and total weight of the whole graph.
    fn from_petgraph(graph: &UnGraphMap<u32, f64>, nodes: &[u32]) -> Self {
        let index = |node: u32| nodes.binary_search(&node).ok();
        let mut adjacency = vec![Vec::new(); nodes.len()];
        let mut degree = vec![0.0; nodes.len()];
        let mut total = 0.0;
        for (a, b, &weight) in graph.all_edges() {
            total += 2.0 * weight;
            let (a, b) = (index(a), index(b));
            for end in [a, b].into_iter().flatten() {
                degree[end] += weight;
            }
            if let (Some(a), Some(b)) = (a, b)
                && a != b
            {
                adjacency[a].push((b, weight));
                adjacency[b].push((a, weight));
            }
        }
        for neighbours in &mut adjacency {
            neighbours.sort_unstable_by_key(|&(n, _)| n);
        }
        Self {
            adjacency,
            degree,
            total,
        }
    }

    fn len(&self) -> usize {
        self.degree.len()
    }

    /// The graph with one node per community of `assignment`, whose labels are `0..count`.
    fn aggregate(&self, assignment: &[usize], count: usize) -> Self {
        let mut weights: Vec<BTreeMap<usize, f64>> = vec![BTreeMap::new(); count];
        let mut degree = vec![0.0; count];
        for (node, neighbours) in self.adjacency.iter().enumerate() {
            let from = assignment[node];
            degree[from] += self.degree[node];
            for &(other, weight) in neighbours {
                let to = assignment[other];
                if from != to {
                    *weights[from].entry(to).or_insert(0.0) += weight;
                }
            }
        }
        Self {
            adjacency: weights
                .into_iter()
                .map(|w| w.into_iter().collect())
                .collect(),
            degree,
            total: self.total,
        }
    }
}

/// Community of every node of `base`.
fn leiden(base: &Graph, resolution: f64, rng: &mut SplitMix) -> Vec<usize> {
    let mut membership: Vec<usize> = (0..base.len()).collect();
    if base.total == 0.0 {
        return membership;
    }
    let mut owned: Option<Graph> = None;
    let mut communities: Vec<usize> = (0..base.len()).collect();
    loop {
        let graph = owned.as_ref().unwrap_or(base);
        move_nodes(graph, &mut communities, resolution, rng);
        let count = relabel(&mut communities);
        if count == graph.len() {
            break;
        }
        let mut refined = refine(graph, &communities, resolution, rng);
        let refined_count = relabel(&mut refined);
        if refined_count == graph.len() {
            break;
        }
        let mut next = vec![0; refined_count];
        for (node, &cell) in refined.iter().enumerate() {
            next[cell] = communities[node];
        }
        for member in &mut membership {
            *member = refined[*member];
        }
        let aggregated = graph.aggregate(&refined, refined_count);
        communities = next;
        owned = Some(aggregated);
    }
    membership.iter().map(|&node| communities[node]).collect()
}

/// Renumbers labels to `0..count` in order of first appearance and returns `count`.
fn relabel(labels: &mut [usize]) -> usize {
    let mut map = vec![usize::MAX; labels.len()];
    let mut next = 0;
    for label in labels.iter_mut() {
        if map[*label] == usize::MAX {
            map[*label] = next;
            next += 1;
        }
        *label = map[*label];
    }
    next
}

/// Twice the total weight times the modularity gain of joining a community of total degree
/// `community_degree` holding `links` weight of edges to a node of degree `degree`, against
/// staying alone.
fn gain(graph: &Graph, links: f64, degree: f64, community_degree: f64, resolution: f64) -> f64 {
    graph.total * links - resolution * degree * community_degree
}

/// Moves single nodes to the neighbouring community with the best gain until no move improves
/// the partition. Labels in `communities` stay below `graph.len()`.
fn move_nodes(graph: &Graph, communities: &mut [usize], resolution: f64, rng: &mut SplitMix) {
    let n = graph.len();
    let mut community_degree = vec![0.0; n];
    let mut size = vec![0usize; n];
    for (node, &c) in communities.iter().enumerate() {
        community_degree[c] += graph.degree[node];
        size[c] += 1;
    }
    let mut empty: Vec<usize> = (0..n).rev().filter(|&c| size[c] == 0).collect();
    let mut order: Vec<usize> = (0..n).collect();
    rng.shuffle(&mut order);
    let mut queued = vec![true; n];
    let mut queue: VecDeque<usize> = order.into();
    let mut links = vec![0.0; n];
    let mut touched: Vec<usize> = Vec::new();
    while let Some(node) = queue.pop_front() {
        queued[node] = false;
        let current = communities[node];
        let degree = graph.degree[node];
        community_degree[current] -= degree;
        size[current] -= 1;
        for &(other, weight) in &graph.adjacency[node] {
            let c = communities[other];
            if links[c] == 0.0 {
                touched.push(c);
            }
            links[c] += weight;
        }
        let mut best = current;
        let mut best_gain = gain(
            graph,
            links[current],
            degree,
            community_degree[current],
            resolution,
        );
        for &c in &touched {
            let candidate = gain(graph, links[c], degree, community_degree[c], resolution);
            if candidate > best_gain {
                best = c;
                best_gain = candidate;
            }
        }
        let alone = size[current] > 0 && best_gain < 0.0;
        if alone && let Some(fresh) = empty.pop() {
            best = fresh;
        }
        for &c in &touched {
            links[c] = 0.0;
        }
        touched.clear();
        community_degree[best] += degree;
        size[best] += 1;
        if best == current {
            continue;
        }
        if size[current] == 0 {
            empty.push(current);
        }
        communities[node] = best;
        for &(other, _) in &graph.adjacency[node] {
            if !queued[other] && communities[other] != best {
                queued[other] = true;
                queue.push_back(other);
            }
        }
    }
}

/// Splits every community of `communities` into well-connected sub-communities, merging
/// singletons greedily within their community. Returns the sub-community of every node.
fn refine(graph: &Graph, communities: &[usize], resolution: f64, rng: &mut SplitMix) -> Vec<usize> {
    let n = graph.len();
    let mut members: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (node, &c) in communities.iter().enumerate() {
        members[c].push(node);
    }
    let mut refined: Vec<usize> = (0..n).collect();
    let mut cell_degree = graph.degree.clone();
    let mut cell_size = vec![1usize; n];
    let mut external = vec![0.0; n];
    for node in 0..n {
        external[node] = graph.adjacency[node]
            .iter()
            .filter(|&&(other, _)| communities[other] == communities[node])
            .map(|&(_, w)| w)
            .sum();
    }
    let mut links = vec![0.0; n];
    let mut touched: Vec<usize> = Vec::new();
    for group in members.iter_mut().filter(|g| g.len() > 1) {
        let community = communities[group[0]];
        let group_degree: f64 = group.iter().map(|&v| graph.degree[v]).sum();
        let well_connected = |links_out: f64, degree: f64| {
            graph.total * links_out >= resolution * degree * (group_degree - degree)
        };
        rng.shuffle(group);
        for &node in group.iter() {
            let own = refined[node];
            let degree = graph.degree[node];
            if cell_size[own] != 1 || !well_connected(external[node], degree) {
                continue;
            }
            for &(other, weight) in &graph.adjacency[node] {
                if communities[other] != community {
                    continue;
                }
                let cell = refined[other];
                if links[cell] == 0.0 {
                    touched.push(cell);
                }
                links[cell] += weight;
            }
            let mut best = own;
            let mut best_gain = 0.0;
            for &cell in &touched {
                if !well_connected(external[cell], cell_degree[cell]) {
                    continue;
                }
                let candidate = gain(graph, links[cell], degree, cell_degree[cell], resolution);
                if candidate > best_gain {
                    best = cell;
                    best_gain = candidate;
                }
            }
            if best != own {
                external[best] += external[node] - 2.0 * links[best];
                cell_degree[best] += degree;
                cell_size[best] += 1;
                cell_degree[own] = 0.0;
                cell_size[own] = 0;
                refined[node] = best;
            }
            for &cell in &touched {
                links[cell] = 0.0;
            }
            touched.clear();
        }
    }
    refined
}

/// SplitMix64: a small, fixed pseudo-random sequence for visit orders.
struct SplitMix(u64);

impl SplitMix {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    fn shuffle<T>(&mut self, items: &mut [T]) {
        for i in (1..items.len()).rev() {
            let bound = u64::try_from(i + 1).unwrap_or(u64::MAX);
            let j = usize::try_from(self.next() % bound).unwrap_or(0);
            items.swap(i, j);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn graph(edges: &[(u32, u32)], nodes: u32) -> UnGraphMap<u32, f64> {
        let mut graph = UnGraphMap::new();
        for node in 0..nodes {
            graph.add_node(node);
        }
        for &(a, b) in edges {
            graph.add_edge(a, b, 1.0);
        }
        graph
    }

    fn whole(graph: &UnGraphMap<u32, f64>, resolution: f64) -> Vec<Vec<u32>> {
        let nodes: Vec<u32> = graph.nodes().collect();
        partition(graph, &nodes, resolution)
    }

    #[test]
    fn two_triangles_joined_by_one_edge_split_in_two() {
        let g = graph(&[(0, 1), (1, 2), (0, 2), (3, 4), (4, 5), (3, 5), (2, 3)], 6);
        assert_eq!(whole(&g, 1.0), vec![vec![0, 1, 2], vec![3, 4, 5]]);
    }

    #[test]
    fn ring_of_cliques_yields_one_community_per_clique() {
        let mut edges = Vec::new();
        for clique in 0..6u32 {
            let base = clique * 5;
            for a in 0..5 {
                for b in a + 1..5 {
                    edges.push((base + a, base + b));
                }
            }
            edges.push((base + 4, (base + 5) % 30));
        }
        let expected: Vec<Vec<u32>> = (0..6).map(|c| (c * 5..c * 5 + 5).collect()).collect();
        assert_eq!(whole(&graph(&edges, 30), 1.0), expected);
    }

    fn clique(base: u32, size: u32, edges: &mut Vec<(u32, u32)>) {
        for a in 0..size {
            for b in a + 1..size {
                edges.push((base + a, base + b));
            }
        }
    }

    #[test]
    fn higher_resolution_splits_blocks_that_merge_at_resolution_one() {
        let mut edges = vec![];
        clique(0, 6, &mut edges);
        clique(6, 6, &mut edges);
        edges.extend([(0, 6), (1, 7), (2, 8), (3, 9)]);
        for base in [12, 22, 32] {
            clique(base, 10, &mut edges);
        }
        let g = graph(&edges, 42);
        assert_eq!(
            whole(&g, 1.0),
            vec![
                (0..12).collect::<Vec<_>>(),
                (12..22).collect(),
                (22..32).collect(),
                (32..42).collect()
            ]
        );
        let blocks: Vec<u32> = (0..12).collect();
        assert_eq!(
            partition(&g, &blocks, 2.0),
            vec![(0..6).collect::<Vec<_>>(), (6..12).collect()]
        );
    }

    #[test]
    fn isolated_nodes_stay_alone() {
        let g = graph(&[(0, 1)], 3);
        assert_eq!(whole(&g, 1.0), vec![vec![0, 1], vec![2]]);
    }
}
