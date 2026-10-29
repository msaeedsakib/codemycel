use std::collections::BTreeMap;

use super::status::root_label;
use super::{Answer, Budget, Line, Outcome, QueryEngine, Result, Section, UnresolvedLine};
use crate::store::{StoreRead, StoredUnresolved};
use crate::{NodeKind, RepoPath, UnresolvedReason};

/// Files listed in the repository summary.
const TOP_FILES: usize = 20;

impl QueryEngine<'_> {
    /// Without `scope`, counts by reason and by root and the files with the most candidates.
    /// With a file path or a symbol, each of its candidates. Only `reason` when given.
    pub fn unresolved(
        &self,
        scope: Option<&str>,
        reason: Option<UnresolvedReason>,
        budget: Budget,
    ) -> Result<Answer> {
        let scope = scope.map(str::trim).filter(|scope| !scope.is_empty());
        match scope {
            None => self.unresolved_summary(reason, budget),
            Some(scope) => self.unresolved_in(scope, reason, budget),
        }
    }

    fn unresolved_summary(
        &self,
        reason: Option<UnresolvedReason>,
        budget: Budget,
    ) -> Result<Answer> {
        let store = self.store();
        let reasons: Vec<Line> = store
            .counts()?
            .unresolved
            .into_iter()
            .filter(|(r, _)| reason.is_none_or(|wanted| wanted == *r))
            .map(|(r, count)| Line::field(r.as_str(), count))
            .collect();
        let mut sections = Vec::new();
        if reasons.is_empty() {
            let filter = reason.map_or(String::new(), |r| format!(" with reason {r}"));
            let line = Line::text(format!("no unresolved candidates{filter}"));
            sections.push(Section::new("result", 0, vec![line]).without_header());
        }
        let labels: BTreeMap<_, _> = store
            .roots()?
            .iter()
            .map(|root| (root.id, root_label(root)))
            .collect();
        let mut by_root: BTreeMap<String, Vec<(UnresolvedReason, u64)>> = BTreeMap::new();
        for (root, r, count) in store.unresolved_counts_by_root(reason)? {
            let label = root
                .and_then(|id| labels.get(&id).cloned())
                .unwrap_or_else(|| "no root".to_owned());
            by_root.entry(label).or_default().push((r, count));
        }
        let roots = by_root
            .into_iter()
            .map(|(label, counts)| {
                let total: u64 = counts.iter().map(|(_, count)| count).sum();
                let parts: Vec<String> = counts
                    .iter()
                    .map(|(r, count)| format!("{r} {count}"))
                    .collect();
                Line::field(label, format!("{total} · {}", parts.join(" · ")))
            })
            .collect();
        let files = store
            .unresolved_counts_by_file(reason)?
            .into_iter()
            .map(|(path, count)| Line::field(path.as_str(), count))
            .collect();
        sections.extend([
            Section::new("reasons", 0, reasons),
            Section::new("roots", 1, roots),
            Section::limited("files", 2, files, TOP_FILES),
        ]);
        self.finish(
            Outcome::Ok,
            sections,
            budget,
            "pass a file or symbol as scope for its candidates",
        )
    }

    /// `scope` is an indexed file path first, then a node name.
    fn unresolved_in(
        &self,
        scope: &str,
        reason: Option<UnresolvedReason>,
        budget: Budget,
    ) -> Result<Answer> {
        let store = self.store();
        let in_file = match RepoPath::new(scope) {
            Ok(path) if store.file_by_path(&path)?.is_some() => Some(path),
            _ => None,
        };
        let rows = match in_file {
            Some(path) => store.unresolved_for_file(&path)?,
            None => match self.lookup(scope, budget)? {
                Ok(node) => match (&node.node.kind, &node.node.file) {
                    (NodeKind::File, Some(path)) => store.unresolved_for_file(path)?,
                    _ => store.unresolved_for_node(node.id)?,
                },
                Err(answer) => return Ok(answer),
            },
        };
        let lines: Vec<Line> = rows
            .into_iter()
            .filter(|row| reason.is_none_or(|wanted| wanted == row.unresolved.reason))
            .map(candidate_line)
            .collect();
        let mut sections = Vec::new();
        if lines.is_empty() {
            let filter = reason.map_or(String::new(), |r| format!(" with reason {r}"));
            let line = Line::text(format!("no unresolved candidates{filter} in `{scope}`"));
            sections.push(Section::new("result", 0, vec![line]).without_header());
        }
        sections.push(Section::new("unresolved", 0, lines));
        self.finish(
            Outcome::Ok,
            sections,
            budget,
            "raise max_tokens or filter by reason",
        )
    }
}

fn candidate_line(stored: StoredUnresolved) -> Line {
    let candidate = stored.unresolved.candidate;
    Line::Unresolved(UnresolvedLine {
        file: candidate.file,
        line: candidate.at.line,
        col: candidate.at.col,
        role: candidate.role,
        text: candidate.text,
        reason: stored.unresolved.reason,
    })
}
