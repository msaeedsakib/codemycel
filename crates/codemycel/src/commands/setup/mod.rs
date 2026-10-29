mod claude_code;
mod codex;
mod cursor;
mod gemini;
mod guidance;
mod merge;
mod opencode;
mod state;

use std::io::{BufRead as _, Write as _};
use std::path::{Path, PathBuf};

use anyhow::{Context as _, bail};

use self::merge::Entry;
use self::state::{Created, State};
use super::ExitCode;
use crate::cli::{Context, SetupArgs};

/// A coding agent `setup` can register codemycel with.
pub struct Harness {
    /// The name `--harness` takes.
    id: &'static str,
    name: &'static str,
    /// Present when the harness is installed.
    dir: PathBuf,
    config: PathBuf,
    format: Format,
    entry: Entry,
    /// The global instructions file, when the harness reads one from disk.
    instructions: Option<PathBuf>,
}

pub enum Format {
    Json { servers: &'static str },
    Toml { servers: &'static str },
}

const SERVER_NAME: &str = "codemycel";

/// Writes or, with `--remove`, removes codemycel's MCP server entry and guidance block for the
/// harnesses named by `--harness`, or those picked interactively. Touches only files under the
/// user's home directory.
pub fn run(args: &SetupArgs, _ctx: &Context) -> anyhow::Result<ExitCode> {
    let home = std::env::home_dir().context("finding the home directory")?;
    let harnesses = harnesses(&home);
    let chosen = if args.harness.is_empty() {
        pick(&harnesses, args.remove)?
    } else {
        named(&harnesses, &args.harness)?
    };
    let mut state = State::load()?;
    for harness in chosen {
        if !harness.dir.is_dir() {
            println!("{}: not installed, skipped", harness.name);
            continue;
        }
        if args.remove {
            remove(harness, &mut state)?;
        } else {
            install(harness, &mut state)?;
        }
        state.save()?;
    }
    Ok(ExitCode::Ok)
}

fn harnesses(home: &Path) -> Vec<Harness> {
    vec![
        claude_code::harness(home),
        codex::harness(home),
        cursor::harness(home),
        gemini::harness(home),
        opencode::harness(home),
    ]
}

fn named<'a>(harnesses: &'a [Harness], ids: &[String]) -> anyhow::Result<Vec<&'a Harness>> {
    ids.iter()
        .map(|id| {
            harnesses
                .iter()
                .find(|harness| harness.id == id)
                .with_context(|| {
                    let known: Vec<&str> = harnesses.iter().map(|h| h.id).collect();
                    format!("unknown harness `{id}`; one of: {}", known.join(", "))
                })
        })
        .collect()
}

/// Lists the harnesses with their detection and reads a choice from stdin; an empty answer
/// takes every installed one.
fn pick(harnesses: &[Harness], removing: bool) -> anyhow::Result<Vec<&Harness>> {
    let mut stdout = std::io::stdout().lock();
    for (number, harness) in harnesses.iter().enumerate() {
        let found = if harness.dir.is_dir() {
            "installed"
        } else {
            "not found"
        };
        writeln!(stdout, "  {}. {:<12} {found}", number + 1, harness.name)?;
    }
    let verb = if removing { "Remove from" } else { "Configure" };
    write!(
        stdout,
        "{verb} which? Numbers separated by commas, Enter for every installed one: "
    )?;
    stdout.flush()?;
    let mut answer = String::new();
    std::io::stdin().lock().read_line(&mut answer)?;
    let answer = answer.trim();
    if answer.is_empty() {
        return Ok(harnesses.iter().filter(|h| h.dir.is_dir()).collect());
    }
    answer
        .split(',')
        .map(|part| {
            part.trim()
                .parse::<usize>()
                .ok()
                .and_then(|number| harnesses.get(number.checked_sub(1)?))
                .with_context(|| format!("no harness numbered `{}`", part.trim()))
        })
        .collect()
}

