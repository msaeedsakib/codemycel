#![allow(clippy::unwrap_used)]

mod harness;

use std::path::{Path, PathBuf};

use codemycel_core::config::Config;
use codemycel_core::config::paths::notes_dir;
use codemycel_core::query::symbol::SymbolOptions;
use codemycel_core::query::{Answer, Budget, Outcome, QueryEngine, TextFormatter};
use codemycel_core::store::{Store, StoreRead};
use codemycel_core::{Note, NoteId, NoteKind};
use codemycel_index::{NoteRequest, NoteTool, NotesReconciler, Pipeline, Registry};
use harness::{fixture, fixture_copy};
use tempfile::TempDir;

const SIDES: &str = "crates/core/src/geometry.rs::sides";

/// A parse-only index of a copy of the Rust fixture at `<copy>/.codemycel/index.db`.
fn indexed() -> (TempDir, PathBuf) {
    let (temp, copy) = fixture_copy(&fixture("rust/workspace-basic"));
    let home = temp.path().join("home");
    std::fs::create_dir(&home).unwrap();
    std::fs::create_dir(copy.join(".codemycel")).unwrap();
    let config = Config::load_with_home(&copy, &home).unwrap();
    let mut store = Store::create(&copy.join(".codemycel/index.db")).unwrap();
    Pipeline::parse_only(&copy, &config, &mut store, &Registry::default()).unwrap();
    (temp, copy)
}

fn store(repo: &Path) -> Store {
    Store::open(&repo.join(".codemycel/index.db"), true).unwrap()
}

fn note(repo: &Path, target: &str, body: &str, replace: Option<&str>) -> Answer {
    let request = NoteRequest {
        target,
        body,
        kind: NoteKind::Note,
        replace,
    };
    NoteTool::write(repo, &request, false, Budget::new(500)).unwrap()
}

/// The uuid an answer of the note tool reports.
fn uuid_of(answer: &Answer) -> String {
    let text = TextFormatter::render(answer);
    text.lines()
        .find_map(|line| line.strip_prefix("uuid  "))
        .unwrap_or_else(|| panic!("{text}"))
        .to_owned()
}

#[test]
fn note_write_creates_file_and_card_entry() {
    let (_temp, repo) = indexed();
    let answer = note(
        &repo,
        "sides",
        "Counts corners, not edges.\nSee the tests.\n",
        None,
    );
    let uuid = uuid_of(&answer);
    assert_eq!(
        TextFormatter::render(&answer).replace(&uuid, "<uuid>"),
        "uuid  <uuid>
target  crates/core/src/geometry.rs::sides (function)
index: no git (0 files changed since) · coverage: rust not verified
"
    );
    let file = std::fs::read_to_string(notes_dir(&repo).join(format!("{uuid}.md"))).unwrap();
    assert!(
        file.starts_with(&format!("---\ntarget: \"{SIDES}\"\nkind: note\ncreated: ")),
        "{file}"
    );
    assert!(
        file.ends_with("---\nCounts corners, not edges.\nSee the tests.\n"),
        "{file}"
    );
    let registry = Registry::default();
    let engine = QueryEngine::open(&repo, store(&repo), registry.adapters());
    let options = SymbolOptions {
        include_body: false,
        callers: false,
        callees: false,
        limit: 25,
    };
    let card = TextFormatter::render(&engine.symbol(SIDES, options, Budget::new(4000)).unwrap());
    let today = &jiff::Timestamp::now().to_string()[..10];
    assert!(
        card.contains(&format!(
            "\n  note \"Counts corners, not edges.\" ({today})\n"
        )),
        "{card}"
    );
    assert!(
        card.contains(&format!("\nnotes:\n  note {uuid} on {SIDES} (")),
        "{card}"
    );
}

