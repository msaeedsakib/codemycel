use super::{Answer, Budget, QueryEngine, Result};

impl QueryEngine<'_> {
    pub fn notes(&self, target: Option<&str>, orphans: bool, budget: Budget) -> Result<Answer> {
        let _ = (target, orphans);
        self.not_found("not implemented: notes", budget)
    }
}
