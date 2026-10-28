use std::collections::BTreeMap;

use codemycel_core::LabelSource;
use codemycel_core::text::name_split;

const SKIPPED_SEGMENTS: [&str; 7] = ["src", "lib", "pkg", "crates", "packages", "app", "apps"];

pub(crate) struct Member<'a> {
    pub file: Option<&'a str>,
    pub name: &'a str,
}

/// The display label of a community with stable id `id`: its majority folder, else the common
/// leading words of its member names, else `cluster-<id>`.
pub(crate) fn label(members: &[Member<'_>], id: &str) -> (String, LabelSource) {
    if let Some(folder) = majority_folder(members) {
        return (folder, LabelSource::Folder);
    }
    if let Some(prefix) = common_prefix(members) {
        return (prefix, LabelSource::Prefix);
    }
    (format!("cluster-{id}"), LabelSource::Fallback)
}

/// The deepest directory holding more than half of the members, written without its
/// `src`-like segments; `None` when that leaves nothing.
fn majority_folder(members: &[Member<'_>]) -> Option<String> {
    let mut counts: BTreeMap<Vec<&str>, usize> = BTreeMap::new();
    for file in members.iter().filter_map(|m| m.file) {
        let segments: Vec<&str> = file.split('/').collect();
        for depth in 1..segments.len() {
            *counts.entry(segments[..depth].to_vec()).or_insert(0) += 1;
        }
    }
    let deepest = counts
        .into_iter()
        .filter(|(_, count)| count * 2 > members.len())
        .map(|(dir, _)| dir)
        .max_by_key(Vec::len)?;
    let kept: Vec<&str> = deepest
        .into_iter()
        .filter(|segment| !SKIPPED_SEGMENTS.contains(segment))
        .collect();
    (!kept.is_empty()).then(|| kept.join("/"))
}

/// The words every member name starts with, lowercase and joined by `_`.
fn common_prefix(members: &[Member<'_>]) -> Option<String> {
    let split: Vec<String> = members.iter().map(|m| name_split(m.name)).collect();
    let mut names = split.iter().map(|s| s.split(' ').collect::<Vec<_>>());
    let mut prefix = names.next()?;
    for words in names {
        let shared = prefix
            .iter()
            .zip(&words)
            .take_while(|(a, b)| a == b)
            .count();
        prefix.truncate(shared);
    }
    prefix.retain(|w| !w.is_empty());
    (!prefix.is_empty()).then(|| prefix.join("_"))
}
