use std::collections::BTreeMap;
use std::path::PathBuf;

use notify::event::ModifyKind;
use notify::{Event, EventKind};

use super::rules::Rules;
use crate::sync::ChangeSet;
use crate::walk::repo_path;

/// Paths touched since the last change set, each flagged when an event created or renamed it.
#[derive(Default)]
pub(super) struct Pending {
    touched: BTreeMap<PathBuf, bool>,
    rescan: bool,
}

impl Pending {
    /// Keeps the event's paths the rules watch; `true` when it kept anything. Reads and
    /// metadata-only changes are dropped.
    pub(super) fn record(&mut self, rules: &mut Rules, event: &Event) -> bool {
        if matches!(
            event.kind,
            EventKind::Access(_) | EventKind::Modify(ModifyKind::Metadata(_))
        ) {
            return false;
        }
        let mut kept = false;
        if event.need_rescan() {
            self.rescan = true;
            kept = true;
        }
        let created = matches!(
            event.kind,
            EventKind::Create(_) | EventKind::Modify(ModifyKind::Name(_))
        );
        for path in &event.paths {
            let Some(relative) = rules.relative(path) else {
                continue;
            };
            rules.forget_if_ignore_file(&relative);
            if !rules.watched(&relative) {
                continue;
            }
            *self.touched.entry(relative).or_default() |= created;
            kept = true;
        }
        kept
    }

    pub(super) fn is_empty(&self) -> bool {
        self.touched.is_empty() && !self.rescan
    }

    /// The touched paths classified by what is on disk now: gone is deleted, created or renamed
    /// into place is added, anything else is modified. A directory counts only when it appeared.
    /// `None` when there is nothing to report.
    pub(super) fn take(&mut self, rules: &Rules) -> Option<ChangeSet> {
        let rescan = std::mem::take(&mut self.rescan);
        let mut changes = ChangeSet::default();
        for (relative, created) in std::mem::take(&mut self.touched) {
            let Some(path) = repo_path(&relative) else {
                continue;
            };
            match std::fs::symlink_metadata(rules.absolute(&relative)) {
                Err(_) => {
                    changes.deleted.insert(path);
                }
                Ok(metadata) if metadata.is_dir() && !created => {}
                Ok(_) if created => {
                    changes.added.insert(path);
                }
                Ok(_) => {
                    changes.modified.insert(path);
                }
            }
        }
        (rescan || !changes.is_empty()).then_some(changes)
    }
}
