use super::{Answer, Budget, QueryEngine, Result};

impl QueryEngine<'_> {
    pub fn overview(&self, scope: Option<&str>, budget: Budget) -> Result<Answer> {
        let _ = scope;
        self.not_found("not implemented: overview", budget)
    }
}
