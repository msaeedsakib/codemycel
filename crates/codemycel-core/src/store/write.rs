use std::collections::{BTreeMap, BTreeSet};

use rusqlite::{OptionalExtension, params};

use super::error::Result;
use super::read::{file_key, node_key};
use super::{NoteTarget, StoreError, Tx};
use crate::text::name_split;
use crate::{
    Community, Edge, EdgeKind, FileId, FileRecord, ImplRecord, LabelSource, Node, NodeId, NodeKind,
    NodeRef, Note, NoteId, Occurrence, Package, RepoPath, Root, RootId, RootStatus, SearchDoc,
    SemanticPath, Unresolved, VerifiedBy,
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

    pub fn meta_delete(&mut self, key: &str) -> Result<()> {
        self.tx.execute("DELETE FROM meta WHERE key = ?1", [key])?;
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

    pub fn set_root_status(&mut self, root: RootId, status: &RootStatus) -> Result<()> {
        let server = status.server.as_ref();
        self.tx.execute(
            "UPDATE roots SET server_name = ?2, server_version = ?3, server_source = ?4,
                              coverage = ?5, degraded_reason = ?6, config_diagnostics = ?7
             WHERE id = ?1",
            params![
                root.0,
                server.map(|s| &s.name),
                server.map(|s| &s.version),
                server.map(|s| s.source),
                status.coverage,
                status.degraded,
                serde_json::to_string(&status.config_diagnostics)?,
            ],
        )?;
        Ok(())
    }

    /// Marks a file whose server requests failed twice; `upsert_file` clears the mark.
    pub fn set_partially_enriched(&mut self, path: &RepoPath) -> Result<()> {
        self.tx.execute(
            "UPDATE files SET partially_enriched = 1 WHERE path = ?1",
            params![path],
        )?;
        Ok(())
    }

    /// Recomputes every node's `unresolved_out` and `external_out` from the unresolved table.
    pub fn refresh_unresolved_counts(&mut self) -> Result<()> {
        self.tx.execute(
            "UPDATE nodes SET
                unresolved_out = (SELECT count(*) FROM unresolved u WHERE u.node_id = nodes.id
                    AND u.reason IN ('no_definition', 'server_unavailable', 'unmapped')),
                external_out = (SELECT count(*) FROM unresolved u WHERE u.node_id = nodes.id
                    AND u.reason IN ('external_dependency', 'unindexed_file'))",
            [],
        )?;
        Ok(())
    }

    /// Inserts or updates the row for `file.path`, keeping its id and everything that hangs off it.
    /// A changed root moves the file's nodes and their outgoing edges to the new root.
    pub fn upsert_file(&mut self, file: &FileRecord) -> Result<FileId> {
        let size = i64::try_from(file.size).map_err(|_| StoreError::OutOfRange("file size"))?;
        let id = self.tx.query_row(
            "INSERT INTO files (path, root_id, hash, size, language, kind, parsed, skip_reason,
                                scip_covered)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT (path) DO UPDATE SET
                root_id = excluded.root_id, hash = excluded.hash, size = excluded.size,
                language = excluded.language, kind = excluded.kind, parsed = excluded.parsed,
                skip_reason = excluded.skip_reason, scip_covered = excluded.scip_covered,
                partially_enriched = 0
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
        let root_id = file.root_id.map(|r| r.0);
        self.tx.execute(
            "UPDATE nodes SET root_id = ?2 WHERE file_id = ?1 AND root_id IS NOT ?2",
            params![id, root_id],
        )?;
        self.tx.execute(
            "UPDATE edges SET root_id = ?2
             WHERE src IN (SELECT id FROM nodes WHERE file_id = ?1) AND root_id IS NOT ?2",
            params![id, root_id],
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

    /// One row per package node; the node must already be inserted.
    pub fn insert_packages(&mut self, packages: &[Package]) -> Result<()> {
        let mut insert = self.tx.prepare_cached(
            "INSERT INTO packages (node_id, name, version, external, manifest_file_id)
             VALUES (?1, ?2, ?3, ?4, ?5)",
        )?;
        for package in packages {
            let node_id = node_key(&self.tx, &package.node)?;
            let manifest = match &package.manifest {
                Some(path) => Some(file_key(&self.tx, path)?.0.0),
                None => None,
            };
            insert.execute(params![
                node_id.0,
                package.name,
                package.version,
                package.external,
                manifest,
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

    /// Makes the code file at `path` hold exactly `nodes`, `impl_records`, and the structural
    /// `contains`/`defines` edges `structural_edges`. Nodes are matched by identity, so a node
    /// that stays keeps its id and the edges other files hold to it; a node that goes takes its
    /// edges in both directions, unresolved rows, and search doc with it. Verified edges and
    /// unresolved rows of the nodes that stay are left for `reset_verification`.
    pub fn replace_file_nodes(
        &mut self,
        path: &RepoPath,
        nodes: &[Node],
        impl_records: &[ImplRecord],
        structural_edges: &[Edge],
        search_docs: &[SearchDoc],
    ) -> Result<()> {
        let (file_id, root_id) = file_key(&self.tx, path)?;
        let mut existing: BTreeMap<NodeRef, i64> = BTreeMap::new();
        {
            let mut stmt = self
                .tx
                .prepare_cached("SELECT id, semantic_path, kind FROM nodes WHERE file_id = ?1")?;
            let rows = stmt.query_map([file_id.0], |row| {
                Ok((
                    NodeRef::new(row.get(1)?, row.get(2)?),
                    row.get::<_, i64>(0)?,
                ))
            })?;
            for row in rows {
                let (node, id) = row?;
                existing.insert(node, id);
            }
        }
        let mut added = Vec::new();
        for node in nodes {
            match existing.remove(&node.node_ref()) {
                Some(id) => {
                    self.tx.execute(
                        "UPDATE nodes SET name = ?2, root_id = ?3, byte_start = ?4, byte_end = ?5,
                            line_start = ?6, line_end = ?7, sel_start = ?8, sel_end = ?9,
                            signature = ?10, has_explicit_return_type = ?11, body_hash = ?12,
                            doc_comment = ?13, visibility = ?14
                         WHERE id = ?1",
                        params![
                            id,
                            node.name,
                            root_id.map(|r| r.0),
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
                        ],
                    )?;
                }
                None => added.push(node.clone()),
            }
        }
        for id in existing.into_values() {
            self.tx.execute("DELETE FROM nodes WHERE id = ?1", [id])?;
        }
        self.insert_nodes(&added)?;
        self.tx
            .execute("DELETE FROM impl_records WHERE file_id = ?1", [file_id.0])?;
        self.insert_impl_records(impl_records)?;
        self.tx.execute(
            "DELETE FROM edges WHERE kind IN ('contains', 'defines')
                AND src IN (SELECT id FROM nodes WHERE file_id = ?1)",
            [file_id.0],
        )?;
        self.insert_edges(structural_edges)?;
        self.insert_search_docs(search_docs)
    }

    /// Clears what verifying the file produced: the `scip`/`lsp` edges whose source node lives
    /// in it, its unresolved rows, and its partial-enrichment mark.
    pub fn reset_verification(&mut self, path: &RepoPath) -> Result<()> {
        let (file_id, _) = file_key(&self.tx, path)?;
        self.tx.execute(
            "DELETE FROM edges WHERE verified_by IN ('scip', 'lsp')
                AND src IN (SELECT id FROM nodes WHERE file_id = ?1)",
            [file_id.0],
        )?;
        self.tx
            .execute("DELETE FROM unresolved WHERE file_id = ?1", [file_id.0])?;
        self.tx.execute(
            "UPDATE files SET partially_enriched = 0 WHERE id = ?1",
            [file_id.0],
        )?;
        Ok(())
    }

    /// Deletes every root, file, and node other than communities and notes, with everything
    /// that hangs off them, and the name counts; a full rebuild writes them again.
    pub fn clear_index(&mut self) -> Result<()> {
        self.tx.execute_batch(
            "DELETE FROM nodes WHERE kind NOT IN ('community', 'note');
             DELETE FROM files;
             DELETE FROM roots;
             DELETE FROM name_counts;",
        )?;
        Ok(())
    }

    /// Deletes the package, doc, and heading nodes, with their edges and side rows.
    pub fn clear_docs_layer(&mut self) -> Result<()> {
        self.tx.execute(
            "DELETE FROM nodes WHERE kind IN ('package', 'doc', 'heading')",
            [],
        )?;
        Ok(())
    }

    /// Recounts `name_counts` over the code symbols: every node in a file other than file,
    /// package, doc, and heading nodes.
    pub fn recompute_name_counts(&mut self) -> Result<()> {
        self.tx.execute_batch(
            "DELETE FROM name_counts;
             INSERT INTO name_counts (name, count)
                SELECT name, count(*) FROM nodes
                WHERE file_id IS NOT NULL
                    AND kind NOT IN ('file', 'package', 'doc', 'heading', 'community', 'note')
                GROUP BY name;",
        )?;
        Ok(())
    }

    /// Replaces every community: its node (`community:<stable id>`, named by its label), its
    /// row, a `member_of` edge from each member, and the members' `community_id`.
    pub fn replace_communities(&mut self, communities: &[Community]) -> Result<()> {
        self.tx
            .execute("DELETE FROM nodes WHERE kind = 'community'", [])?;
        for community in communities {
            let path = SemanticPath::community(&community.stable_id)?;
            let node = Node {
                path: path.clone(),
                kind: NodeKind::Community,
                name: community.label.clone(),
                file: None,
                span: None,
                selection: None,
                signature: None,
                has_explicit_return_type: false,
                body_hash: None,
                doc_comment: None,
                visibility: None,
            };
            let [id] = self.insert_nodes(std::slice::from_ref(&node))?[..] else {
                return Err(StoreError::OutOfRange("community node"));
            };
            let size = i64::try_from(community.members.len())
                .map_err(|_| StoreError::OutOfRange("size"))?;
            self.tx.execute(
                "INSERT INTO communities (node_id, stable_id, label, label_source, size)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    id.0,
                    community.stable_id,
                    community.label,
                    community.label_source,
                    size
                ],
            )?;
            let edges: Vec<Edge> = community
                .members
                .iter()
                .map(|member| Edge {
                    src: member.clone(),
                    dst: node.node_ref(),
                    kind: EdgeKind::MemberOf,
                    verified_by: VerifiedBy::Derived,
                    occurrences: Vec::new(),
                })
                .collect();
            self.insert_edges(&edges)?;
            for member in &community.members {
                let member = node_key(&self.tx, member)?;
                self.tx.execute(
                    "UPDATE nodes SET community_id = ?2 WHERE id = ?1",
                    params![member.0, id.0],
                )?;
            }
        }
        Ok(())
    }

    /// Sets the label of the community with `stable_id` on its row and as its node's name;
    /// does nothing when there is no such community.
    pub fn set_community_label(
        &mut self,
        stable_id: &str,
        label: &str,
        label_source: LabelSource,
    ) -> Result<()> {
        self.tx.execute(
            "UPDATE communities SET label = ?2, label_source = ?3 WHERE stable_id = ?1",
            params![stable_id, label, label_source],
        )?;
        self.tx.execute(
            "UPDATE nodes SET name = ?2
             WHERE id = (SELECT node_id FROM communities WHERE stable_id = ?1)",
            params![stable_id, label],
        )?;
        Ok(())
    }

    /// Inserts or updates the note's node (`note:<uuid>`), its row, and its search doc, named
    /// after its target. The target link is `set_note_target`'s; a new note starts orphaned.
    pub fn upsert_note(&mut self, note: &Note) -> Result<NodeId> {
        let path = SemanticPath::note(&note.uuid.0)?;
        let target: Option<SemanticPath> = note.target_semantic_path.parse().ok();
        let name = target
            .as_ref()
            .map_or(note.target_semantic_path.as_str(), SemanticPath::name)
            .to_owned();
        let node = Node {
            path: path.clone(),
            kind: NodeKind::Note,
            name: name.clone(),
            file: None,
            span: None,
            selection: None,
            signature: None,
            has_explicit_return_type: false,
            body_hash: None,
            doc_comment: None,
            visibility: None,
        };
        let node_ref = node.node_ref();
        let id = match node_key(&self.tx, &node_ref) {
            Ok(id) => {
                self.tx.execute(
                    "UPDATE nodes SET name = ?2 WHERE id = ?1",
                    params![id.0, name],
                )?;
                id
            }
            Err(StoreError::UnknownNode(_)) => {
                let [id] = self.insert_nodes(std::slice::from_ref(&node))?[..] else {
                    return Err(StoreError::OutOfRange("note node"));
                };
                id
            }
            Err(error) => return Err(error),
        };
        self.tx.execute(
            "INSERT INTO notes (node_id, uuid, target_semantic_path, kind, body, created, updated,
                                orphaned)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 1)
             ON CONFLICT (uuid) DO UPDATE SET
                target_semantic_path = excluded.target_semantic_path, kind = excluded.kind,
                body = excluded.body, created = excluded.created, updated = excluded.updated",
            params![
                id.0,
                note.uuid.0,
                note.target_semantic_path,
                note.kind,
                note.body,
                note.created,
                note.updated,
            ],
        )?;
        self.insert_search_docs(&[SearchDoc {
            node: node_ref,
            name_split: name_split(&name),
            name,
            signature: None,
            doc_comment: Some(note.body.clone()),
            path: target
                .as_ref()
                .and_then(|t| t.file())
                .and_then(|file| RepoPath::new(file).ok()),
        }])?;
        Ok(id)
    }

    /// Removes the note's node with its row, search doc, and `annotates` edge.
    pub fn delete_note(&mut self, uuid: &NoteId) -> Result<()> {
        self.tx.execute(
            "DELETE FROM nodes WHERE id = (SELECT node_id FROM notes WHERE uuid = ?1)",
            [&uuid.0],
        )?;
        Ok(())
    }

    /// Points the note at its target: a node through an `annotates` edge, the repository with no
    /// edge, or nothing, which marks it orphaned.
    pub fn set_note_target(&mut self, uuid: &NoteId, target: NoteTarget<'_>) -> Result<()> {
        let note = self
            .tx
            .query_row(
                "SELECT node_id FROM notes WHERE uuid = ?1",
                [&uuid.0],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .ok_or_else(|| StoreError::UnknownNote(uuid.clone()))?;
        let target_id = match target {
            NoteTarget::Node(node) => Some(node_key(&self.tx, node)?),
            NoteTarget::Repository | NoteTarget::Orphaned => None,
        };
        self.tx.execute(
            "UPDATE notes SET target_node_id = ?2, orphaned = ?3 WHERE node_id = ?1",
            params![
                note,
                target_id.map(|t| t.0),
                matches!(target, NoteTarget::Orphaned)
            ],
        )?;
        self.tx.execute(
            "DELETE FROM edges WHERE src = ?1 AND kind = 'annotates'",
            [note],
        )?;
        if let Some(target) = target_id {
            self.tx.execute(
                "INSERT INTO edges (src, dst, kind, verified_by, root_id, occurrences)
                 VALUES (?1, ?2, 'annotates', 'note', NULL, '[]')",
                params![note, target.0],
            )?;
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
