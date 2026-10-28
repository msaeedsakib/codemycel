use std::collections::{BTreeMap, BTreeSet};

use codemycel_core::{Community, NodeRef};

const ID_LEN: usize = 8;

/// The stable id of each of `communities`, whose member lists are sorted and disjoint.
///
/// A community keeps the id of the previous community it overlaps most, when their Jaccard
/// similarity is at least one half; each previous id goes to one community at most. Every other
/// community gets the first eight hex digits of the BLAKE3 hash of its sorted member paths,
/// rehashed with a counter while that id belongs to a previous or already assigned community.
pub(crate) fn assign(communities: &[Vec<NodeRef>], previous: &[Community]) -> Vec<String> {
    let mut owner: BTreeMap<&NodeRef, usize> = BTreeMap::new();
    for (index, old) in previous.iter().enumerate() {
        for member in &old.members {
            owner.insert(member, index);
        }
    }
    let mut matches: Vec<Match> = Vec::new();
    for (new, members) in communities.iter().enumerate() {
        let mut shared: BTreeMap<usize, u64> = BTreeMap::new();
        for member in members {
            if let Some(&old) = owner.get(member) {
                *shared.entry(old).or_insert(0) += 1;
            }
        }
        for (old, intersection) in shared {
            let union = len(members.len()) + len(previous[old].members.len()) - intersection;
            if 2 * intersection >= union {
                matches.push(Match {
                    new,
                    old,
                    intersection,
                    union,
                });
            }
        }
    }
    matches.sort_by(|a, b| {
        let by_similarity = (u128::from(b.intersection) * u128::from(a.union))
            .cmp(&(u128::from(a.intersection) * u128::from(b.union)));
        by_similarity
            .then(b.intersection.cmp(&a.intersection))
            .then(a.new.cmp(&b.new))
            .then_with(|| previous[a.old].stable_id.cmp(&previous[b.old].stable_id))
    });
    let mut ids: Vec<Option<String>> = vec![None; communities.len()];
    let mut used_old = vec![false; previous.len()];
    for m in matches {
        if ids[m.new].is_none() && !used_old[m.old] {
            ids[m.new] = Some(previous[m.old].stable_id.clone());
            used_old[m.old] = true;
        }
    }
    let mut taken: BTreeSet<String> = previous.iter().map(|c| c.stable_id.clone()).collect();
    taken.extend(ids.iter().flatten().cloned());
    communities
        .iter()
        .zip(ids)
        .map(|(members, id)| {
            id.unwrap_or_else(|| {
                let id = mint(members, &taken);
                taken.insert(id.clone());
                id
            })
        })
        .collect()
}

struct Match {
    new: usize,
    old: usize,
    intersection: u64,
    union: u64,
}

fn len(n: usize) -> u64 {
    u64::try_from(n).unwrap_or(u64::MAX)
}

fn mint(members: &[NodeRef], taken: &BTreeSet<String>) -> String {
    let mut paths: Vec<&str> = members.iter().map(|m| m.path.as_str()).collect();
    paths.sort_unstable();
    let mut attempt = 0u64;
    loop {
        let mut hasher = blake3::Hasher::new();
        for path in &paths {
            hasher.update(path.as_bytes());
            hasher.update(b"\n");
        }
        if attempt > 0 {
            hasher.update(format!("#{attempt}").as_bytes());
        }
        let id = hasher.finalize().to_hex()[..ID_LEN].to_owned();
        if !taken.contains(&id) {
            return id;
        }
        attempt += 1;
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use codemycel_core::{LabelSource, NodeKind, RepoPath, SemanticPath};

    use super::*;

    fn members(names: &[&str]) -> Vec<NodeRef> {
        let file = RepoPath::new("a.rs").unwrap();
        let mut refs: Vec<NodeRef> = names
            .iter()
            .map(|name| {
                let path = SemanticPath::symbol(&file, &[] as &[&str], name, None).unwrap();
                NodeRef::new(path, NodeKind::Function)
            })
            .collect();
        refs.sort();
        refs
    }

    fn old(id: &str, names: &[&str]) -> Community {
        Community {
            stable_id: id.to_owned(),
            label: String::new(),
            label_source: LabelSource::Fallback,
            members: members(names),
        }
    }

    #[test]
    fn an_old_community_split_exactly_in_half_passes_its_id_to_the_first_half_only() {
        let previous = [old("0000aaaa", &["a", "b", "c", "d"])];
        let ids = assign(&[members(&["a", "b"]), members(&["c", "d"])], &previous);
        assert_eq!(ids[0], "0000aaaa");
        assert_ne!(ids[1], "0000aaaa");
    }

    #[test]
    fn a_minted_id_never_reuses_a_previous_id() {
        let fresh = assign(&[members(&["x", "y"])], &[]);
        let previous = [old(&fresh[0], &["p", "q"])];
        let ids = assign(&[members(&["x", "y"])], &previous);
        assert_ne!(ids[0], fresh[0]);
        assert_eq!(ids[0].len(), ID_LEN);
    }

    #[test]
    fn the_closest_previous_community_wins() {
        let previous = [
            old("0000aaaa", &["a", "b", "c", "x"]),
            old("0000bbbb", &["d", "e", "y"]),
        ];
        let ids = assign(&[members(&["a", "b", "c", "d"])], &previous);
        assert_eq!(ids, vec!["0000aaaa".to_owned()]);
    }
}
