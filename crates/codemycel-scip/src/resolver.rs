use std::collections::BTreeMap;

use codemycel_core::{
    Candidate, NodeRef, Occurrence, RepoPath, Resolver, ResolverSource, SourceFile,
    UnresolvedReason, Verdict,
};

use crate::ScipIndex;
use crate::mapping::FileLines;

/// A SCIP index bound to the parsed files, answering for the files it covers. Reference
/// occurrences resolve candidates at the same position; a candidate with no occurrence is
/// `no_definition`. Calls come through candidates, so `outgoing_calls` is unsupported.
pub struct ScipResolver<'a> {
    index: &'a ScipIndex,
    files: BTreeMap<&'a RepoPath, FileLines<'a>>,
}

impl ResolverSource for ScipIndex {
    fn bind<'a>(&'a self, files: &'a [SourceFile<'a>]) -> Box<dyn Resolver + 'a> {
        Box::new(ScipResolver {
            index: self,
            files: files.iter().map(|f| (f.path, FileLines::new(f))).collect(),
        })
    }
}

impl ScipResolver<'_> {
    fn verdict(&self, candidate: &Candidate) -> Verdict {
        let no_definition = Verdict::Unresolved {
            reason: UnresolvedReason::NoDefinition,
        };
        let (Some(document), Some(file)) = (
            self.index.documents.get(&candidate.file),
            self.files.get(&candidate.file),
        ) else {
            return no_definition;
        };
        let row = candidate.at.line.saturating_sub(1);
        let Some(unit) = file.units(row, candidate.at.col, document.encoding) else {
            return no_definition;
        };
        let occurrence = document
            .occurrences
            .iter()
            .find(|o| !o.definition && o.range.contains(row, unit));
        match occurrence {
            Some(occurrence) => self.definition_of(&occurrence.symbol),
            None => no_definition,
        }
    }

    /// The node defining `symbol`; a symbol the index never defines is external.
    fn definition_of(&self, symbol: &str) -> Verdict {
        let mut reason = UnresolvedReason::ExternalDependency;
        for (path, range) in self.index.definitions.get(symbol).into_iter().flatten() {
            let Some(file) = self.files.get(path) else {
                reason = UnresolvedReason::UnindexedFile;
                continue;
            };
            let Some(document) = self.index.documents.get(path) else {
                continue;
            };
            match file.node_at(*range, document.encoding) {
                Some(target) => return Verdict::Confirmed { target },
                None => reason = UnresolvedReason::Unmapped,
            }
        }
        Verdict::Unresolved { reason }
    }

    /// The global symbol whose definition occurrence lies in `node`'s selection.
    fn symbol_of(&self, node: &NodeRef) -> Option<&str> {
        let path = RepoPath::new(node.path.file()?).ok()?;
        let file = self.files.get(&path)?;
        let document = self.index.documents.get(&path)?;
        let selection = file.node(node)?.selection?;
        document
            .occurrences
            .iter()
            .filter(|o| o.definition)
            .find(|o| {
                file.offset(o.range.start_line, o.range.start, document.encoding)
                    .is_some_and(|at| selection.start <= at && at <= selection.end)
            })
            .map(|o| o.symbol.as_str())
    }
}

impl Resolver for ScipResolver<'_> {
    fn name(&self) -> &'static str {
        "scip"
    }

    fn covers(&self, file: &RepoPath) -> bool {
        self.index.covers(file)
    }

    fn resolve(&self, candidates: &[Candidate]) -> Vec<Verdict> {
        candidates.iter().map(|c| self.verdict(c)).collect()
    }

    fn implementers(&self, node: &NodeRef) -> Vec<NodeRef> {
        let Some(symbol) = self.symbol_of(node) else {
            return Vec::new();
        };
        let mut found: Vec<NodeRef> = self
            .index
            .implementers
            .get(symbol)
            .into_iter()
            .flatten()
            .filter_map(|implementer| match self.definition_of(implementer) {
                Verdict::Confirmed { target } => Some(target),
                Verdict::Unresolved { .. } => None,
            })
            .collect();
        found.sort();
        found.dedup();
        found
    }

    fn outgoing_calls(&self, _: &NodeRef) -> Option<Vec<(NodeRef, Vec<Occurrence>)>> {
        None
    }
}
