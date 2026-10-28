//! The generic tree-sitter driver every language adapter runs on (spec §7.2).
//!
//! An adapter supplies a grammar, three query sources, and a [`LanguageHooks`]. Capture names:
//!
//! `symbols`
//! - `@def.<node kind>` on a definition, e.g. `@def.function`; one per match. Several matches on
//!   the same node and kind merge, so an optional wrapper (an `export` statement) can live in its
//!   own pattern.
//! - `@def.name` (required): the identifier; its text is the node's name and its range the
//!   selection range.
//! - `@def.body`: the body; the signature ends where it starts and `body_hash` covers its bytes.
//! - `@def.visibility`: the visibility or export modifier, kept as written.
//! - `@def.return_type`: present when the return type is written out.
//! - `@def.overload`: marks an overload signature. Overloads collapse into the implementation
//!   with the same path and kind, or into the first overload when there is none.
//! - `@impl` on a block that names a container without being a node (a Rust `impl`), with
//!   `@impl.type` (required), `@impl.type_name` (the identifier to select; defaults to the whole
//!   type), `@impl.trait`, and `@impl.body` (required). Each becomes an `ImplRecord`; definitions
//!   inside it get the container [`LanguageHooks::impl_container`] returns.
//!
//! `candidates`: `@cand.<role>` on an identifier, e.g. `@cand.call`. When one identifier is
//! captured under several roles, the first of export, import, extends, implements, call, type,
//! value wins.
//!
//! `locals`: `@local.scope` and `@local.definition`. A candidate whose text equals a definition in
//! a scope enclosing it, starting no later than it, is dropped (spec §6.5).
//!
//! Captures whose names start with `_` are free for predicates. Any other name is an error.
//!
//! Paths: containers are the enclosing definitions and impl blocks; same path, same kind siblings
//! after the first get `[1]`, `[2]`, … in source order. Edges: a definition directly inside a
//! `module` gets `defines` from it, one inside any other definition `contains` from it, and one
//! inside no definition `contains` from the file node. Impl blocks are skipped when finding the
//! parent, so impl methods hang off the file.

mod doc_comment;
mod extract;
mod locals;
mod queries;
mod signature;

use codemycel_core::{AdapterError, ParsedFile, RepoPath};
use thiserror::Error;

pub use signature::{normalise_signature, normalise_type};

use queries::Compiled;

/// The query sources of one grammar; see the module documentation for capture names.
#[derive(Clone, Copy, Debug)]
pub struct QuerySet {
    pub symbols: &'static str,
    pub candidates: &'static str,
    pub locals: &'static str,
}

/// The language-specific corners of extraction that queries cannot express.
pub trait LanguageHooks: Send + Sync {
    /// The container segment for definitions inside an impl block. Both texts are already
    /// whitespace-normalised by [`normalise_type`].
    fn impl_container(&self, self_type: &str, trait_text: Option<&str>) -> String {
        let _ = trait_text;
        self_type.to_owned()
    }

    /// Whether a comment directly before a definition documents it (`///`, `/**`).
    fn is_doc_comment(&self, text: &str) -> bool;

    /// Node kinds that wrap a definition without being part of it (`export_statement`); the doc
    /// comment is looked for before the outermost wrapper.
    fn doc_wrapper_kinds(&self) -> &'static [&'static str] {
        &[]
    }

    /// Node kinds allowed between a doc comment and its definition (`attribute_item`).
    fn doc_skip_kinds(&self) -> &'static [&'static str] {
        &[]
    }

    /// Runs last, with the tree. `source` is the file without a UTF-8 BOM; byte positions in
    /// `parsed` are file positions, so they exceed tree positions by the BOM's length.
    fn finish(&self, parsed: &mut ParsedFile, root: tree_sitter::Node<'_>, source: &str) {
        let _ = (parsed, root, source);
    }
}

#[derive(Debug, Error)]
pub enum DriverError {
    #[error("{query} query: {source}")]
    Query {
        query: &'static str,
        source: tree_sitter::QueryError,
    },
    #[error("{query} query: unknown capture `@{name}`")]
    UnknownCapture { query: &'static str, name: String },
    #[error("grammar: {0}")]
    Language(#[from] tree_sitter::LanguageError),
}

pub struct Driver {
    language: tree_sitter::Language,
    queries: Compiled,
    hooks: Box<dyn LanguageHooks>,
}

impl Driver {
    pub fn new(
        language: tree_sitter::Language,
        queries: QuerySet,
        hooks: Box<dyn LanguageHooks>,
    ) -> Result<Self, DriverError> {
        tree_sitter::Parser::new().set_language(&language)?;
        let queries = Compiled::new(&language, queries)?;
        Ok(Self {
            language,
            queries,
            hooks,
        })
    }

    /// Extracts one file. The result depends only on `path` and `bytes`.
    pub fn parse(&self, path: &RepoPath, bytes: &[u8]) -> Result<ParsedFile, AdapterError> {
        extract::run(self, path, bytes)
    }
}
