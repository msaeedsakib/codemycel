use std::time::Duration;

use codemycel_core::{Root, RootStatus};
use codemycel_index::{Phase, Progress};

/// One plain line per phase and per enriched root on stderr, with elapsed time.
pub struct Printer;

impl Progress for Printer {
    fn roots(&self, roots: &[Root]) {
        eprintln!("roots: {}", roots.len());
        for root in roots {
            eprintln!("  {} ({})", display(root), root.language);
        }
    }

    fn phase(&self, phase: Phase, elapsed: Duration) {
        let name = match phase {
            Phase::Walk => "walk",
            Phase::Parse => "parse",
            Phase::Docs => "docs and manifests",
            Phase::Communities => "communities",
            Phase::Write => "write and search index",
        };
        eprintln!("{name} {}", seconds(elapsed));
    }

    fn root(&self, root: &Root, status: &RootStatus, elapsed: Duration) {
        let server = match &status.server {
            Some(server) => format!("{} {} ({})", server.name, server.version, server.source),
            None => "no server".to_owned(),
        };
        let outcome = match status.degraded {
            Some(reason) => format!("degraded: {reason}"),
            None => format!("verified by {}", status.coverage),
        };
        eprintln!(
            "lsp {} ({}): {server}, {outcome} {}",
            display(root),
            root.language,
            seconds(elapsed)
        );
    }
}

/// The root's directory, `.` for the repository root.
pub fn display(root: &Root) -> &str {
    if root.path.is_root() {
        "."
    } else {
        root.path.as_str()
    }
}

pub fn seconds(elapsed: Duration) -> String {
    format!("{:.1}s", elapsed.as_secs_f64())
}
