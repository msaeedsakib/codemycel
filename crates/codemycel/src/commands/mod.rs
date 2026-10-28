mod doctor;
mod impact;
mod init;
mod mcp;
mod note;
mod notes;
mod overview;
mod search;
mod setup;
mod status;
mod symbol;
mod sync;
mod trace;
mod ui;
mod unresolved;
mod upgrade;

use anyhow::Context as _;
use codemycel_core::query::{
    Answer, CoverageState, JsonFormatter, Outcome, QueryEngine, QueryError, TextFormatter,
};
use codemycel_core::store::{SCHEMA_VERSION, Store, StoreError};
use codemycel_index::{Registry, RepoLocation, RepoLocator, RepoRegistry};

use crate::cli::{Command, Context};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExitCode {
    Ok = 0,
    Error = 1,
    /// A root is degraded or a name lookup was ambiguous.
    Degraded = 2,
}

impl From<ExitCode> for std::process::ExitCode {
    fn from(code: ExitCode) -> Self {
        Self::from(code as u8)
    }
}

pub fn dispatch(command: &Command, ctx: &Context) -> anyhow::Result<ExitCode> {
    match command {
        Command::Init(args) => init::run(args, ctx),
        Command::Sync(args) => sync::run(args, ctx),
        Command::Mcp(args) => mcp::run(args, ctx),
        Command::Ui(args) => ui::run(args, ctx),
        Command::Setup(args) => setup::run(args, ctx),
        Command::Doctor(args) => doctor::run(args, ctx),
        Command::Upgrade(args) => upgrade::run(args, ctx),
        Command::Search(args) => search::run(args, ctx),
        Command::Symbol(args) => symbol::run(args, ctx),
        Command::Impact(args) => impact::run(args, ctx),
        Command::Trace(args) => trace::run(args, ctx),
        Command::Overview(args) => overview::run(args, ctx),
        Command::Unresolved(args) => unresolved::run(args, ctx),
        Command::Status(args) => status::run(args, ctx),
        Command::Note(args) => note::run(args, ctx),
        Command::Notes(args) => notes::run(args, ctx),
    }
}

/// The repository `--repo` or the current directory is in.
fn locate(ctx: &Context) -> anyhow::Result<RepoLocation> {
    let start = match &ctx.repo {
        Some(path) => path.clone(),
        None => std::env::current_dir().context("reading the current directory")?,
    };
    let start =
        std::path::absolute(&start).with_context(|| format!("resolving {}", start.display()))?;
    Ok(RepoLocator::locate(
        &start,
        std::env::home_dir().as_deref(),
        codemycel_core::config::home_dir().ok().as_deref(),
    ))
}

/// The read-only index of `location`, or `None` after saying why there is none: no index (with
/// the registry's repositories listed on stderr for information) or one of another schema.
fn open_index(location: &RepoLocation) -> anyhow::Result<Option<Store>> {
    let db = location.index_path();
    let repo = location.root.display();
    if !db.exists() {
        println!("no index: run codemycel init in {repo}");
        if let Ok(home) = codemycel_core::config::home_dir() {
            let registry = RepoRegistry::load(&home);
            if !registry.entries().is_empty() {
                eprintln!("indexed repositories:");
                for entry in registry.entries() {
                    eprintln!("  {}", entry.path.display());
                }
            }
        }
        return Ok(None);
    }
    match Store::open(&db, true) {
        Ok(store) => Ok(Some(store)),
        Err(StoreError::SchemaMismatch { found, .. }) => {
            println!(
                "index schema {found} is out of date (expected {SCHEMA_VERSION}): run codemycel init in {repo}"
            );
            Ok(None)
        }
        Err(error) => Err(error).with_context(|| format!("opening {}", db.display())),
    }
}

/// Runs one query tool on the repository's index and prints its answer.
fn query(
    ctx: &Context,
    run: impl FnOnce(&QueryEngine<'_>) -> Result<Answer, QueryError>,
) -> anyhow::Result<ExitCode> {
    let location = locate(ctx)?;
    let Some(store) = open_index(&location)? else {
        return Ok(ExitCode::Degraded);
    };
    let registry = Registry::default();
    let engine = QueryEngine::open(&location.root, store, registry.adapters());
    let answer = run(&engine)?;
    print_answer(&answer, ctx.json)
}

/// Prints `answer` as text or JSON. Exits 2 when ambiguous or when a root is degraded, 1 when
/// nothing matched.
fn print_answer(answer: &Answer, json: bool) -> anyhow::Result<ExitCode> {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&JsonFormatter::render(answer))?
        );
    } else {
        print!("{}", TextFormatter::render(answer));
    }
    let degraded = answer
        .freshness
        .coverage
        .iter()
        .flat_map(|language| &language.states)
        .any(|state| matches!(state, CoverageState::Degraded { .. }));
    Ok(match answer.outcome {
        Outcome::Ambiguous => ExitCode::Degraded,
        Outcome::NotFound => ExitCode::Error,
        Outcome::Ok if degraded => ExitCode::Degraded,
        Outcome::Ok => ExitCode::Ok,
    })
}

fn not_implemented(command: &str) -> anyhow::Result<ExitCode> {
    eprintln!("not implemented: {command}");
    Ok(ExitCode::Error)
}
