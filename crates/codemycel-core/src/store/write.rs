use std::collections::BTreeSet;

use rusqlite::{OptionalExtension, params};

use super::error::Result;
use super::read::{file_key, node_key};
use super::{StoreError, Tx};
use crate::{
    Edge, FileId, FileRecord, ImplRecord, Node, NodeId, Occurrence, RepoPath, Root, RootId,
    SearchDoc, Unresolved,
};

impl Tx<'_> {
    pub fn meta_set(&mut self, key: &str, value: &str) -> Result<()> {
        self.tx.execute(
            "INSERT INTO meta (key, value) VALUES (?1, ?2)
             ON CONFLICT (key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    /// Returns the existing id when a root with the same path and language is already stored.
    pub fn set_root(&mut self, root: &Root) -> Result<RootId> {
        let id = self.tx.query_row(
            "INSERT INTO roots (path, language) VALUES (?1, ?2)
             ON CONFLICT (path, language) DO UPDATE SET path = excluded.path
             RETURNING id",
            params![root.path, root.language],
            |row| row.get(0),
        )?;
        Ok(RootId(id))
    }

    /// Inserts or updates the row for `file.path`, keeping its id and everything that hangs off it.
    pub fn upsert_file(&mut self, file: &FileRecord) -> Result<FileId> {
        let size = i64::try_from(file.size).map_err(|_| StoreError::OutOfRange("file size"))?;
        let id = self.tx.query_row(
            "INSERT INTO files (path, root_id, hash, size, language, kind, parsed, skip_reason,
                                scip_covered)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT (path) DO UPDATE SET
                root_id = excluded.root_id, hash = excluded.hash, size = excluded.size,
                language = excluded.language, kind = excluded.kind, parsed = excluded.parsed,
                skip_reason = excluded.skip_reason, scip_covered = excluded.scip_covered
             RETURNING id",
            params![
                file.path,
                file.root_id.map(|r| r.0),
                file.hash.as_bytes().as_slice(),
                size,
                file.language,
                file.kind,
                file.parsed,
                file.skip_reason,
                file.scip_covered,
            ],
            |row| row.get(0),
        )?;
        Ok(FileId(id))
    }

    /// Deletes the file row; foreign keys cascade to everything it owns. `name_counts` is the
    /// caller's to adjust.
    pub fn delete_file(&mut self, path: &RepoPath) -> Result<()> {
        self.tx
            .execute("DELETE FROM files WHERE path = ?1", params![path])?;
        Ok(())
    }

    pub fn insert_nodes(&mut self, nodes: &[Node]) -> Result<Vec<NodeId>> {
        let mut insert = self.tx.prepare_cached(
            "INSERT INTO nodes (semantic_path, kind, name, file_id, root_id, byte_start, byte_end,
                                line_start, line_end, sel_start, sel_end, signature,
                                has_explicit_return_type, body_hash, doc_comment, visibility)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
        )?;
        let mut ids = Vec::with_capacity(nodes.len());
        for node in nodes {
            let (file_id, root_id) = match &node.file {
                Some(path) => {
                    let (file_id, root_id) = file_key(&self.tx, path)?;
                    (Some(file_id.0), root_id.map(|r| r.0))
                }
                None => (None, None),
            };
            insert.execute(params![
                node.path,
                node.kind,
                node.name,
                file_id,
                root_id,
                node.span.map(|s| s.bytes.start),
                node.span.map(|s| s.bytes.end),
                node.span.map(|s| s.line_start),
                node.span.map(|s| s.line_end),
                node.selection.map(|s| s.start),
                node.selection.map(|s| s.end),
                node.signature,
                node.has_explicit_return_type,
                node.body_hash.as_ref().map(|h| h.as_bytes().as_slice()),
                node.doc_comment,
                node.visibility,
            ])?;
            ids.push(NodeId(self.tx.last_insert_rowid()));
        }
        Ok(ids)
    }

    /// Upserts on (src, dst, kind); occurrences merge as a sorted, deduplicated union.
    pub fn insert_edges(&mut self, edges: &[Edge]) -> Result<()> {
        for edge in edges {
            let src = node_key(&self.tx, &edge.src)?;
            let dst = node_key(&self.tx, &edge.dst)?;
            let root_id: Option<i64> =
                self.tx
                    .query_row("SELECT root_id FROM nodes WHERE id = ?1", [src.0], |row| {
                        row.get(0)
                    })?;
            let existing: Option<String> = self
                .tx
                .query_row(
                    "SELECT occurrences FROM edges WHERE src = ?1 AND dst = ?2 AND kind = ?3",
                    params![src.0, dst.0, edge.kind],
                    |row| row.get(0),
                )
                .optional()?;
            let mut merged: BTreeSet<Occurrence> = edge.occurrences.iter().copied().collect();
            if let Some(existing) = existing {
                merged.extend(serde_json::from_str::<Vec<Occurrence>>(&existing)?);
            }
            let occurrences = serde_json::to_string(&merged)?;
            self.tx.execute(
                "INSERT INTO edges (src, dst, kind, verified_by, root_id, occurrences)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT (src, dst, kind) DO UPDATE SET
                    verified_by = excluded.verified_by, root_id = excluded.root_id,
                    occurrences = excluded.occurrences",
                params![
                    src.0,
                    dst.0,
                    edge.kind,
                    edge.verified_by,
                    root_id,
                    occurrences
                ],
            )?;
        }
        Ok(())
    }

    pub fn insert_unresolved(&mut self, rows: &[Unresolved]) -> Result<()> {
        let mut insert = self.tx.prepare_cached(
            "INSERT INTO unresolved (file_id, node_id, text, role, line, col, reason)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        )?;
        for row in rows {
            let candidate = &row.candidate;
            let (file_id, _) = file_key(&self.tx, &candidate.file)?;
            let node_id = node_key(&self.tx, &candidate.enclosing)?;
            insert.execute(params![
                file_id.0,
                node_id.0,
                candidate.text,
                candidate.role,
                candidate.at.line,
                candidate.at.col,
                row.reason,
            ])?;
        }
        Ok(())
    }

    pub fn insert_impl_records(&mut self, records: &[ImplRecord]) -> Result<()> {
        let mut insert = self.tx.prepare_cached(
            "INSERT INTO impl_records (file_id, self_type_text, trait_text, byte_start, byte_end,
                                       line_start, line_end, header_start, header_end,
                                       body_start, body_end, self_type_sel_start,
                                       self_type_sel_end)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        )?;
        for record in records {
            let (file_id, _) = file_key(&self.tx, &record.file)?;
            insert.execute(params![
                file_id.0,
                record.self_type_text,
                record.trait_text,
                record.span.bytes.start,
                record.span.bytes.end,
                record.span.line_start,
                record.span.line_end,
                record.header.start,
                record.header.end,
                record.body.start,
                record.body.end,
                record.self_type_selection.start,
                record.self_type_selection.end,
            ])?;
        }
        Ok(())
    }

    /// One row per node; a second doc for the same node replaces the first.
    pub fn insert_search_docs(&mut self, docs: &[SearchDoc]) -> Result<()> {
        let mut upsert = self.tx.prepare_cached(
            "INSERT INTO search_docs (node_id, name, name_split, signature, doc_comment, path)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT (node_id) DO UPDATE SET
                name = excluded.name, name_split = excluded.name_split,
                signature = excluded.signature, doc_comment = excluded.doc_comment,
                path = excluded.path",
        )?;
        for doc in docs {
            let node_id = node_key(&self.tx, &doc.node)?;
            upsert.execute(params![
                node_id.0,
                doc.name,
                doc.name_split,
                doc.signature,
                doc.doc_comment,
                doc.path,
            ])?;
        }
        Ok(())
    }

    /// Adds `delta` to the name's count; the row is removed when the count reaches zero.
    pub fn name_counts_add(&mut self, name: &str, delta: i64) -> Result<()> {
        self.tx.execute(
            "INSERT INTO name_counts (name, count) VALUES (?1, ?2)
             ON CONFLICT (name) DO UPDATE SET count = count + excluded.count",
            params![name, delta],
        )?;
        self.tx.execute(
            "DELETE FROM name_counts WHERE name = ?1 AND count <= 0",
            params![name],
        )?;
        Ok(())
    }
}
