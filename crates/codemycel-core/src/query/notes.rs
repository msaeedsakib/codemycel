use super::{Answer, Budget, Line, NoteLine, Outcome, QueryEngine, Result, Section};
use crate::store::{StoreRead, StoredNote};

/// The target of a note on the repository as a whole.
const REPOSITORY: &str = "repo";

impl QueryEngine<'_> {
    /// Every note by uuid; with `target`, only the notes on it; with `orphans`, only notes whose
    /// target no longer exists. A target written exactly as a note stores it (`repo`, or the
    /// semantic path of a node that is gone) matches without a lookup.
    pub fn notes(&self, target: Option<&str>, orphans: bool, budget: Budget) -> Result<Answer> {
        let target = target.map(str::trim).filter(|target| !target.is_empty());
        let mut notes: Vec<StoredNote> = self.store().notes()?;
        if let Some(target) = target {
            let written =
                target == REPOSITORY || notes.iter().any(|n| n.note.target_semantic_path == target);
            let path = if written {
                target.to_owned()
            } else {
                match self.lookup(target, budget)? {
                    Ok(node) => node.node.path.to_string(),
                    Err(answer) => return Ok(answer),
                }
            };
            notes.retain(|note| note.note.target_semantic_path == path);
        }
        if orphans {
            notes.retain(|note| note.orphaned);
        }
        let lines: Vec<Line> = notes.iter().map(|n| Line::Note(NoteLine::of(n))).collect();
        let mut sections = Vec::new();
        if lines.is_empty() {
            let what = if orphans { "orphaned notes" } else { "notes" };
            let on = target.map_or(String::new(), |t| format!(" on `{t}`"));
            let line = Line::text(format!("no {what}{on}"));
            sections.push(Section::new("result", 0, vec![line]).without_header());
        }
        sections.push(Section::new("notes", 0, lines));
        self.finish(
            Outcome::Ok,
            sections,
            budget,
            "raise max_tokens or pass a target",
        )
    }
}