#[test]
fn note_write_replace_keeps_created_and_bumps_updated() {
    let (_temp, repo) = indexed();
    let uuid = uuid_of(&note(&repo, "repo", "First.", None));
    let first = store(&repo)
        .note_by_uuid(&NoteId(uuid.clone()))
        .unwrap()
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(5));
    let replaced = note(&repo, "crates/core/src/lib.rs", "Second.", Some(&uuid));
    assert_eq!(uuid_of(&replaced), uuid);
    let second = store(&repo)
        .note_by_uuid(&NoteId(uuid.clone()))
        .unwrap()
        .unwrap();
    assert_eq!(second.note.created, first.note.created);
    assert!(
        second.note.updated.parse::<jiff::Timestamp>().unwrap()
            > first.note.updated.parse::<jiff::Timestamp>().unwrap()
    );
    assert_eq!(second.note.body, "Second.");
    assert_eq!(second.note.target_semantic_path, "crates/core/src/lib.rs");
    assert_eq!(store(&repo).notes().unwrap().len(), 1);
}

#[test]
fn note_write_refuses_read_only_and_ambiguous_targets() {
    let (_temp, repo) = indexed();
    let request = NoteRequest {
        target: SIDES,
        body: "Never written.",
        kind: NoteKind::Note,
        replace: None,
    };
    let read_only = NoteTool::write(&repo, &request, true, Budget::new(500)).unwrap();
    assert_eq!(
        TextFormatter::render(&read_only).lines().next(),
        Some("index is read-only: another session is watching")
    );
    let ambiguous = note(&repo, "new", "Which one?", None);
    assert_eq!(ambiguous.outcome, Outcome::Ambiguous);
    assert!(store(&repo).notes().unwrap().is_empty());
    assert!(
        !notes_dir(&repo).exists() || std::fs::read_dir(notes_dir(&repo)).unwrap().count() == 0
    );
}

#[test]
fn notes_lists_orphans() {
    let (_temp, repo) = indexed();
    let mut writable = Store::open(&repo.join(".codemycel/index.db"), false).unwrap();
    for (uuid, target) in [
        ("0b6f2c1e-4a8d-4f3e-9a51-2c7d8e9f0a1b", SIDES),
        (
            "1c7f3d2e-5b9e-4a4f-8b62-3d8e9f0a1b2c",
            "crates/core/src/gone.rs::vanished",
        ),
        ("2d8a4e3f-6c0f-4b5a-9c73-4e9f0a1b2c3d", "repo"),
    ] {
        NotesReconciler::write(
            &mut writable,
            &notes_dir(&repo),
            Note {
                uuid: NoteId(uuid.to_owned()),
                target_semantic_path: target.to_owned(),
                kind: NoteKind::Note,
                body: format!("On {target}."),
                created: "2026-10-27T09:00:00Z".to_owned(),
                updated: "2026-10-28T09:00:00Z".to_owned(),
            },
        )
        .unwrap();
    }
    let registry = Registry::default();
    let engine = QueryEngine::open(&repo, store(&repo), registry.adapters());
    let render = |target, orphans| {
        TextFormatter::render(&engine.notes(target, orphans, Budget::new(2000)).unwrap())
    };
    assert_eq!(
        render(None, true),
        "notes:
  note 1c7f3d2e-5b9e-4a4f-8b62-3d8e9f0a1b2c on crates/core/src/gone.rs::vanished (orphaned) (2026-10-28)
    On crates/core/src/gone.rs::vanished.
index: no git (0 files changed since) · coverage: rust not verified
"
    );
    assert_eq!(
        render(None, false),
        "notes:
  note 0b6f2c1e-4a8d-4f3e-9a51-2c7d8e9f0a1b on crates/core/src/geometry.rs::sides (2026-10-28)
    On crates/core/src/geometry.rs::sides.
  note 1c7f3d2e-5b9e-4a4f-8b62-3d8e9f0a1b2c on crates/core/src/gone.rs::vanished (orphaned) (2026-10-28)
    On crates/core/src/gone.rs::vanished.
  note 2d8a4e3f-6c0f-4b5a-9c73-4e9f0a1b2c3d on repo (2026-10-28)
    On repo.
index: no git (0 files changed since) · coverage: rust not verified
"
    );
    assert_eq!(
        render(Some("sides"), false),
        "notes:
  note 0b6f2c1e-4a8d-4f3e-9a51-2c7d8e9f0a1b on crates/core/src/geometry.rs::sides (2026-10-28)
    On crates/core/src/geometry.rs::sides.
index: no git (0 files changed since) · coverage: rust not verified
"
    );
    assert_eq!(
        render(Some("repo"), true),
        "no orphaned notes on `repo`
index: no git (0 files changed since) · coverage: rust not verified
"
    );
}
