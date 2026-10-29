# codemycel

A persistent, LSP-verified knowledge graph of your codebase, for coding agents.

codemycel builds a graph of your repository: which function calls which, what implements a trait or interface, what breaks if a signature changes. It hands that graph to your coding agent over MCP, to your shell as CLI commands, and to you as a local web view. Tree-sitter finds the structure. Every cross-file edge is then verified by a language server (rust-analyzer, TypeScript 7) or by a SCIP index. When an edge can't be verified, it is counted as unresolved and never guessed. The graph lives in `.codemycel/` inside the repository, so it outlasts any one agent session.

> **Status: pre-release.** No version has been published yet. The installer below works once the first release is out. The before/after evaluation is in `eval/`, and its results will be published with the first release.

## Install

```sh
curl -fsSL https://raw.githubusercontent.com/msaeedsakib/codemycel/main/install.sh | sh
```

On Windows, in PowerShell:

```powershell
irm https://raw.githubusercontent.com/msaeedsakib/codemycel/main/install.ps1 | iex
```

The installer:

1. downloads the archive for your platform from the latest GitHub release;
2. verifies its SHA-256 checksum;
3. puts `codemycel` in `~/.local/bin` (`%LOCALAPPDATA%\codemycel\bin` on Windows), and warns if that directory isn't on your `PATH`;
4. runs `codemycel setup`.

Supported platforms: Linux x86_64 and aarch64, macOS x86_64 and aarch64, Windows x86_64. There is no `cargo install`, npm package, Homebrew formula or Scoop manifest.

## Use

```sh
codemycel setup          # pick your agents; writes their MCP entry and a short guidance block
cd your-repo
codemycel init           # index the repository
codemycel ui             # open the web view on 127.0.0.1
```

`setup` supports Claude Code, Codex, Cursor, Gemini CLI and OpenCode. For each one you pick, it adds a user-scope MCP server entry (`codemycel mcp`) and a short guidance block in the agent's global instructions file. `codemycel setup --remove` undoes both.

While an agent session runs, `codemycel mcp` watches the repository and keeps the graph current. From the shell, `codemycel sync` does the same.

### Tools

The same tools are available over MCP and on the command line. Add `--json` for machine output.

| Tool | Answers |
|---|---|
| `search` | Which symbols match these words |
| `symbol` | One symbol's card: signature, callers, callees, implementers, notes, and optionally its body |
| `impact` | What depends on a symbol, up to five hops |
| `trace` | The shortest path between two symbols, stepping through trait and interface dispatch |
| `overview` | The repository, a directory, file, package or community at a glance |
| `unresolved` | References that couldn't be verified, and why |
| `status` | Freshness, coverage per language, counts |
| `note`, `notes` | Durable notes an agent attaches to a symbol or the repository |

Every answer ends with a freshness line: the indexed commit, any files changed since, and each language's coverage. Answers are cut to a token budget, and when one is cut it says so.

`codemycel doctor` diagnoses language servers, roots and the index. `codemycel upgrade` replaces the binary with the latest release.

## What "verified" means

- **Verified edges.** An edge such as `calls`, `references` or `implements` exists only when rust-analyzer, TypeScript 7 or a SCIP index confirmed it.
- **Unresolved references.** A reference no server could resolve is stored as unresolved, with a reason (`no_definition`, `external_dependency`, `server_unavailable`, …). It is never turned into a guessed edge.
- **Counts are lower bounds.** "12 callers" means at least 12 verified callers.
- **Degraded roots are reported.** A language root whose server couldn't start shows as degraded in every answer's coverage line and in `doctor`.
- **Communities are heuristics.** Communities are clusters computed from the verified graph. They are labelled `derived` wherever they appear.

Language servers are found on your `PATH` or toolchain first. If none is found, codemycel downloads one into `~/.codemycel/servers/`:

- TypeScript 7 from its GitHub release;
- rust-analyzer through `rustup` or its GitHub release.

Indexing TypeScript and JavaScript needs nothing pre-installed. Indexing Rust needs a Rust toolchain, the same one the repository already builds with.

## Languages

Rust, TypeScript and JavaScript. Markdown files and package manifests (`Cargo.toml`, `package.json`) are indexed too, so documents that mention a symbol, and packages that depend on each other, are linked.

## Known limitations

- **TypeScript 7 semantics.** Repositories relying on options TypeScript 7 removed or changed (older `moduleResolution` modes, `baseUrl`-dependent path mapping) may see imports resolve as `no_definition`. The root's configuration diagnostics appear in `status` and `doctor`. Upstream still marks the TypeScript 7 language service as in progress.
- **Rust needs a toolchain** and, by default, runs build scripts and proc macros (see Security).
- **Communities are heuristic** and labelled so.
- **Renames lose notes.** Renames and file moves create new nodes, so notes on the old path are orphaned until you re-attach them.
- **No performance targets yet.** Large repositories may take a while to index the first time.

## Privacy

No telemetry of any kind: no usage data, no crash reports, no identifiers. codemycel makes exactly three kinds of outbound request:

1. language-server downloads from GitHub Releases, and `rustup component add rust-analyzer` when rustup is present (`--no-download` or `no_download = true` turns these off);
2. `codemycel upgrade` downloading the latest release;
3. a daily version check, a plain GET for the latest release, made by CLI commands but never by `mcp` or `ui` (`CODEMYCEL_NO_UPDATE_CHECK=1` turns it off).

A test in this repository fails if any other module gains a network client.

## Security

rust-analyzer with build scripts and proc macros enabled runs code from the repository and its dependencies while indexing. That is the same exposure as opening the repository in a Rust IDE. To turn it off for an untrusted repository, put this in `.codemycel/config.toml`:

```toml
[servers.rust]
build_scripts = false
```

Macro-generated items then stay unresolved.

The web view binds `127.0.0.1` only and is read-only. `note` is the only tool that writes, and it writes only under `.codemycel/notes/`. codemycel itself never writes anywhere else in your repository and never edits `.gitignore`. Language servers keep their usual caches: rust-analyzer may write `target/` and update `Cargo.lock`.

## What lives in `.codemycel/`

- `index.db` is a cache. Delete it and `codemycel init` rebuilds it. Most teams ignore it in git.
- `notes/` is knowledge: plain Markdown files your agents wrote. Commit it if you want the notes shared.
- `config.toml` holds optional settings (ignores, roots, server paths).

## Evaluation

`eval/` holds a reproducible before/after evaluation: ten caller, impact and orientation tasks on [honojs/hono](https://github.com/honojs/hono) and [irixsoft/irixmail](https://github.com/irixsoft/irixmail), each run with and without codemycel through Claude Code. `eval/README.md` explains how to run it. Results will be published here with the first release.

## License

MIT
