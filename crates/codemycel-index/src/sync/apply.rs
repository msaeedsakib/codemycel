use std::collections::BTreeMap;

use codemycel_core::RepoPath;
use codemycel_core::store::Tx;

use super::changes::ChangeSet;
use super::plan::SyncPlan;
use crate::IndexError;
use crate::pipeline::{ParsedCode, Staged, Verified, verify_phase, write_phase};

/// Applies an incremental plan's code-layer writes: deleted files with everything they own, the
/// rows of added, modified, and re-verified files, the nodes of rewritten files, and the
/// verification of re-verified files, which are the files `verified` rebuilt. The caller's transaction continues with the docs layer,
/// notes, communities, and meta.
pub fn run(
    tx: &mut Tx<'_>,
    staged: &Staged,
    changes: &ChangeSet,
    plan: &SyncPlan,
    verified: &Verified,
) -> Result<(), IndexError> {
    for path in &changes.deleted {
        tx.delete_file(path)?;
    }
    let records = write_phase::records(
        tx,
        &staged.files,
        staged.roots.roots(),
        &staged.assignment,
        &staged.parsed,
    )?;
    for record in records {
        let touched = changes.added.contains(&record.path)
            || changes.modified.contains(&record.path)
            || verified.files.contains(&record.path);
        if touched {
            tx.upsert_file(&record)?;
        }
    }
    let parsed: BTreeMap<&RepoPath, &ParsedCode> = staged
        .parsed
        .iter()
        .map(|code| (&staged.files[code.file].path, code))
        .collect();
    for path in &plan.rewrite {
        write_phase::replace_file(tx, path, parsed.get(path).copied())?;
    }
    for path in &verified.files {
        tx.reset_verification(path)?;
    }
    verify_phase::write(tx, verified)
}
