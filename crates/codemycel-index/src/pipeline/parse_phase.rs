use std::collections::BTreeSet;
use std::path::Path;

use codemycel_core::{
    ByteRange, Candidate, CandidateRole, FileKind, FileRecord, Node, NodeKind, ParsedFile, Root,
    SemanticPath, SkipReason, Span,
};
use rayon::prelude::*;

use crate::{IndexError, Registry, RootDetector};

/// One parsed code file: its index in the walk, its text, its file node, and what the adapter
/// extracted.
pub struct ParsedCode {
    pub file: usize,
    pub text: String,
    pub file_node: Node,
    pub parsed: ParsedFile,
}

/// Assigns each code file its root, marks rootless ones `no_root`, and parses the rest in
/// parallel. Results come back in walk order.
pub fn run(
    repo_root: &Path,
    files: &mut [FileRecord],
    roots: &RootDetector,
    registry: &Registry,
) -> Result<(Vec<Option<Root>>, Vec<ParsedCode>), IndexError> {
    let assignment: Vec<Option<Root>> = files
        .iter()
        .map(|file| match (file.kind, file.language) {
            (FileKind::Code, Some(language)) => roots.assign(&file.path, language).cloned(),
            _ => None,
        })
        .collect();
    for (file, root) in files.iter_mut().zip(&assignment) {
        if file.kind == FileKind::Code && file.skip_reason.is_none() && root.is_none() {
            file.skip_reason = Some(SkipReason::NoRoot);
        }
    }
    let parsed = files
        .par_iter()
        .enumerate()
        .filter(|(_, file)| file.kind == FileKind::Code && file.skip_reason.is_none())
        .filter_map(|(index, file)| {
            let adapter = registry.for_language(file.language?)?;
            Some((index, file, adapter))
        })
        .map(|(index, file, adapter)| {
            let absolute = repo_root.join(file.path.as_str());
            let bytes = std::fs::read(&absolute).map_err(|source| IndexError::Io {
                path: absolute,
                source,
            })?;
            let parsed = adapter.parse(&file.path, &bytes)?;
            let text = String::from_utf8_lossy(&bytes).into_owned();
            let mut file_node = file_node(file, &bytes)?;
            file_node.body_hash = surface_hash(&text, &parsed);
            Ok(ParsedCode {
                file: index,
                file_node,
                parsed,
                text,
            })
        })
        .collect::<Result<Vec<_>, IndexError>>()?;
    Ok((assignment, parsed))
}

/// An import or export candidate outside every function and method body: one that can change
/// what the file, or the modules below it, expose to the rest of the repository.
pub fn is_surface(candidate: &Candidate) -> bool {
    matches!(
        candidate.role,
        CandidateRole::Import | CandidateRole::Export
    ) && !matches!(
        candidate.enclosing.kind,
        NodeKind::Function | NodeKind::Method
    )
}

/// A hash of the file's import and export statements: for every surface candidate, the stretch
/// of text between the symbols before and after it, whitespace dropped. `None` without one.
fn surface_hash(text: &str, parsed: &ParsedFile) -> Option<blake3::Hash> {
    let line_starts: Vec<usize> = std::iter::once(0)
        .chain(text.match_indices('\n').map(|(at, _)| at + 1))
        .collect();
    let spans: Vec<(usize, usize)> = parsed
        .nodes
        .iter()
        .filter_map(|node| node.span)
        .map(|span| (span.bytes.start as usize, span.bytes.end as usize))
        .collect();
    let stretches: BTreeSet<(usize, usize)> = parsed
        .candidates
        .iter()
        .filter(|candidate| is_surface(candidate))
        .filter_map(|candidate| {
            let line_start = *line_starts.get((candidate.at.line as usize).checked_sub(1)?)?;
            let (column, _) = text
                .get(line_start..)?
                .char_indices()
                .nth(candidate.at.col as usize)?;
            let at = line_start + column;
            let start = spans.iter().map(|s| s.1).filter(|end| *end <= at).max();
            let end = spans.iter().map(|s| s.0).filter(|start| *start > at).min();
            Some((start.unwrap_or(0), end.unwrap_or(text.len())))
        })
        .collect();
    if stretches.is_empty() {
        return None;
    }
    let mut hasher = blake3::Hasher::new();
    for (start, end) in stretches {
        let stretch: String = text
            .get(start..end)
            .unwrap_or_default()
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        hasher.update(stretch.as_bytes());
        hasher.update(&[0]);
    }
    Some(hasher.finalize())
}

fn file_node(file: &FileRecord, bytes: &[u8]) -> Result<Node, IndexError> {
    let newlines = bytes.iter().filter(|&&b| b == b'\n').count();
    let unterminated = usize::from(bytes.last().is_some_and(|&b| b != b'\n'));
    let lines = u32::try_from((newlines + unterminated).max(1)).unwrap_or(u32::MAX);
    Ok(Node {
        path: SemanticPath::for_file(&file.path)?,
        kind: NodeKind::File,
        name: file.path.as_str().to_owned(),
        file: Some(file.path.clone()),
        span: Some(Span {
            bytes: ByteRange {
                start: 0,
                end: u32::try_from(bytes.len()).unwrap_or(u32::MAX),
            },
            line_start: 1,
            line_end: lines,
        }),
        selection: None,
        signature: None,
        has_explicit_return_type: false,
        body_hash: None,
        doc_comment: None,
        visibility: None,
    })
}
