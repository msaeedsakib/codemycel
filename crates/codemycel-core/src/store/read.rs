use std::collections::BTreeMap;

use rusqlite::types::{Type, Value};
use rusqlite::{Connection, OptionalExtension, Row, params, params_from_iter};

use super::StoreError;
use super::dump;
use super::error::Result;
use super::sql::{hash_at, json_at, occurrences_at};
use crate::{
    ByteRange, Candidate, CandidateRole, Community, Coverage, Edge, EdgeKind, FileId, FileRecord,
    ImplRecord, Node, NodeId, NodeKind, NodeRef, Note, NoteId, Occurrence, Package, RepoPath, Root,
    RootId, RootStatus, SearchHit, SemanticPath, ServerInfo, Span, StoredNode, Unresolved,
    UnresolvedReason, VerifiedBy,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredFile {
    pub id: FileId,
    pub record: FileRecord,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredRoot {
    pub id: RootId,
    pub root: Root,
    /// `None` until a verify phase has run over the root.
    pub status: Option<RootStatus>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredEdge {
    pub src_id: NodeId,
    pub dst_id: NodeId,
    pub root_id: Option<RootId>,
    pub edge: Edge,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredUnresolved {
    pub node_id: NodeId,
    pub unresolved: Unresolved,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredNote {
    pub node_id: NodeId,
    pub note: Note,
    /// The node the note annotates; `None` while it is orphaned.
    pub target: Option<NodeRef>,
    pub orphaned: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub nodes: u64,
    pub edges: BTreeMap<EdgeKind, u64>,
    pub unresolved: BTreeMap<UnresolvedReason, u64>,
}

const NODE_SELECT: &str = "SELECT n.id, n.semantic_path, n.kind, n.name, f.path, n.byte_start,
        n.byte_end, n.line_start, n.line_end, n.sel_start, n.sel_end, n.signature,
        n.has_explicit_return_type, n.body_hash, n.doc_comment, n.visibility, n.root_id,
        n.community_id, n.unresolved_out, n.external_out
    FROM nodes n LEFT JOIN files f ON f.id = n.file_id";

const FILE_SELECT: &str = "SELECT id, path, root_id, hash, size, language, kind, parsed,
        skip_reason, scip_covered
    FROM files";

const EDGE_SELECT: &str = "SELECT e.src, e.dst, s.semantic_path, s.kind, d.semantic_path,
        d.kind, e.kind, e.verified_by, e.root_id, e.occurrences
    FROM edges e JOIN nodes s ON s.id = e.src JOIN nodes d ON d.id = e.dst";

const NOTE_SELECT: &str = "SELECT n.node_id, n.uuid, n.target_semantic_path, n.kind, n.body,
        n.created, n.updated, t.semantic_path, t.kind, n.orphaned
    FROM notes n LEFT JOIN nodes t ON t.id = n.target_node_id";

const UNRESOLVED_SELECT: &str = "SELECT u.node_id, f.path, n.semantic_path, n.kind, u.text,
        u.role, u.line, u.col, u.reason
    FROM unresolved u JOIN files f ON f.id = u.file_id JOIN nodes n ON n.id = u.node_id";

/// Every read on the index, shared by `Store` and `Tx`.
pub trait StoreRead {
    fn conn(&self) -> &Connection;

    fn meta_get(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .conn()
            .query_row("SELECT value FROM meta WHERE key = ?1", [key], |row| {
                row.get(0)
            })
            .optional()?)
    }

    fn roots(&self) -> Result<Vec<StoredRoot>> {
        let mut stmt = self.conn().prepare_cached(
            "SELECT id, path, language, server_name, server_version, server_source, coverage,
                    degraded_reason, config_diagnostics
             FROM roots ORDER BY path, language",
        )?;
        let rows = stmt.query_map([], |row| {
            let server = match (row.get(3)?, row.get(4)?, row.get(5)?) {
                (Some(name), Some(version), Some(source)) => Some(ServerInfo {
                    name,
                    version,
                    source,
                }),
                _ => None,
            };
            let status = match row.get::<_, Option<Coverage>>(6)? {
                Some(coverage) => Some(RootStatus {
                    server,
                    coverage,
                    degraded: row.get(7)?,
                    config_diagnostics: json_at(row, 8)?.unwrap_or_default(),
                }),
                None => None,
            };
            Ok(StoredRoot {
                id: RootId(row.get(0)?),
                root: Root {
                    path: row.get(1)?,
                    language: row.get(2)?,
                },
                status,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Files whose server requests failed twice in the last run, sorted.
    fn partially_enriched_files(&self) -> Result<Vec<RepoPath>> {
        let mut stmt = self
            .conn()
            .prepare_cached("SELECT path FROM files WHERE partially_enriched = 1 ORDER BY path")?;
        let rows = stmt.query_map([], |row| row.get(0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    fn file_by_path(&self, path: &RepoPath) -> Result<Option<StoredFile>> {
        let sql = format!("{FILE_SELECT} WHERE path = ?1");
        Ok(self
            .conn()
            .prepare_cached(&sql)?
            .query_row([path], stored_file)
            .optional()?)
    }

    fn files(&self) -> Result<Vec<StoredFile>> {
        let sql = format!("{FILE_SELECT} ORDER BY path");
        let mut stmt = self.conn().prepare_cached(&sql)?;
        let rows = stmt.query_map([], stored_file)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    fn node_by_id(&self, id: NodeId) -> Result<Option<StoredNode>> {
        let sql = format!("{NODE_SELECT} WHERE n.id = ?1");
        Ok(self
            .conn()
            .prepare_cached(&sql)?
            .query_row([id.0], stored_node)
            .optional()?)
    }

    /// With a kind, at most one node; without, every kind sharing the path.
    fn node_by_semantic_path(
        &self,
        path: &SemanticPath,
        kind: Option<NodeKind>,
    ) -> Result<Vec<StoredNode>> {
        let sql = format!(
            "{NODE_SELECT} WHERE n.semantic_path = ?1 AND (?2 IS NULL OR n.kind = ?2)
             ORDER BY n.kind"
        );
        let mut stmt = self.conn().prepare_cached(&sql)?;
        let rows = stmt.query_map(params![path, kind], stored_node)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    fn nodes_in_file(&self, path: &RepoPath) -> Result<Vec<StoredNode>> {
        let sql = format!("{NODE_SELECT} WHERE f.path = ?1 ORDER BY n.semantic_path, n.kind");
        let mut stmt = self.conn().prepare_cached(&sql)?;
        let rows = stmt.query_map([path], stored_node)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Exact, case-sensitive name match across all kinds.
    fn nodes_named(&self, name: &str) -> Result<Vec<StoredNode>> {
        let sql = format!("{NODE_SELECT} WHERE n.name = ?1 ORDER BY n.semantic_path, n.kind");
        let mut stmt = self.conn().prepare_cached(&sql)?;
        let rows = stmt.query_map([name], stored_node)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Every node of one of `kinds`, by semantic path and kind.
    fn nodes_of_kinds(&self, kinds: &[NodeKind]) -> Result<Vec<StoredNode>> {
        let placeholders: Vec<String> = (1..=kinds.len()).map(|i| format!("?{i}")).collect();
        let sql = format!(
            "{NODE_SELECT} WHERE n.kind IN ({}) ORDER BY n.semantic_path, n.kind",
            placeholders.join(", ")
        );
        let mut stmt = self.conn().prepare_cached(&sql)?;
        let rows = stmt.query_map(params_from_iter(kinds), stored_node)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Every edge of one of `kinds` verified by one of `verified_by`, sorted by source, target,
    /// and kind.
    fn edges_of_kinds(
        &self,
        kinds: &[EdgeKind],
        verified_by: &[VerifiedBy],
    ) -> Result<Vec<StoredEdge>> {
        let list = |from: usize, len: usize| {
            (from..from + len)
                .map(|i| format!("?{i}"))
                .collect::<Vec<_>>()
                .join(", ")
        };
        let sql = format!(
            "{EDGE_SELECT} WHERE e.kind IN ({}) AND e.verified_by IN ({})
             ORDER BY s.semantic_path, s.kind, d.semantic_path, d.kind, e.kind",
            list(1, kinds.len()),
            list(kinds.len() + 1, verified_by.len())
        );
        let values = kinds
            .iter()
            .map(|k| Value::Text(k.as_str().to_owned()))
            .chain(
                verified_by
                    .iter()
                    .map(|v| Value::Text(v.as_str().to_owned())),
            );
        let mut stmt = self.conn().prepare_cached(&sql)?;
        let rows = stmt.query_map(params_from_iter(values), stored_edge)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Outgoing edges of `node`; an empty `kinds` means every kind.
    fn edges_from(&self, node: NodeId, kinds: &[EdgeKind]) -> Result<Vec<StoredEdge>> {
        edges_where(self.conn(), "e.src", node, kinds, "d.semantic_path, d.kind")
    }

    /// Incoming edges of `node`; an empty `kinds` means every kind.
    fn edges_to(&self, node: NodeId, kinds: &[EdgeKind]) -> Result<Vec<StoredEdge>> {
        edges_where(self.conn(), "e.dst", node, kinds, "s.semantic_path, s.kind")
    }

    /// How many edges of `kind` start at `node`.
    fn count_edges_from(&self, node: NodeId, kind: EdgeKind) -> Result<u64> {
        let count: i64 = self
            .conn()
            .prepare_cached("SELECT count(*) FROM edges WHERE src = ?1 AND kind = ?2")?
            .query_row(params![node.0, kind], |row| row.get(0))?;
        to_u64(count)
    }

    /// How many edges of `kind` end at `node`.
    fn count_edges_to(&self, node: NodeId, kind: EdgeKind) -> Result<u64> {
        let count: i64 = self
            .conn()
            .prepare_cached("SELECT count(*) FROM edges WHERE dst = ?1 AND kind = ?2")?
            .query_row(params![node.0, kind], |row| row.get(0))?;
        to_u64(count)
    }

    fn unresolved_for_file(&self, path: &RepoPath) -> Result<Vec<StoredUnresolved>> {
        let sql = format!("{UNRESOLVED_SELECT} WHERE f.path = ?1 ORDER BY u.line, u.col, u.text");
        let mut stmt = self.conn().prepare_cached(&sql)?;
        let rows = stmt.query_map([path], stored_unresolved)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    fn unresolved_for_node(&self, node: NodeId) -> Result<Vec<StoredUnresolved>> {
        let sql =
            format!("{UNRESOLVED_SELECT} WHERE u.node_id = ?1 ORDER BY u.line, u.col, u.text");
        let mut stmt = self.conn().prepare_cached(&sql)?;
        let rows = stmt.query_map([node.0], stored_unresolved)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    fn impl_records_in_file(&self, path: &RepoPath) -> Result<Vec<ImplRecord>> {
        let mut stmt = self.conn().prepare_cached(
            "SELECT f.path, i.self_type_text, i.trait_text, i.byte_start, i.byte_end,
                    i.line_start, i.line_end, i.header_start, i.header_end, i.body_start,
                    i.body_end, i.self_type_sel_start, i.self_type_sel_end
             FROM impl_records i JOIN files f ON f.id = i.file_id
             WHERE f.path = ?1 ORDER BY i.byte_start, i.id",
        )?;
        let rows = stmt.query_map([path], |row| {
            Ok(ImplRecord {
                file: row.get(0)?,
                self_type_text: row.get(1)?,
                trait_text: row.get(2)?,
                span: Span {
                    bytes: range(row, 3)?,
                    line_start: row.get(5)?,
                    line_end: row.get(6)?,
                },
                header: range(row, 7)?,
                body: range(row, 9)?,
                self_type_selection: range(row, 11)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Files holding an unresolved candidate whose text is `text`, sorted.
    fn files_with_unresolved_text(&self, text: &str) -> Result<Vec<RepoPath>> {
        let mut stmt = self.conn().prepare_cached(
            "SELECT DISTINCT f.path FROM unresolved u JOIN files f ON f.id = u.file_id
             WHERE u.text = ?1 ORDER BY f.path",
        )?;
        let rows = stmt.query_map([text], |row| row.get(0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// The file and text of every unresolved candidate of one of `roles`, distinct and sorted.
    fn unresolved_texts(&self, roles: &[CandidateRole]) -> Result<Vec<(RepoPath, String)>> {
        let placeholders: Vec<String> = (1..=roles.len()).map(|i| format!("?{i}")).collect();
        let sql = format!(
            "SELECT DISTINCT f.path, u.text FROM unresolved u JOIN files f ON f.id = u.file_id
             WHERE u.role IN ({}) ORDER BY f.path, u.text",
            placeholders.join(", ")
        );
        let mut stmt = self.conn().prepare_cached(&sql)?;
        let values = roles
            .iter()
            .map(|role| Value::Text(role.as_str().to_owned()));
        let rows = stmt.query_map(params_from_iter(values), |row| {
            Ok((row.get(0)?, row.get(1)?))
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Every community with its members sorted, by stable id.
    fn communities(&self) -> Result<Vec<Community>> {
        let mut stmt = self.conn().prepare_cached(
            "SELECT c.node_id, c.stable_id, c.label, c.label_source FROM communities c
             ORDER BY c.stable_id",
        )?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    NodeId(row.get(0)?),
                    Community {
                        stable_id: row.get(1)?,
                        label: row.get(2)?,
                        label_source: row.get(3)?,
                        members: Vec::new(),
                    },
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut communities = Vec::with_capacity(rows.len());
        for (id, mut community) in rows {
            community.members = self
                .edges_to(id, &[EdgeKind::MemberOf])?
                .into_iter()
                .map(|stored| stored.edge.src)
                .collect();
            communities.push(community);
        }
        Ok(communities)
    }

    /// Every note, by uuid.
    fn notes(&self) -> Result<Vec<StoredNote>> {
        let sql = format!("{NOTE_SELECT} ORDER BY n.uuid");
        let mut stmt = self.conn().prepare_cached(&sql)?;
        let rows = stmt.query_map([], stored_note)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    fn note_by_uuid(&self, uuid: &NoteId) -> Result<Option<StoredNote>> {
        let sql = format!("{NOTE_SELECT} WHERE n.uuid = ?1");
        Ok(self
            .conn()
            .prepare_cached(&sql)?
            .query_row([&uuid.0], stored_note)
            .optional()?)
    }

    fn name_count(&self, name: &str) -> Result<u32> {
        let count: Option<u32> = self
            .conn()
            .query_row(
                "SELECT count FROM name_counts WHERE name = ?1",
                [name],
                |row| row.get(0),
            )
            .optional()?;
        Ok(count.unwrap_or(0))
    }

    /// Every FTS5 match for `query` (FTS5 query syntax) of one of `kinds` (any kind when empty),
    /// best first, scored by BM25 with per-column weights.
    fn search_fts(&self, query: &str, kinds: &[NodeKind]) -> Result<Vec<SearchHit>> {
        let filter = if kinds.is_empty() {
            String::new()
        } else {
            let placeholders: Vec<String> = (2..kinds.len() + 2).map(|i| format!("?{i}")).collect();
            format!(" AND n.kind IN ({})", placeholders.join(", "))
        };
        let sql = format!(
            "SELECT n.id, n.kind, n.name, f.path,
                    bm25(search_fts, 10.0, 6.0, 2.0, 2.0, 1.0) AS score
             FROM search_fts
             JOIN nodes n ON n.id = search_fts.rowid
             LEFT JOIN files f ON f.id = n.file_id
             WHERE search_fts MATCH ?1{filter}
             ORDER BY score, n.id"
        );
        let mut values = vec![Value::Text(query.to_owned())];
        values.extend(kinds.iter().map(|k| Value::Text(k.as_str().to_owned())));
        let mut stmt = self.conn().prepare_cached(&sql)?;
        let rows = stmt.query_map(params_from_iter(values), |row| {
            Ok(SearchHit {
                node_id: NodeId(row.get(0)?),
                kind: row.get(1)?,
                name: row.get(2)?,
                file: row.get(3)?,
                bm25: row.get(4)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    fn counts(&self) -> Result<Counts> {
        let conn = self.conn();
        let nodes: i64 = conn.query_row("SELECT count(*) FROM nodes", [], |row| row.get(0))?;
        let mut counts = Counts {
            nodes: to_u64(nodes)?,
            ..Counts::default()
        };
        let mut stmt = conn.prepare_cached("SELECT kind, count(*) FROM edges GROUP BY kind")?;
        for row in stmt.query_map([], |row| Ok((row.get(0)?, row.get::<_, i64>(1)?)))? {
            let (kind, count) = row?;
            counts.edges.insert(kind, to_u64(count)?);
        }
        let mut stmt =
            conn.prepare_cached("SELECT reason, count(*) FROM unresolved GROUP BY reason")?;
        for row in stmt.query_map([], |row| Ok((row.get(0)?, row.get::<_, i64>(1)?)))? {
            let (reason, count) = row?;
            counts.unresolved.insert(reason, to_u64(count)?);
        }
        Ok(counts)
    }

    /// `<semantic path>\t<kind>\t<line_start>-<line_end>` per node other than communities,
    /// sorted.
    fn dump_nodes(&self) -> Result<String> {
        dump::nodes(self.conn())
    }

    /// `<src path> (<src kind>)\t<edge kind>\t<dst path> (<dst kind>)\t<occurrence count>` per
    /// edge other than `member_of`, sorted.
    fn dump_edges(&self) -> Result<String> {
        dump::edges(self.conn())
    }

    /// `<stable id>\t<label>\t<member semantic paths, sorted, comma-separated>` per community,
    /// sorted by id.
    fn dump_communities(&self) -> Result<String> {
        let mut out = String::new();
        for community in self.communities()? {
            let mut members: Vec<&str> =
                community.members.iter().map(|m| m.path.as_str()).collect();
            members.sort_unstable();
            out.push_str(&format!(
                "{}\t{}\t{}\n",
                community.stable_id,
                community.label,
                members.join(",")
            ));
        }
        Ok(out)
    }

    /// `<file>:<line>:<col>\t<role>\t<text>\t<reason>` per candidate, sorted.
    fn dump_unresolved(&self) -> Result<String> {
        dump::unresolved(self.conn())
    }

    /// `<file>\t<self type>\t<trait or ->\t<line_start>-<line_end>` per impl record, sorted.
    fn dump_impl_records(&self) -> Result<String> {
        dump::impl_records(self.conn())
    }

    /// Code symbols with their count of incoming `kinds` edges, highest first, then by semantic
    /// path and kind; at most `limit`. Files, packages, docs, headings, communities and notes are
    /// left out. `community` keeps only its members, `file` only the nodes in that file.
    fn nodes_by_in_degree(
        &self,
        kinds: &[EdgeKind],
        community: Option<NodeId>,
        file: Option<&RepoPath>,
        limit: u32,
    ) -> Result<Vec<(NodeId, u64)>> {
        let placeholders: Vec<String> = (4..kinds.len() + 4).map(|i| format!("?{i}")).collect();
        let degree = if kinds.is_empty() {
            "0".to_owned()
        } else {
            format!(
                "(SELECT count(*) FROM edges e WHERE e.dst = n.id AND e.kind IN ({}))",
                placeholders.join(", ")
            )
        };
        let sql = format!(
            "SELECT n.id, {degree} AS degree FROM nodes n LEFT JOIN files f ON f.id = n.file_id
             WHERE n.kind NOT IN ('file', 'package', 'doc', 'heading', 'community', 'note')
               AND (?1 IS NULL OR n.community_id = ?1) AND (?2 IS NULL OR f.path = ?2)
             ORDER BY degree DESC, n.semantic_path, n.kind
             LIMIT ?3"
        );
        let mut values = vec![
            community.map_or(Value::Null, |id| Value::Integer(id.0)),
            file.map_or(Value::Null, |path| Value::Text(path.as_str().to_owned())),
            Value::Integer(i64::from(limit)),
        ];
        values.extend(kinds.iter().map(|k| Value::Text(k.as_str().to_owned())));
        let mut stmt = self.conn().prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(values), |row| {
            Ok((NodeId(row.get(0)?), row.get::<_, i64>(1)?))
        })?;
        let mut ranked = Vec::new();
        for row in rows {
            let (id, degree) = row?;
            ranked.push((id, to_u64(degree)?));
        }
        Ok(ranked)
    }

    /// Every unresolved candidate whose text is `text`, by file, line, and column.
    fn unresolved_with_text(&self, text: &str) -> Result<Vec<StoredUnresolved>> {
        let sql =
            format!("{UNRESOLVED_SELECT} WHERE u.text = ?1 ORDER BY f.path, u.line, u.col, u.id");
        let mut stmt = self.conn().prepare_cached(&sql)?;
        let rows = stmt.query_map([text], stored_unresolved)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Unresolved candidates per root and reason, by root id and reason; only `reason` when set.
    /// Files under no root count under `None`.
    fn unresolved_counts_by_root(
        &self,
        reason: Option<UnresolvedReason>,
    ) -> Result<Vec<(Option<RootId>, UnresolvedReason, u64)>> {
        let mut stmt = self.conn().prepare_cached(
            "SELECT f.root_id, u.reason, count(*) FROM unresolved u
             JOIN files f ON f.id = u.file_id
             WHERE ?1 IS NULL OR u.reason = ?1
             GROUP BY f.root_id, u.reason ORDER BY f.root_id, u.reason",
        )?;
        let rows = stmt.query_map([reason], |row| {
            Ok((
                row.get::<_, Option<i64>>(0)?.map(RootId),
                row.get(1)?,
                row.get::<_, i64>(2)?,
            ))
        })?;
        let mut counts = Vec::new();
        for row in rows {
            let (root, reason, count) = row?;
            counts.push((root, reason, to_u64(count)?));
        }
        Ok(counts)
    }

    /// Unresolved candidates per file, most first, then by path; only `reason` when set.
    fn unresolved_counts_by_file(
        &self,
        reason: Option<UnresolvedReason>,
    ) -> Result<Vec<(RepoPath, u64)>> {
        let mut stmt = self.conn().prepare_cached(
            "SELECT f.path, count(*) AS n FROM unresolved u JOIN files f ON f.id = u.file_id
             WHERE ?1 IS NULL OR u.reason = ?1
             GROUP BY f.path ORDER BY n DESC, f.path",
        )?;
        let rows = stmt.query_map([reason], |row| Ok((row.get(0)?, row.get::<_, i64>(1)?)))?;
        let mut counts = Vec::new();
        for row in rows {
            let (path, count) = row?;
            counts.push((path, to_u64(count)?));
        }
        Ok(counts)
    }

    /// Every package, workspace and external, by name and semantic path.
    fn packages(&self) -> Result<Vec<Package>> {
        let mut stmt = self.conn().prepare_cached(
            "SELECT n.semantic_path, n.kind, p.name, p.version, p.external, f.path
             FROM packages p JOIN nodes n ON n.id = p.node_id
             LEFT JOIN files f ON f.id = p.manifest_file_id
             ORDER BY p.name, n.semantic_path",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(Package {
                node: NodeRef::new(row.get(0)?, row.get(1)?),
                name: row.get(2)?,
                version: row.get(3)?,
                external: row.get(4)?,
                manifest: row.get(5)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// The database's size in bytes: its page count times its page size.
    fn database_size(&self) -> Result<u64> {
        let pages: i64 = self
            .conn()
            .query_row("PRAGMA page_count", [], |row| row.get(0))?;
        let size: i64 = self
            .conn()
            .query_row("PRAGMA page_size", [], |row| row.get(0))?;
        to_u64(pages.saturating_mul(size))
    }
}

pub(crate) fn file_key(conn: &Connection, path: &RepoPath) -> Result<(FileId, Option<RootId>)> {
    conn.prepare_cached("SELECT id, root_id FROM files WHERE path = ?1")?
        .query_row([path], |row| {
            Ok((
                FileId(row.get(0)?),
                row.get::<_, Option<i64>>(1)?.map(RootId),
            ))
        })
        .optional()?
        .ok_or_else(|| StoreError::UnknownFile(path.clone()))
}

pub(crate) fn node_key(conn: &Connection, node: &NodeRef) -> Result<NodeId> {
    conn.prepare_cached("SELECT id FROM nodes WHERE semantic_path = ?1 AND kind = ?2")?
        .query_row(params![node.path, node.kind], |row| row.get(0))
        .optional()?
        .map(NodeId)
        .ok_or_else(|| StoreError::UnknownNode(node.clone()))
}

fn edges_where(
    conn: &Connection,
    column: &str,
    node: NodeId,
    kinds: &[EdgeKind],
    order: &str,
) -> Result<Vec<StoredEdge>> {
    let mut values = vec![Value::Integer(node.0)];
    let mut sql = format!("{EDGE_SELECT} WHERE {column} = ?1");
    if !kinds.is_empty() {
        let placeholders: Vec<String> = (2..kinds.len() + 2).map(|i| format!("?{i}")).collect();
        sql.push_str(&format!(" AND e.kind IN ({})", placeholders.join(", ")));
        values.extend(kinds.iter().map(|k| Value::Text(k.as_str().to_owned())));
    }
    sql.push_str(&format!(" ORDER BY e.kind, {order}"));
    let mut stmt = conn.prepare_cached(&sql)?;
    let rows = stmt.query_map(params_from_iter(values), stored_edge)?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

fn stored_edge(row: &Row<'_>) -> rusqlite::Result<StoredEdge> {
    Ok(StoredEdge {
        src_id: NodeId(row.get(0)?),
        dst_id: NodeId(row.get(1)?),
        root_id: row.get::<_, Option<i64>>(8)?.map(RootId),
        edge: Edge {
            src: NodeRef::new(row.get(2)?, row.get(3)?),
            dst: NodeRef::new(row.get(4)?, row.get(5)?),
            kind: row.get(6)?,
            verified_by: row.get(7)?,
            occurrences: occurrences_at(row, 9)?,
        },
    })
}

fn stored_node(row: &Row<'_>) -> rusqlite::Result<StoredNode> {
    let span = match row.get::<_, Option<u32>>(5)? {
        Some(start) => Some(Span {
            bytes: ByteRange {
                start,
                end: row.get(6)?,
            },
            line_start: row.get(7)?,
            line_end: row.get(8)?,
        }),
        None => None,
    };
    let selection = match row.get::<_, Option<u32>>(9)? {
        Some(start) => Some(ByteRange {
            start,
            end: row.get(10)?,
        }),
        None => None,
    };
    Ok(StoredNode {
        id: NodeId(row.get(0)?),
        root_id: row.get::<_, Option<i64>>(16)?.map(RootId),
        community_id: row.get::<_, Option<i64>>(17)?.map(NodeId),
        unresolved_out: row.get(18)?,
        external_out: row.get(19)?,
        node: Node {
            path: row.get(1)?,
            kind: row.get(2)?,
            name: row.get(3)?,
            file: row.get(4)?,
            span,
            selection,
            signature: row.get(11)?,
            has_explicit_return_type: row.get(12)?,
            body_hash: hash_at(row, 13)?,
            doc_comment: row.get(14)?,
            visibility: row.get(15)?,
        },
    })
}

fn stored_file(row: &Row<'_>) -> rusqlite::Result<StoredFile> {
    let size: i64 = row.get(4)?;
    Ok(StoredFile {
        id: FileId(row.get(0)?),
        record: FileRecord {
            path: row.get(1)?,
            root_id: row.get::<_, Option<i64>>(2)?.map(RootId),
            hash: hash_at(row, 3)?.ok_or(rusqlite::Error::InvalidColumnType(
                3,
                "hash".to_owned(),
                Type::Null,
            ))?,
            size: u64::try_from(size).map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(4, Type::Integer, Box::new(e))
            })?,
            language: row.get(5)?,
            kind: row.get(6)?,
            parsed: row.get(7)?,
            skip_reason: row.get(8)?,
            scip_covered: row.get(9)?,
        },
    })
}

fn stored_unresolved(row: &Row<'_>) -> rusqlite::Result<StoredUnresolved> {
    Ok(StoredUnresolved {
        node_id: NodeId(row.get(0)?),
        unresolved: Unresolved {
            candidate: Candidate {
                file: row.get(1)?,
                enclosing: NodeRef::new(row.get(2)?, row.get(3)?),
                text: row.get(4)?,
                role: row.get(5)?,
                at: Occurrence {
                    line: row.get(6)?,
                    col: row.get(7)?,
                },
            },
            reason: row.get(8)?,
        },
    })
}

fn stored_note(row: &Row<'_>) -> rusqlite::Result<StoredNote> {
    let target = match (row.get::<_, Option<SemanticPath>>(7)?, row.get(8)?) {
        (Some(path), Some(kind)) => Some(NodeRef::new(path, kind)),
        _ => None,
    };
    Ok(StoredNote {
        node_id: NodeId(row.get(0)?),
        note: Note {
            uuid: NoteId(row.get(1)?),
            target_semantic_path: row.get(2)?,
            kind: row.get(3)?,
            body: row.get(4)?,
            created: row.get(5)?,
            updated: row.get(6)?,
        },
        target,
        orphaned: row.get(9)?,
    })
}

fn range(row: &Row<'_>, start: usize) -> rusqlite::Result<ByteRange> {
    Ok(ByteRange {
        start: row.get(start)?,
        end: row.get(start + 1)?,
    })
}

fn to_u64(count: i64) -> Result<u64> {
    u64::try_from(count).map_err(|_| StoreError::OutOfRange("row count"))
}
