use std::path::Path;

use codemycel_core::config::paths::{REPO_DIR_NAME, notes_dir};
use codemycel_core::query::{
    Answer, Budget, Line, NodeLine, Outcome, QueryEngine, QueryError, Section,
};
use codemycel_core::store::{Store, StoreRead};
use codemycel_core::{LanguageAdapter, NodeKind, Note, NoteId, NoteKind};

use super::{NotesReconciler, REPOSITORY_TARGET};
use crate::{INDEX_FILE_NAME, IndexError};

/// One call of the `note` tool.
#[derive(Clone, Copy, Debug)]
pub struct NoteRequest<'a> {
    /// A name, semantic path, file path, `community:<id>`, package name, or `repo`.
    pub target: &'a str,
    pub body: &'a str,
    pub kind: NoteKind,
    /// The uuid of a note to replace.
    pub replace: Option<&'a str>,
}

/// The one writing tool: stores a note through `NotesReconciler::write`.
pub struct NoteTool;

const NO_ADAPTERS: &[Box<dyn LanguageAdapter>] = &[];

impl NoteTool {
    /// Writes the note into the index at `repo_root`; when `read_only` (another session holds
    /// `session.lock`), writes nothing and says so. The target is resolved like any name, so an
    /// ambiguous one answers with the candidates. Replacing keeps the note's `created` time.
    pub fn write(
        repo_root: &Path,
        request: &NoteRequest<'_>,
        read_only: bool,
        budget: Budget,
    ) -> Result<Answer, IndexError> {
        let db = repo_root.join(REPO_DIR_NAME).join(INDEX_FILE_NAME);
        let engine = QueryEngine::open(repo_root, Store::open(&db, true)?, NO_ADAPTERS);
        let answer = |line: String| {
            let section = Section::new("note", 0, vec![Line::text(line)]).without_header();
            engine.finish(Outcome::Ok, vec![section], budget, "")
        };
        if read_only {
            return answer("index is read-only: another session is watching".to_owned())
                .map_err(query_error);
        }
        let target = request.target.trim();
        let (target, shown) = if target == REPOSITORY_TARGET {
            (
                REPOSITORY_TARGET.to_owned(),
                Line::field("target", REPOSITORY_TARGET),
            )
        } else {
            match engine.lookup(target, budget).map_err(query_error)? {
                Ok(node) if node.node.kind == NodeKind::Note => {
                    return engine
                        .not_found(
                            format!("a note cannot target another note: `{target}`"),
                            budget,
                        )
                        .map_err(query_error);
                }
                Ok(node) => (node.node.path.to_string(), Line::Node(NodeLine::of(&node))),
                Err(answer) => return Ok(answer),
            }
        };
        let now = jiff::Timestamp::now().to_string();
        let (uuid, created) = match request.replace {
            Some(uuid) => {
                let uuid = NoteId(uuid.trim().to_owned());
                match engine.store().note_by_uuid(&uuid)? {
                    Some(existing) => (uuid, existing.note.created),
                    None => {
                        return engine
                            .not_found(format!("no note `{}` to replace", uuid.0), budget)
                            .map_err(query_error);
                    }
                }
            }
            None => (NotesReconciler::new_id(), now.clone()),
        };
        let note = Note {
            uuid,
            target_semantic_path: target,
            kind: request.kind,
            body: request.body.trim_end_matches('\n').to_owned(),
            created,
            updated: now,
        };
        let mut store = Store::open(&db, false)?;
        let uuid = NotesReconciler::write(&mut store, &notes_dir(repo_root), note)?;
        let lines = vec![Line::field("uuid", &uuid.0), shown];
        let section = Section::new("note", 0, lines).without_header();
        engine
            .finish(Outcome::Ok, vec![section], budget, "")
            .map_err(query_error)
    }
}

fn query_error(error: QueryError) -> IndexError {
    match error {
        QueryError::Store(error) => IndexError::Store(error),
        QueryError::Io { path, source } => IndexError::Io { path, source },
    }
}
