/// Bumped whenever the DDL or any stored text format changes; a mismatch on open means rebuild.
pub const SCHEMA_VERSION: u32 = 3;

pub(crate) const DDL: &str = r#"
CREATE TABLE meta (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE roots (
    id INTEGER PRIMARY KEY,
    path TEXT NOT NULL,
    language TEXT NOT NULL,
    server_name TEXT,
    server_version TEXT,
    server_source TEXT,
    coverage TEXT,
    degraded_reason TEXT,
    config_diagnostics TEXT,
    UNIQUE (path, language)
);

CREATE TABLE files (
    id INTEGER PRIMARY KEY,
    path TEXT NOT NULL UNIQUE,
    root_id INTEGER REFERENCES roots (id) ON DELETE SET NULL,
    hash BLOB NOT NULL,
    size INTEGER NOT NULL,
    language TEXT,
    kind TEXT NOT NULL,
    parsed INTEGER NOT NULL,
    skip_reason TEXT,
    scip_covered INTEGER NOT NULL,
    partially_enriched INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE nodes (
    id INTEGER PRIMARY KEY,
    semantic_path TEXT NOT NULL,
    kind TEXT NOT NULL,
    name TEXT NOT NULL,
    file_id INTEGER REFERENCES files (id) ON DELETE CASCADE,
    root_id INTEGER REFERENCES roots (id) ON DELETE SET NULL,
    byte_start INTEGER,
    byte_end INTEGER,
    line_start INTEGER,
    line_end INTEGER,
    sel_start INTEGER,
    sel_end INTEGER,
    signature TEXT,
    has_explicit_return_type INTEGER NOT NULL,
    body_hash BLOB,
    doc_comment TEXT,
    visibility TEXT,
    community_id INTEGER REFERENCES nodes (id) ON DELETE SET NULL,
    unresolved_out INTEGER NOT NULL DEFAULT 0,
    external_out INTEGER NOT NULL DEFAULT 0,
    attrs TEXT,
    UNIQUE (semantic_path, kind)
);
CREATE INDEX nodes_file ON nodes (file_id);
CREATE INDEX nodes_name ON nodes (name);
CREATE INDEX nodes_community ON nodes (community_id);

CREATE TABLE impl_records (
    id INTEGER PRIMARY KEY,
    file_id INTEGER NOT NULL REFERENCES files (id) ON DELETE CASCADE,
    self_type_text TEXT NOT NULL,
    trait_text TEXT,
    byte_start INTEGER NOT NULL,
    byte_end INTEGER NOT NULL,
    line_start INTEGER NOT NULL,
    line_end INTEGER NOT NULL,
    header_start INTEGER NOT NULL,
    header_end INTEGER NOT NULL,
    body_start INTEGER NOT NULL,
    body_end INTEGER NOT NULL,
    self_type_sel_start INTEGER NOT NULL,
    self_type_sel_end INTEGER NOT NULL
);
CREATE INDEX impl_records_file ON impl_records (file_id);

CREATE TABLE edges (
    src INTEGER NOT NULL REFERENCES nodes (id) ON DELETE CASCADE,
    dst INTEGER NOT NULL REFERENCES nodes (id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    verified_by TEXT NOT NULL,
    root_id INTEGER REFERENCES roots (id) ON DELETE SET NULL,
    occurrences TEXT NOT NULL,
    PRIMARY KEY (src, dst, kind)
);
CREATE INDEX edges_dst_kind ON edges (dst, kind);
CREATE INDEX edges_src_kind ON edges (src, kind);

CREATE TABLE unresolved (
    id INTEGER PRIMARY KEY,
    file_id INTEGER NOT NULL REFERENCES files (id) ON DELETE CASCADE,
    node_id INTEGER NOT NULL REFERENCES nodes (id) ON DELETE CASCADE,
    text TEXT NOT NULL,
    role TEXT NOT NULL,
    line INTEGER NOT NULL,
    col INTEGER NOT NULL,
    reason TEXT NOT NULL
);
CREATE INDEX unresolved_file ON unresolved (file_id);
CREATE INDEX unresolved_node ON unresolved (node_id);
CREATE INDEX unresolved_text ON unresolved (text);

CREATE TABLE communities (
    node_id INTEGER PRIMARY KEY REFERENCES nodes (id) ON DELETE CASCADE,
    stable_id TEXT NOT NULL UNIQUE,
    label TEXT NOT NULL,
    label_source TEXT NOT NULL,
    size INTEGER NOT NULL
);

CREATE TABLE packages (
    node_id INTEGER PRIMARY KEY REFERENCES nodes (id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    version TEXT,
    external INTEGER NOT NULL,
    manifest_file_id INTEGER REFERENCES files (id) ON DELETE SET NULL
);

CREATE TABLE notes (
    node_id INTEGER PRIMARY KEY REFERENCES nodes (id) ON DELETE CASCADE,
    uuid TEXT NOT NULL UNIQUE,
    target_semantic_path TEXT NOT NULL,
    target_node_id INTEGER REFERENCES nodes (id) ON DELETE SET NULL,
    kind TEXT NOT NULL,
    body TEXT NOT NULL,
    created TEXT NOT NULL,
    updated TEXT NOT NULL,
    orphaned INTEGER NOT NULL
);
CREATE INDEX notes_target ON notes (target_node_id);

CREATE TABLE name_counts (
    name TEXT PRIMARY KEY,
    count INTEGER NOT NULL
);

CREATE TABLE search_docs (
    node_id INTEGER PRIMARY KEY REFERENCES nodes (id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    name_split TEXT NOT NULL,
    signature TEXT,
    doc_comment TEXT,
    path TEXT
);

CREATE VIRTUAL TABLE search_fts USING fts5 (
    name, name_split, signature, doc_comment, path,
    content = 'search_docs',
    content_rowid = 'node_id',
    tokenize = 'unicode61'
);

CREATE TRIGGER search_docs_ai AFTER INSERT ON search_docs BEGIN
    INSERT INTO search_fts (rowid, name, name_split, signature, doc_comment, path)
    VALUES (new.node_id, new.name, new.name_split, new.signature, new.doc_comment, new.path);
END;

CREATE TRIGGER search_docs_ad AFTER DELETE ON search_docs BEGIN
    INSERT INTO search_fts (search_fts, rowid, name, name_split, signature, doc_comment, path)
    VALUES ('delete', old.node_id, old.name, old.name_split, old.signature, old.doc_comment, old.path);
END;

CREATE TRIGGER search_docs_au AFTER UPDATE ON search_docs BEGIN
    INSERT INTO search_fts (search_fts, rowid, name, name_split, signature, doc_comment, path)
    VALUES ('delete', old.node_id, old.name, old.name_split, old.signature, old.doc_comment, old.path);
    INSERT INTO search_fts (rowid, name, name_split, signature, doc_comment, path)
    VALUES (new.node_id, new.name, new.name_split, new.signature, new.doc_comment, new.path);
END;
"#;
