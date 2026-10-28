use std::ffi::OsStr;
use std::path::PathBuf;
use std::process::{Command, Stdio};

/// The first `<dir>/<name><exe suffix>` that is a file, over the directories of `search_path`.
pub fn find_on_path(name: &str, search_path: Option<&OsStr>) -> Option<PathBuf> {
    let file = format!("{name}{}", std::env::consts::EXE_SUFFIX);
    std::env::split_paths(search_path?)
        .map(|dir| dir.join(&file))
        .find(|candidate| candidate.is_file())
}

/// Runs a toolchain command whose program is looked up on `search_path`; its trimmed stdout on
/// success. `None` when the program is absent or the command fails.
pub fn run_tool(command: &[&str], search_path: Option<&OsStr>) -> Option<String> {
    let (program, args) = command.split_first()?;
    let program = find_on_path(program, search_path)?;
    let mut cmd = Command::new(program);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    if let Some(path) = search_path {
        cmd.env("PATH", path);
    }
    let output = cmd.output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}
