use std::path::Path;

use codemycel_core::{
    ByteRange, FileKind, FileRecord, Node, NodeKind, ParsedFile, Root, SemanticPath, SkipReason,
    Span,
};
use rayon::prelude::*;

use crate::{IndexError, Registry, RootDetector};

/// One parsed code file: its index in the walk, its file node, and what the adapter extracted.
pub struct ParsedCode {
    pub file: usize,
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
            Ok(ParsedCode {
                file: index,
                file_node: file_node(file, &bytes)?,
                parsed: adapter.parse(&file.path, &bytes)?,
            })
        })
        .collect::<Result<Vec<_>, IndexError>>()?;
    Ok((assignment, parsed))
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
