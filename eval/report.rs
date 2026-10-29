use std::path::PathBuf;

fn main() -> anyhow::Result<()> {
    let logs = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("logs"));
    print!("{}", codemycel_eval::table(&logs)?);
    Ok(())
}
