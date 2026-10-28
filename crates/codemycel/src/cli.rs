#![expect(dead_code, reason = "stub commands do not read their arguments yet")]

use std::path::PathBuf;

use clap::{ArgAction, Args, Parser, Subcommand};
use codemycel_core::{EdgeKind, NodeKind, NoteKind, UnresolvedReason};

#[derive(Debug, Parser)]
#[command(
    name = "codemycel",
    version,
    about = "Verified code intelligence for coding agents"
)]
pub struct Cli {
    #[command(flatten)]
    pub context: Context,
    #[command(subcommand)]
    pub command: Command,
}

/// Flags every command accepts, before or after the subcommand.
#[derive(Debug, Args)]
pub struct Context {
    /// Repository to operate on; defaults to the one containing the current directory
    #[arg(long, global = true, value_name = "PATH")]
    pub repo: Option<PathBuf>,
    /// Print JSON instead of text
    #[arg(long, global = true)]
    pub json: bool,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Index the repository for the first time
    Init(InitArgs),
    /// Bring the index up to date with the working tree
    Sync(SyncArgs),
    /// Serve the MCP tools over stdio
    Mcp(McpArgs),
    /// Open the web view
    Ui(UiArgs),
    /// Register codemycel with installed coding agents
    Setup(SetupArgs),
    /// Diagnose servers, roots, and the index
    Doctor(DoctorArgs),
    /// Replace this binary with the latest release
    Upgrade(UpgradeArgs),
    /// Find symbols by words
    Search(SearchArgs),
    /// Show one symbol with its relations
    Symbol(SymbolArgs),
    /// List what depends on a symbol
    Impact(ImpactArgs),
    /// Find the shortest path between two symbols
    Trace(TraceArgs),
    /// Summarise the repository, a directory, a file, a community, or a package
    Overview(OverviewArgs),
    /// List candidates that could not be verified
    Unresolved(UnresolvedArgs),
    /// Report index freshness, coverage, and counts
    Status(StatusArgs),
    /// Write a note attached to a target
    Note(NoteArgs),
    /// List notes
    Notes(NotesArgs),
}

#[derive(Debug, Args)]
pub struct InitArgs {
    /// Never download language servers
    #[arg(long)]
    pub no_download: bool,
    /// Commit the SCIP index was generated at, instead of HEAD
    #[arg(long, value_name = "SHA")]
    pub scip_commit: Option<String>,
}

#[derive(Debug, Args)]
pub struct SyncArgs {
    /// Re-parse and re-verify everything
    #[arg(long)]
    pub full: bool,
    /// Never download language servers
    #[arg(long)]
    pub no_download: bool,
    /// Commit the SCIP index was generated at, instead of HEAD
    #[arg(long, value_name = "SHA")]
    pub scip_commit: Option<String>,
}

#[derive(Debug, Args)]
pub struct McpArgs {}

#[derive(Debug, Args)]
pub struct UiArgs {
    /// Loopback port; defaults to the first free port from 7111
    #[arg(long)]
    pub port: Option<u16>,
}

#[derive(Debug, Args)]
pub struct SetupArgs {
    /// Remove codemycel's entries instead of writing them
    #[arg(long)]
    pub remove: bool,
}

#[derive(Debug, Args)]
pub struct DoctorArgs {}

#[derive(Debug, Args)]
pub struct UpgradeArgs {}

#[derive(Debug, Args)]
pub struct SearchArgs {
    pub query: String,
    /// Only return nodes of these kinds
    #[arg(long, value_delimiter = ',')]
    pub kind: Vec<NodeKind>,
    #[arg(long, default_value_t = 20)]
    pub limit: u32,
    #[arg(long, default_value_t = 3000)]
    pub max_tokens: u32,
}

#[derive(Debug, Args)]
pub struct SymbolArgs {
    /// Semantic path or bare name
    pub name: String,
    #[arg(long)]
    pub include_body: bool,
    #[arg(long, default_value_t = true, action = ArgAction::Set, value_name = "BOOL")]
    pub callers: bool,
    #[arg(long, default_value_t = true, action = ArgAction::Set, value_name = "BOOL")]
    pub callees: bool,
    /// Maximum entries per list
    #[arg(long, default_value_t = 25)]
    pub limit: u32,
    #[arg(long, default_value_t = 4000)]
    pub max_tokens: u32,
}

#[derive(Debug, Args)]
pub struct ImpactArgs {
    /// Semantic path or bare name
    pub name: String,
    #[arg(long, default_value_t = 2, value_parser = clap::value_parser!(u8).range(1..=5))]
    pub depth: u8,
    /// Edge kinds to follow
    #[arg(
        long,
        value_delimiter = ',',
        default_values_t = [
            EdgeKind::Calls,
            EdgeKind::References,
            EdgeKind::Extends,
            EdgeKind::Implements,
            EdgeKind::Overrides,
            EdgeKind::Imports,
        ]
    )]
    pub kinds: Vec<EdgeKind>,
    #[arg(long, default_value_t = 6000)]
    pub max_tokens: u32,
}

#[derive(Debug, Args)]
pub struct TraceArgs {
    pub from: String,
    pub to: String,
    /// Edge kinds to follow; all verified kinds when omitted
    #[arg(long, value_delimiter = ',')]
    pub kinds: Vec<EdgeKind>,
    #[arg(long, default_value_t = 8)]
    pub max_depth: u32,
    #[arg(long, default_value_t = 2000)]
    pub max_tokens: u32,
}

#[derive(Debug, Args)]
pub struct OverviewArgs {
    /// A directory, file, `community:<id>`, or package name; the repository when omitted
    pub scope: Option<String>,
    #[arg(long, default_value_t = 4000)]
    pub max_tokens: u32,
}

#[derive(Debug, Args)]
pub struct UnresolvedArgs {
    /// A file path or symbol name; a repository summary when omitted
    pub scope: Option<String>,
    #[arg(long)]
    pub reason: Option<UnresolvedReason>,
    #[arg(long, default_value_t = 3000)]
    pub max_tokens: u32,
}

#[derive(Debug, Args)]
pub struct StatusArgs {
    #[arg(long, default_value_t = 1000)]
    pub max_tokens: u32,
}

#[derive(Debug, Args)]
pub struct NoteArgs {
    /// A name, file path, `community:<id>`, package name, or `repo`
    pub target: String,
    /// Markdown body
    #[arg(long)]
    pub body: String,
    #[arg(long, default_value_t = NoteKind::Note)]
    pub kind: NoteKind,
    /// Uuid of a note to replace
    #[arg(long, value_name = "UUID")]
    pub replace: Option<String>,
    #[arg(long, default_value_t = 500)]
    pub max_tokens: u32,
}

#[derive(Debug, Args)]
pub struct NotesArgs {
    pub target: Option<String>,
    /// Only notes whose target no longer exists
    #[arg(long)]
    pub orphans: bool,
    #[arg(long, default_value_t = 2000)]
    pub max_tokens: u32,
}
