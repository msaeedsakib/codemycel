mod docs_phase;
mod parse_phase;
mod write_phase;

use std::path::Path;

use codemycel_core::config::Config;
use codemycel_core::store::Store;
use codemycel_core::{RepoPath, SkipReason};

pub use docs_phase::DocsCounts;

use crate::{IndexError, Registry, RootDetector, Walker};

/// What a pipeline run did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PipelineReport {
    pub files: usize,
    pub parsed: usize,
    pub skipped: Vec<(RepoPath, SkipReason)>,
    pub roots: usize,
    /// File and symbol nodes of parsed code files.
    pub nodes: usize,
    pub structural_edges: usize,
    pub candidates: usize,
    pub docs: DocsCounts,
}

pub struct Pipeline;

impl Pipeline {
    /// Walks, detects roots, parses every rooted code file, then the documents and manifests,
    /// and writes the syntactic graph into an empty store in one transaction (spec §5.3 without
    /// the resolvers).
    pub fn parse_only(
        repo_root: &Path,
        config: &Config,
        store: &mut Store,
        registry: &Registry,
    ) -> Result<PipelineReport, IndexError> {
        let mut files = Walker::new(repo_root, config, registry).walk()?;
        let listed: Vec<RepoPath> = files.iter().map(|file| file.path.clone()).collect();
        let roots = RootDetector::detect(repo_root, &listed, config, registry)?;
        let (assignment, parsed) = parse_phase::run(repo_root, &mut files, &roots, registry)?;
        let docs = docs_phase::prepare(repo_root, &files)?;
        let code_files: Vec<RepoPath> = parsed
            .iter()
            .map(|code| files[code.file].path.clone())
            .collect();
        let docs = store.transaction(|tx| {
            write_phase::run(tx, &files, roots.roots(), &assignment, &parsed)?;
            docs_phase::write(tx, &docs, &code_files)
        })?;
        Ok(PipelineReport {
            files: files.len(),
            parsed: parsed.len(),
            skipped: files
                .iter()
                .filter_map(|file| Some((file.path.clone(), file.skip_reason?)))
                .collect(),
            roots: roots.roots().len(),
            nodes: parsed.iter().map(|code| 1 + code.parsed.nodes.len()).sum(),
            structural_edges: parsed
                .iter()
                .map(|code| code.parsed.structural_edges.len())
                .sum(),
            candidates: parsed.iter().map(|code| code.parsed.candidates.len()).sum(),
            docs,
        })
    }
}
