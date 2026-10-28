#![allow(clippy::unwrap_used)]

use std::path::Path;
use std::process::{Command, Stdio};

#[test]
fn every_committed_scenario_loads() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut checked = 0;
    for dir in [
        "codemycel-lsp/tests/scenarios",
        "codemycel-index/tests/scenarios",
    ] {
        let Ok(entries) = std::fs::read_dir(crates.join(dir)) else {
            continue;
        };
        for entry in entries {
            let path = entry.unwrap().path();
            if path.extension().is_none_or(|ext| ext != "json") {
                continue;
            }
            let output = Command::new(env!("CARGO_BIN_EXE_codemycel-fake-lsp"))
                .arg(&path)
                .stdin(Stdio::null())
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}: {}",
                path.display(),
                String::from_utf8_lossy(&output.stderr)
            );
            checked += 1;
        }
    }
    assert!(checked > 0, "no scenarios found");
}
