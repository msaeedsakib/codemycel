use super::{Answer, Budget, Line, Outcome, QueryEngine, Result, Section};
use crate::store::{StoreRead, StoredRoot, meta};
use crate::{Coverage, SkipReason, SyncOwner};

impl QueryEngine<'_> {
    /// The index's state: commit and time, files changed since, the lock owners, database size,
    /// each root's server and coverage, configuration diagnostics, counts, partially enriched
    /// files, and files skipped as binary or oversize. The owners come from the caller.
    pub fn status(
        &self,
        sync_owner: Option<SyncOwner>,
        session_owner: Option<SyncOwner>,
        budget: Budget,
    ) -> Result<Answer> {
        let store = self.store();
        let me = std::process::id();
        let session = match session_owner {
            None => "no session is watching".to_owned(),
            Some(owner) if owner.pid == me => format!("this session is watching (pid {me})"),
            Some(owner) => format!("another session is watching (pid {})", owner.pid),
        };
        let sync = match sync_owner {
            None => "idle".to_owned(),
            Some(owner) if owner.pid == me => format!("this session is syncing (pid {me})"),
            Some(owner) => format!("another process owns sync (pid {})", owner.pid),
        };
        let index = vec![
            Line::field(
                "commit",
                store
                    .meta_get(meta::INDEXED_COMMIT)?
                    .unwrap_or_else(|| "no git".to_owned()),
            ),
            Line::field(
                "indexed",
                store
                    .meta_get(meta::INDEXED_AT)?
                    .unwrap_or_else(|| "never".to_owned()),
            ),
            Line::field(
                "changed",
                format!("{} since indexing", self.changed_files()?),
            ),
            Line::field("session", session),
            Line::field("sync", sync),
            Line::field("database", bytes(store.database_size()?)),
        ];
        let roots = store.roots()?;
        let root_lines = roots
            .iter()
            .map(|root| Line::field(root_label(root), root_state(root, true)))
            .collect();
        let diagnostics = roots
            .iter()
            .flat_map(|root| {
                let label = root_label(root);
                root.status
                    .iter()
                    .flat_map(|status| &status.config_diagnostics)
                    .map(move |diagnostic| Line::field(label.clone(), diagnostic))
            })
            .collect();
        let partial = store
            .partially_enriched_files()?
            .into_iter()
            .map(|path| Line::text(path.as_str()))
            .collect();
        let skipped = store
            .files()?
            .into_iter()
            .filter_map(|file| match file.record.skip_reason {
                Some(reason @ (SkipReason::Binary | SkipReason::Oversize)) => {
                    Some(Line::field(file.record.path.as_str(), reason))
                }
                _ => None,
            })
            .collect();
        let sections = vec![
            Section::new("index", 0, index).without_header(),
            Section::new("roots", 0, root_lines),
            Section::new("diagnostics", 1, diagnostics),
            Section::new("counts", 1, self.count_lines()?),
            Section::new("partially_enriched", 2, partial),
            Section::new("skipped", 3, skipped),
        ];
        self.finish(Outcome::Ok, sections, budget, "raise max_tokens")
    }

    /// Indexed files whose content on disk differs from the indexed content, or that are gone.
    fn changed_files(&self) -> Result<String> {
        let mut changed = 0;
        for file in self.store().files()? {
            let current = std::fs::read(self.repo_root().join(file.record.path.as_str()))
                .ok()
                .map(|bytes| blake3::hash(&bytes));
            if current != Some(file.record.hash) {
                changed += 1;
            }
        }
        Ok(super::impact::plural(changed, "file"))
    }

    /// Files, nodes, edges by kind, and unresolved candidates by reason.
    pub(super) fn count_lines(&self) -> Result<Vec<Line>> {
        let store = self.store();
        let files = store.files()?;
        let parsed = files.iter().filter(|f| f.record.parsed).count();
        let skipped = files
            .iter()
            .filter(|f| f.record.skip_reason.is_some())
            .count();
        let counts = store.counts()?;
        let joined = |pairs: Vec<(String, u64)>| {
            if pairs.is_empty() {
                return "none".to_owned();
            }
            pairs
                .iter()
                .map(|(key, count)| format!("{key} {count}"))
                .collect::<Vec<_>>()
                .join(" · ")
        };
        Ok(vec![
            Line::field(
                "files",
                format!("{} · {parsed} parsed · {skipped} skipped", files.len()),
            ),
            Line::field("nodes", counts.nodes),
            Line::field(
                "edges",
                joined(
                    counts
                        .edges
                        .iter()
                        .map(|(kind, count)| (kind.to_string(), *count))
                        .collect(),
                ),
            ),
            Line::field(
                "unresolved",
                joined(
                    counts
                        .unresolved
                        .iter()
                        .map(|(reason, count)| (reason.to_string(), *count))
                        .collect(),
                ),
            ),
        ])
    }
}

/// `<root path> (<language>)`, with `.` for the repository root.
pub(super) fn root_label(root: &StoredRoot) -> String {
    let path = if root.root.path.is_root() {
        "."
    } else {
        root.root.path.as_str()
    };
    format!("{path} ({})", root.root.language)
}

/// The root's coverage, `lsp ✓`, `degraded: <reason>` or `not verified`; with `server`, led by
/// the server's name, version and source when one ran.
pub(super) fn root_state(root: &StoredRoot, server: bool) -> String {
    let Some(status) = &root.status else {
        return "not verified".to_owned();
    };
    let coverage = match (status.degraded, status.coverage) {
        (Some(reason), _) => format!("degraded: {reason}"),
        (None, Coverage::ScipAndLsp) => "scip ✓ lsp ✓".to_owned(),
        (None, Coverage::None) => "none".to_owned(),
        (None, by) => format!("{by} ✓"),
    };
    match (&status.server, server) {
        (Some(info), true) => format!(
            "{} {} ({}) · {coverage}",
            info.name, info.version, info.source
        ),
        _ => coverage,
    }
}

fn bytes(size: u64) -> String {
    const UNITS: [&str; 4] = ["KiB", "MiB", "GiB", "TiB"];
    if size < 1024 {
        return format!("{size} B");
    }
    let mut value = size as f64 / 1024.0;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}