/// Adds the entry and guidance, recording in `state` what was created when it was not there
/// before, so `remove` can restore the files exactly.
fn install(harness: &Harness, state: &mut State) -> anyhow::Result<()> {
    let mut created = None;
    let changed = edit(&harness.config, |text| {
        let (holds_servers, holds_entry) = match harness.format {
            Format::Json { servers } => merge::json_holds(text, servers, SERVER_NAME)?,
            Format::Toml { servers } => merge::toml_holds(text, servers, SERVER_NAME)?,
        };
        if !holds_entry {
            created = Some(Created {
                file: text.is_none(),
                servers: !holds_servers,
            });
        }
        Ok(Some(match harness.format {
            Format::Json { servers } => {
                merge::json_upsert(text, servers, SERVER_NAME, &harness.entry)?
            }
            Format::Toml { servers } => {
                merge::toml_upsert(text, servers, SERVER_NAME, &harness.entry)?
            }
        }))
    })?;
    if let Some(created) = created {
        state.record(harness.id, &harness.config, created);
    }
    report(harness, &harness.config, changed, "MCP server added to");
    match &harness.instructions {
        Some(path) => {
            let mut created = None;
            let changed = edit(path, |text| {
                if !guidance::holds(text) {
                    created = Some(Created {
                        file: text.is_none(),
                        servers: false,
                    });
                }
                Ok(Some(guidance::insert(text)))
            })?;
            if let Some(created) = created {
                state.record(harness.id, path, created);
            }
            report(harness, path, changed, "guidance written to");
        }
        None => println!(
            "{}: keeps its global rules in its settings; paste this block there:\n{}",
            harness.name,
            guidance::BLOCK
        ),
    }
    Ok(())
}

/// Takes out the entry and guidance, and the server map or file only where `state` records that
/// `install` created them.
fn remove(harness: &Harness, state: &mut State) -> anyhow::Result<()> {
    let created = state.take(harness.id, &harness.config);
    let changed = edit(&harness.config, |text| {
        let Some(text) = text else {
            return Ok(None);
        };
        let (rest, empty) = match harness.format {
            Format::Json { servers } => {
                let rest = merge::json_remove(text, servers, SERVER_NAME, created.servers)?;
                let empty = rest.trim() == "{}";
                (rest, empty)
            }
            Format::Toml { servers } => {
                let rest = merge::toml_remove(text, servers, SERVER_NAME)?;
                let empty = rest.trim().is_empty();
                (rest, empty)
            }
        };
        Ok(keep(text, rest, empty, created))
    })?;
    report(harness, &harness.config, changed, "MCP server removed from");
    if let Some(path) = &harness.instructions {
        let created = state.take(harness.id, path);
        let changed = edit(path, |text| {
            Ok(text.and_then(|text| {
                let rest = guidance::remove(text);
                let empty = rest.is_empty();
                keep(text, rest, empty, created)
            }))
        })?;
        report(harness, path, changed, "guidance removed from");
    }
    Ok(())
}

/// `rest`, or `None` to delete the file when `install` created it and removing left it `empty`.
fn keep(text: &str, rest: String, empty: bool, created: Created) -> Option<String> {
    (rest == text || !empty || !created.file).then_some(rest)
}

fn report(harness: &Harness, path: &Path, changed: bool, done: &str) {
    if changed {
        println!("{}: {done} {}", harness.name, path.display());
    } else {
        println!("{}: {} unchanged", harness.name, path.display());
    }
}

/// Replaces the file at `path` with what `change` makes of its text (`None` when missing);
/// `None` back deletes it. The write goes through a sibling temporary file and keeps the
/// file's permissions. True when the file changed.
fn edit(
    path: &Path,
    change: impl FnOnce(Option<&str>) -> anyhow::Result<Option<String>>,
) -> anyhow::Result<bool> {
    let before = match std::fs::read_to_string(path) {
        Ok(text) => Some(text),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error).with_context(|| format!("reading {}", path.display())),
    };
    let after = change(before.as_deref()).with_context(|| format!("editing {}", path.display()))?;
    if after == before {
        return Ok(false);
    }
    match after {
        None => {
            std::fs::remove_file(path).with_context(|| format!("removing {}", path.display()))?
        }
        Some(text) => write(path, &text).with_context(|| format!("writing {}", path.display()))?,
    }
    Ok(true)
}

/// Writes through a symlink to its target, so linked dotfiles stay linked.
fn write(path: &Path, text: &str) -> anyhow::Result<()> {
    let path = &std::fs::canonicalize(path).unwrap_or_else(|_| path.to_owned());
    let Some(dir) = path.parent() else {
        bail!("no parent directory");
    };
    std::fs::create_dir_all(dir)?;
    let mut temp = tempfile::NamedTempFile::new_in(dir)?;
    temp.write_all(text.as_bytes())?;
    if let Ok(metadata) = std::fs::metadata(path) {
        temp.as_file().set_permissions(metadata.permissions())?;
    }
    temp.persist(path)?;
    Ok(())
}
