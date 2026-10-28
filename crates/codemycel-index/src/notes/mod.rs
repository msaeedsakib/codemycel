use std::path::Path;

use codemycel_core::store::{Store, Tx};
use codemycel_core::{Note, NoteId};

use crate::IndexError;

pub struct NotesReconciler;

impl NotesReconciler {
    /// Mirrors `.codemycel/notes/<uuid>.md` into the notes table: every file's note upserted by
    /// uuid, and every row whose file is gone deleted. The files are the source of truth.
    pub fn load(_tx: &mut Tx<'_>, _notes_dir: &Path) -> Result<(), IndexError> {
        Ok(())
    }

    /// Resolves every note's target by semantic path, attaching it with an `annotates` edge or
    /// flagging it orphaned. Runs after every node a note can target is written.
    pub fn attach(_tx: &mut Tx<'_>) -> Result<(), IndexError> {
        Ok(())
    }

    /// Writes the note's file, then its row and target link, for the `note` tool.
    pub fn write(_store: &mut Store, _notes_dir: &Path, note: Note) -> Result<NoteId, IndexError> {
        Ok(note.uuid)
    }
}
