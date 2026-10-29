mod files;
pub mod tool;

use std::collections::BTreeSet;
use std::io;
use std::path::{Path, PathBuf};

use codemycel_core::store::{NoteTarget, Store, StoreRead, StoredNote, Tx};
use codemycel_core::{NodeKind, NodeRef, Note, NoteId, NoteKind, SemanticPath};

use crate::{IndexError, communities};

/// The target of a note on the repository as a whole. It names no node, so such a note has no
/// `annotates` edge and is never orphaned.
pub const REPOSITORY_TARGET: &str = "repo";

pub struct NotesReconciler;

impl NotesReconciler {
    /// Mirrors `.codemycel/notes/<uuid>.md` into the notes table: every file's note upserted by
    /// uuid, and every row whose file is gone deleted. The files are the source of truth. A file
    /// that cannot be read or parsed is skipped with a warning and its row left as it was; a
    /// missing directory holds no notes.
    pub fn load(tx: &mut Tx<'_>, notes_dir: &Path) -> Result<(), IndexError> {
        let mut present = BTreeSet::new();
        for (uuid, path) in note_files(notes_dir)? {
            present.insert(uuid.clone());
            let text = match std::fs::read_to_string(&path) {
                Ok(text) => text,
                Err(error) => {
                    tracing::warn!("skipping note {}: {error}", path.display());
                    continue;
                }
            };
            match files::parse(uuid, &text) {
                Ok(note) => {
                    tx.upsert_note(&note)?;
                }
                Err(error) => tracing::warn!("skipping note {}: {error}", path.display()),
            }
        }
        for stored in tx.notes()? {
            if !present.contains(&stored.note.uuid) {
                tx.delete_note(&stored.note.uuid)?;
            }
        }
        Ok(())
    }

    /// Resolves every note's target by semantic path, attaching it with an `annotates` edge or
    /// flagging it orphaned. Runs after every node a note can target is written.
    ///
    /// When several kinds share the target path (a TypeScript `const Foo` and `type Foo`, a
    /// manifest's file and package nodes), the note annotates the first of them in this order:
    /// package, doc, then the remaining kinds in `NodeKind` order. `repo` names the repository.
    pub fn attach(tx: &mut Tx<'_>) -> Result<(), IndexError> {
        for stored in tx.notes()? {
            attach_one(tx, &stored)?;
        }
        Ok(())
    }

    /// Writes the note's file, creating the notes directory if needed, then upserts its row and
    /// attaches it in one transaction, for the `note` tool. A `label` note on a community relabels
    /// it in that transaction, as the next sync would. The note must survive being read back
    /// from its file: a canonical uuid, and RFC 3339 `created` and `updated` times.
    pub fn write(store: &mut Store, notes_dir: &Path, note: Note) -> Result<NoteId, IndexError> {
        let path = notes_dir.join(files::file_name(&note.uuid));
        let invalid = |reason: String| IndexError::Io {
            path: path.clone(),
            source: io::Error::new(io::ErrorKind::InvalidInput, reason),
        };
        if !files::is_uuid(&note.uuid) {
            return Err(invalid(format!(
                "`{}` is not a lowercase uuid",
                note.uuid.0
            )));
        }
        let text = files::render(&note);
        match files::parse(note.uuid.clone(), &text) {
            Ok(read_back) if read_back == note => {}
            Ok(_) => return Err(invalid("the note does not read back as written".to_owned())),
            Err(error) => return Err(invalid(error.to_string())),
        }
        let io_error = |path: &Path| {
            let path = path.to_path_buf();
            move |source| IndexError::Io { path, source }
        };
        std::fs::create_dir_all(notes_dir).map_err(io_error(notes_dir))?;
        let partial = notes_dir.join(format!("{}.tmp", files::file_name(&note.uuid)));
        std::fs::write(&partial, text).map_err(io_error(&partial))?;
        std::fs::rename(&partial, &path).map_err(io_error(&path))?;
        store.transaction(|tx| {
            tx.upsert_note(&note)?;
            let Some(stored) = tx.note_by_uuid(&note.uuid)? else {
                return Ok(());
            };
            if let Target::Node(target) = attach_one(tx, &stored)?
                && note.kind == NoteKind::Label
                && target.kind == NodeKind::Community
                && let Some(id) = target.path.as_str().strip_prefix("community:")
            {
                communities::apply_label_notes(tx, id)?;
            }
            Ok::<_, IndexError>(())
        })?;
        Ok(note.uuid)
    }

    /// A fresh note id: a random (v4) uuid.
    pub fn new_id() -> NoteId {
        NoteId(uuid::Uuid::new_v4().hyphenated().to_string())
    }
}

/// The note files in `notes_dir` with their uuids, sorted by file name.
fn note_files(notes_dir: &Path) -> Result<Vec<(NoteId, PathBuf)>, IndexError> {
    let entries = match std::fs::read_dir(notes_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(source) => {
            return Err(IndexError::Io {
                path: notes_dir.to_path_buf(),
                source,
            });
        }
    };
    let mut found = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|source| IndexError::Io {
            path: notes_dir.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        match files::uuid_of(&name) {
            Some(uuid) => found.push((name, uuid, path)),
            None if name.ends_with(".md") => {
                tracing::warn!(
                    "skipping {}: a note file is named `<uuid>.md`",
                    path.display()
                );
            }
            None => {}
        }
    }
    found.sort_unstable_by(|a, b| a.0.cmp(&b.0));
    Ok(found
        .into_iter()
        .map(|(_, uuid, path)| (uuid, path))
        .collect())
}

enum Target {
    Repository,
    Node(NodeRef),
    Missing,
}

fn resolve(tx: &Tx<'_>, target: &str) -> Result<Target, IndexError> {
    if target == REPOSITORY_TARGET {
        return Ok(Target::Repository);
    }
    let Ok(path) = target.parse::<SemanticPath>() else {
        return Ok(Target::Missing);
    };
    let best = tx
        .node_by_semantic_path(&path, None)?
        .into_iter()
        .map(|stored| stored.node.kind)
        .min_by_key(|&kind| precedence(kind));
    Ok(best.map_or(Target::Missing, |kind| {
        Target::Node(NodeRef::new(path, kind))
    }))
}

fn precedence(kind: NodeKind) -> usize {
    match kind {
        NodeKind::Package => 0,
        NodeKind::Doc => 1,
        other => 2 + NodeKind::ALL.iter().position(|&k| k == other).unwrap_or(0),
    }
}

fn attach_one(tx: &mut Tx<'_>, stored: &StoredNote) -> Result<Target, IndexError> {
    let uuid = &stored.note.uuid;
    let target = resolve(tx, &stored.note.target_semantic_path)?;
    match &target {
        Target::Node(node) => {
            if stored.orphaned || stored.target.as_ref() != Some(node) {
                tx.set_note_target(uuid, NoteTarget::Node(node))?;
            }
        }
        Target::Repository => {
            if stored.orphaned || stored.target.is_some() {
                tx.set_note_target(uuid, NoteTarget::Repository)?;
            }
        }
        Target::Missing => {
            if !stored.orphaned || stored.target.is_some() {
                tx.set_note_target(uuid, NoteTarget::Orphaned)?;
            }
        }
    }
    Ok(target)
}
