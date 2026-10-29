use std::path::PathBuf;

use anyhow::Context as _;

/// Prints `<id>\t<prompt>` per task in a tasks file, the prompt on one line, for `run.sh`.
fn main() -> anyhow::Result<()> {
    let path = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .context("usage: tasks <eval/tasks/<repo>.toml>")?;
    for task in codemycel_eval::load_tasks(&path)? {
        let prompt = task.prompt.split_whitespace().collect::<Vec<_>>().join(" ");
        println!("{}\t{prompt}", task.id);
    }
    Ok(())
}
