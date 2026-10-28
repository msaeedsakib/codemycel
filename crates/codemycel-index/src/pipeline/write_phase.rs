use std::collections::{BTreeMap, BTreeSet};

use codemycel_core::store::Tx;
use codemycel_core::text::name_split;
use codemycel_core::{FileRecord, Node, RepoPath, Root, SearchDoc, Unresolved, UnresolvedReason};

use super::parse_phase::ParsedCode;
use crate::IndexError;

/// Writes roots, files, file and symbol nodes, impl records, structural edges, and search docs,
/// in that order. Candidates are written by the verify phase, or by `placeholders`.
pub fn run(
    tx: &mut Tx<'_>,
    files: &[FileRecord],
    roots: &[Root],
    assignment: &[Option<Root>],
    parsed: &[ParsedCode],
) -> Result<(), IndexError> {
    for record in records(tx, files, roots, assignment, parsed)? {
        tx.upsert_file(&record)?;
    }
    let file_nodes: Vec<Node> = parsed.iter().map(|code| code.file_node.clone()).collect();
    tx.insert_nodes(&file_nodes)?;
    for code in parsed {
        tx.insert_nodes(&code.parsed.nodes)?;
    }
    for code in parsed {
        tx.insert_impl_records(&code.parsed.impl_records)?;
    }
    for code in parsed {
        tx.insert_edges(&code.parsed.structural_edges)?;
    }
    let search_docs: Vec<SearchDoc> = parsed
        .iter()
        .flat_map(|code| std::iter::once(&code.file_node).chain(&code.parsed.nodes))
        .map(search_doc)
        .collect();
    tx.insert_search_docs(&search_docs)
        .map_err(IndexError::from)
}

/// The walk's records with their root ids, which stores the roots, and parsed flags.
pub fn records(
    tx: &mut Tx<'_>,
    files: &[FileRecord],
    roots: &[Root],
    assignment: &[Option<Root>],
    parsed: &[ParsedCode],
) -> Result<Vec<FileRecord>, IndexError> {
    let mut root_ids = BTreeMap::new();
    for root in roots {
        root_ids.insert(root, tx.set_root(root)?);
    }
    let parsed_files: BTreeSet<usize> = parsed.iter().map(|code| code.file).collect();
    Ok(files
        .iter()
        .enumerate()
        .map(|(index, file)| {
            let mut record = file.clone();
            record.root_id = assignment[index]
                .as_ref()
                .and_then(|root| root_ids.get(root).copied());
            record.parsed = parsed_files.contains(&index);
            record
        })
        .collect())
}

/// Rewrites a code file's nodes from its parse, or removes them when it is no longer parsed.
pub fn replace_file(
    tx: &mut Tx<'_>,
    path: &RepoPath,
    code: Option<&ParsedCode>,
) -> Result<(), IndexError> {
    let Some(code) = code else {
        return Ok(tx.replace_file_nodes(path, &[], &[], &[], &[])?);
    };
    let nodes: Vec<Node> = std::iter::once(&code.file_node)
        .chain(&code.parsed.nodes)
        .cloned()
        .collect();
    let search_docs: Vec<SearchDoc> = nodes.iter().map(search_doc).collect();
    Ok(tx.replace_file_nodes(
        path,
        &nodes,
        &code.parsed.impl_records,
        &code.parsed.structural_edges,
        &search_docs,
    )?)
}

/// Stores every candidate as `server_unavailable`, for a run in which no resolver runs.
pub fn placeholders(tx: &mut Tx<'_>, parsed: &[ParsedCode]) -> Result<(), IndexError> {
    for code in parsed {
        let rows: Vec<Unresolved> = code
            .parsed
            .candidates
            .iter()
            .map(|candidate| Unresolved {
                candidate: candidate.clone(),
                reason: UnresolvedReason::ServerUnavailable,
            })
            .collect();
        tx.insert_unresolved(&rows)?;
    }
    Ok(())
}

pub fn search_doc(node: &Node) -> SearchDoc {
    SearchDoc {
        node: node.node_ref(),
        name: node.name.clone(),
        name_split: name_split(&node.name),
        signature: node.signature.clone(),
        doc_comment: node.doc_comment.clone(),
        path: node.file.clone(),
    }
}
