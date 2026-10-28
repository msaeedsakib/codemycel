use std::path::Path;

use codemycel_core::NoteKind;
use codemycel_core::query::{Answer, Budget};

use crate::IndexError;

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

impl NoteTool {
    /// Writes the note into the index at `repo_root`; when `read_only` (another session holds
    /// `session.lock`), writes nothing and says so.
    pub fn write(
        repo_root: &Path,
        request: &NoteRequest<'_>,
        read_only: bool,
        budget: Budget,
    ) -> Result<Answer, IndexError> {
        let _ = (repo_root, request, read_only, budget);
        Err(IndexError::Io {
            path: repo_root.to_owned(),
            source: std::io::Error::other("not implemented: note"),
        })
    }
}
