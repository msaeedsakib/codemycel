use std::path::PathBuf;

use anyhow::Context as _;

/// Prints `<id>\t<kind>\t<check>\t<signature>\t<prompt>` per task in a tasks file, the prompt on
/// one line and `-` for a missing field, for `run.sh` and `check.sh`.
fn main() -> anyhow::Result<()> {
    let path = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .context("usage: tasks <eval/tasks/<repo>.toml>")?;
    for task in codemycel_eval::load_tasks(&path)? {
        let prompt = task.prompt.split_whitespace().collect::<Vec<_>>().join(" ");
        println!(
            "{}\t{}\t{}\t{}\t{prompt}",
            task.id,
            task.kind,
            task.check.as_deref().unwrap_or("-"),
            task.signature.as_deref().unwrap_or("-"),
        );
    }
    Ok(())
}
