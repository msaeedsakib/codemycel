mod docs_phase;
mod ownership;
mod parse_phase;
mod verify_phase;
mod write_phase;

use std::path::Path;

use codemycel_core::config::Config;
use codemycel_core::store::Store;
use codemycel_core::{FileRecord, RepoPath, Root, RootStatus, SkipReason};

pub use docs_phase::DocsCounts;
pub use verify_phase::Sources;

use crate::{IndexError, Registry, RootDetector, Walker};
use parse_phase::ParsedCode;

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
    /// Per root, what the verify phase recorded; empty for a parse-only run.
    pub statuses: Vec<(Root, RootStatus)>,
}

/// The walk, the roots, and the parsed code and documents, before anything is written.
struct Staged {
    files: Vec<FileRecord>,
    roots: RootDetector,
    assignment: Vec<Option<Root>>,
    parsed: Vec<ParsedCode>,
    docs: docs_phase::Prepared,
}

impl Staged {
    fn new(repo_root: &Path, config: &Config, registry: &Registry) -> Result<Self, IndexError> {
        let mut files = Walker::new(repo_root, config, registry).walk()?;
        let listed: Vec<RepoPath> = files.iter().map(|file| file.path.clone()).collect();
        let roots = RootDetector::detect(repo_root, &listed, config, registry)?;
        let (assignment, parsed) = parse_phase::run(repo_root, &mut files, &roots, registry)?;
        let docs = docs_phase::prepare(repo_root, &files)?;
        Ok(Self {
            files,
            roots,
            assignment,
            parsed,
            docs,
        })
    }

    fn code_files(&self) -> Vec<RepoPath> {
        self.parsed
            .iter()
            .map(|code| self.files[code.file].path.clone())
            .collect()
    }

    fn report(&self, docs: DocsCounts, statuses: Vec<(Root, RootStatus)>) -> PipelineReport {
        PipelineReport {
            files: self.files.len(),
            parsed: self.parsed.len(),
            skipped: self
                .files
                .iter()
                .filter_map(|file| Some((file.path.clone(), file.skip_reason?)))
                .collect(),
            roots: self.roots.roots().len(),
            nodes: self
                .parsed
                .iter()
                .map(|code| 1 + code.parsed.nodes.len())
                .sum(),
            structural_edges: self
                .parsed
                .iter()
                .map(|code| code.parsed.structural_edges.len())
                .sum(),
            candidates: self
                .parsed
                .iter()
                .map(|code| code.parsed.candidates.len())
                .sum(),
            docs,
            statuses,
        }
    }
}

pub struct Pipeline;

impl Pipeline {
    /// Walks, detects roots, parses every rooted code file, then the documents and manifests,
    /// and writes the syntactic graph into an empty store in one transaction. No resolver runs,
    /// so every candidate is stored as `server_unavailable`.
    pub fn parse_only(
        repo_root: &Path,
        config: &Config,
        store: &mut Store,
        registry: &Registry,
    ) -> Result<PipelineReport, IndexError> {
        let staged = Staged::new(repo_root, config, registry)?;
        let code_files = staged.code_files();
        let docs = store.transaction(|tx| {
            write_phase::run(
                tx,
                &staged.files,
                staged.roots.roots(),
                &staged.assignment,
                &staged.parsed,
            )?;
            write_phase::placeholders(tx, &staged.parsed)?;
            docs_phase::write(tx, &staged.docs, &code_files)
        })?;
        Ok(staged.report(docs, Vec::new()))
    }

    /// `parse_only`'s stages, then the verify phase: each file's candidates go to the resolver
    /// that owns it, and everything is written into an empty store in one transaction. Servers
    /// run before the transaction opens.
    pub fn run(
        repo_root: &Path,
        config: &Config,
        store: &mut Store,
        registry: &Registry,
        sources: &Sources<'_>,
    ) -> Result<PipelineReport, IndexError> {
        let mut staged = Staged::new(repo_root, config, registry)?;
        let code_files = staged.code_files();
        let verified = verify_phase::run(
            repo_root,
            config,
            staged.roots.roots(),
            &staged
                .files
                .iter()
                .map(|f| f.path.clone())
                .collect::<Vec<_>>(),
            &staged.assignment,
            &staged.parsed,
            registry,
            sources,
        )?;
        for file in &mut staged.files {
            file.scip_covered = verified.scip_covered.contains(&file.path);
        }
        let docs = store.transaction(|tx| {
            write_phase::run(
                tx,
                &staged.files,
                staged.roots.roots(),
                &staged.assignment,
                &staged.parsed,
            )?;
            let docs = docs_phase::write(tx, &staged.docs, &code_files)?;
            verify_phase::write(tx, &verified)?;
            Ok::<_, IndexError>(docs)
        })?;
        Ok(staged.report(docs, verified.statuses))
    }
}
