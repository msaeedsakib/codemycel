<p align="center">
  <img src=".github/banner.svg" alt="codemycel: an LSP-verified code graph for coding agents" width="100%">
</p>

<p align="center">
  <a href="https://github.com/msaeedsakib/codemycel/releases/latest"><img alt="Release" src="https://img.shields.io/github/v/release/msaeedsakib/codemycel?style=flat-square&color=2dd4bf&label=release"></a>
  <a href="https://github.com/msaeedsakib/codemycel/actions/workflows/ci.yml"><img alt="CI" src="https://img.shields.io/github/actions/workflow/status/msaeedsakib/codemycel/ci.yml?branch=main&style=flat-square&label=ci"></a>
  <img alt="Status: experimental" src="https://img.shields.io/badge/status-experimental-f59e0b?style=flat-square">
  <img alt="Written in Rust" src="https://img.shields.io/badge/written%20in-rust-b7410e?style=flat-square&logo=rust&logoColor=white">
  <img alt="MCP server" src="https://img.shields.io/badge/MCP-server-84cc16?style=flat-square">
  <a href="LICENSE"><img alt="License: MIT" src="https://img.shields.io/badge/license-MIT-a3e635?style=flat-square"></a>
</p>

<p align="center">
  <a href="#-install">Install</a> ·
  <a href="#-use">Use</a> ·
  <a href="#-what-verified-means">Verified</a> ·
  <a href="#-what-the-benchmark-showed">Benchmark</a> ·
  <a href="eval/RESULTS.md">Full results</a>
</p>

codemycel builds a graph of your repository: which function calls which, what implements a trait or interface, what breaks if a signature changes. It hands that graph to your coding agent over MCP, to your shell as CLI commands, and to you as a local web view. Tree-sitter finds the structure. Every cross-file edge is then verified by a language server (rust-analyzer, TypeScript 7) or by a SCIP index. When an edge can't be verified, it is counted as unresolved and never guessed. The graph lives in `.codemycel/` inside the repository, so it outlasts any one agent session.

> [!IMPORTANT]
> **This is an experiment.** I built codemycel to find out whether a verified code graph makes coding agents better and cheaper. Then I benchmarked it against three similar tools and against plain Claude Code. What I found is in [What the benchmark showed](#-what-the-benchmark-showed).
>
> In short: a graph like this gives an agent more complete knowledge of a codebase, meaning callers and implementations it would otherwise miss, and that matters more the bigger the codebase. It does not make agents meaningfully cheaper, and it cannot beat plain grep on ordinary lookups.
>
> v0.1.0 is published as it stands. I don't plan to develop it further.

|  |  |
|---|---|
| 🍄 **A graph that outlives the session** | Indexed once into `.codemycel/`, kept current by a file watcher while an agent works. |
| ✅ **Verified, never guessed** | Every cross-file edge is confirmed by rust-analyzer, TypeScript 7 or SCIP; what can't be confirmed is listed as unresolved, with the reason. |
| 🔀 **Sees through dispatch** | Callers through a trait object or an interface, and the implementations behind them, are marked `via` the member they go through. |
| 🧭 **One graph, three surfaces** | MCP tools for agents, the same commands in the shell, and a read-only web view on `127.0.0.1`. |
| 🔒 **Local and quiet** | No telemetry. The only network use is language-server downloads, `upgrade`, and a daily version check you can turn off. |

## 📦 Install

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

## 🚀 Use

```sh
codemycel setup          # pick your agents; writes their MCP entry and a short guidance block
cd your-repo
codemycel init           # index the repository
codemycel ui             # open the web view on 127.0.0.1
```

`setup` supports Claude Code, Codex, Cursor, Gemini CLI and OpenCode. For each one you pick, it adds a user-scope MCP server entry (`codemycel mcp`) and a short guidance block in the agent's global instructions file. `codemycel setup --remove` undoes both.

While an agent session runs, `codemycel mcp` watches the repository and keeps the graph current. From the shell, `codemycel sync` does the same.

### 🧰 Tools

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

## ✅ What "verified" means

- **Verified edges.** An edge such as `calls`, `references` or `implements` exists only when rust-analyzer, TypeScript 7 or a SCIP index confirmed it.
- **Unresolved references.** A reference no server could resolve is stored as unresolved, with a reason (`no_definition`, `external_dependency`, `server_unavailable`, …). It is never turned into a guessed edge.
- **Counts are lower bounds.** "12 callers" means at least 12 verified callers.
- **Degraded roots are reported.** A language root whose server couldn't start, or whose pinned Rust toolchain isn't installed, shows as degraded in every answer's coverage line and in `doctor`.
- **Communities are heuristics.** Communities are clusters computed from the verified graph. They are labelled `derived` wherever they appear.

Language servers are found on your `PATH` or toolchain first. If none is found, codemycel downloads one into `~/.codemycel/servers/`:

- TypeScript 7 from its GitHub release;
- rust-analyzer through `rustup` or its GitHub release.

Indexing TypeScript and JavaScript needs nothing pre-installed. Indexing Rust needs a Rust toolchain, the same one the repository already builds with.

## 🌐 Languages

Rust, TypeScript and JavaScript. Markdown files and package manifests (`Cargo.toml`, `package.json`) are indexed too, so documents that mention a symbol, and packages that depend on each other, are linked.

## ⚠️ Known limitations

- **TypeScript 7 semantics.** Repositories relying on options TypeScript 7 removed or changed (older `moduleResolution` modes, `baseUrl`-dependent path mapping) may see imports resolve as `no_definition`. The root's configuration diagnostics appear in `status` and `doctor`. Upstream still marks the TypeScript 7 language service as in progress.
- **Rust needs a toolchain** and, by default, runs build scripts and proc macros (see Security). codemycel asks rustup not to install a root's pinned toolchain while checking for it (`RUSTUP_AUTO_INSTALL=0`), which rustup honours from 1.28.1; rustup older than 1.28 installs it.
- **Communities are heuristic** and labelled so.
- **Renames lose notes.** Renames and file moves create new nodes, so notes on the old path are orphaned until you re-attach them.
- **No performance targets yet.** Large repositories may take a while to index the first time.

## 🔒 Privacy

No telemetry of any kind: no usage data, no crash reports, no identifiers. codemycel makes exactly three kinds of outbound request:

1. language-server downloads from GitHub Releases, and `rustup component add rust-analyzer` when rustup is present (`--no-download` or `no_download = true` turns these off);
2. `codemycel upgrade` downloading the latest release;
3. a daily version check, a plain GET for the latest release, made by CLI commands but never by `mcp` or `ui` (`CODEMYCEL_NO_UPDATE_CHECK=1` turns it off).

A test in this repository fails if any other module gains a network client.

## 🛡️ Security

rust-analyzer with build scripts and proc macros enabled runs code from the repository and its dependencies while indexing. That is the same exposure as opening the repository in a Rust IDE. To turn it off for an untrusted repository, put this in `.codemycel/config.toml`:

```toml
[servers.rust]
build_scripts = false
```

Macro-generated items then stay unresolved.

The web view binds `127.0.0.1` only and is read-only. `note` is the only tool that writes, and it writes only under `.codemycel/notes/`. codemycel itself never writes anywhere else in your repository and never edits `.gitignore`. Language servers keep their usual caches: rust-analyzer may write `target/` and update `Cargo.lock`.

## 📁 What lives in `.codemycel/`

- `index.db` is a cache. Delete it and `codemycel init` rebuilds it. Most teams ignore it in git.
- `notes/` is knowledge: plain Markdown files your agents wrote. Commit it if you want the notes shared.
- `config.toml` holds optional settings (ignores, roots, server paths).

## 📊 What the benchmark showed

`eval/` holds the benchmark. It asks Claude Code (Sonnet, `--bare`, three cold sessions per task) 18 questions on four public repositories: [honojs/hono](https://github.com/honojs/hono), [irixsoft/irixmail](https://github.com/irixsoft/irixmail), [rust-lang/rust-analyzer](https://github.com/rust-lang/rust-analyzer) and [nestjs/nest](https://github.com/nestjs/nest). There are 9 orientation questions ("how does a request reach X"), 7 impact questions ("who calls X, what implements it, what breaks"), and 2 edits ("add a parameter and update every caller"). Each question runs with no tool, with codemycel, and with [GitNexus](https://github.com/abhigyanpatwari/GitNexus), [Serena](https://github.com/oraios/serena) and [CodeGraph](https://github.com/colbymchenry/codegraph), each set up the way its own docs say. Answers are graded blind by Claude Opus against a checklist per question; the impact checklists were checked against language-server references. Edits are graded by the compiler alone. The full table is in [`eval/RESULTS.md`](eval/RESULTS.md).

| Mode | Total tokens (median per task, summed) | Uncached tokens | Cost | Correctness | Completeness |
|---|---:|---:|---:|---:|---:|
| Plain Claude Code | 446k | 151k | $0.57 | 93 | 93 |
| codemycel | 596k | 130k | $0.50 | 92 | 94 |
| GitNexus | 1,216k | 180k | $0.70 | 89 | 89 |
| Serena | 503k | 152k | $0.59 | 93 | 93 |
| CodeGraph | 579k | 175k | $0.61 | 92 | 95 |

What I take from it:

- **None of the four tools beats plain grep on tokens.** On repositories of 500 to 2,000 files, with questions that name the symbol, Claude finds the answer in a few greps. A graph tool adds its tool definitions and results to every turn. codemycel uses the fewest uncached tokens and is about 11% cheaper in dollars than plain Claude Code, but its total token count is higher, and so is every other tool's. The savings are real but small.
- **The one place a graph clearly helps is completeness when dispatch hides a caller or an implementation.** On rust-analyzer, asked for every implementation and trait-object call of `ProcMacroExpander::expand`, codemycel scored 98/100. Every other mode, plain Claude Code included, reached at most 79 completeness. On a multi-crate edit in irixmail ("give `enqueue` a new parameter and update every caller"), codemycel, Serena and CodeGraph scored 100. Plain Claude Code scored 67, because some runs missed call sites and broke the build. This is the knowledge a graph adds, and it matters more as a codebase grows.
- **On orientation questions, plain grep is as good or better.** No tool improved them.
- **Harnesses are closing the gap themselves.** Claude Code and other agents already ship language-server integrations, which give the model the same verified callers and implementations without a separate index. My guess is that they cover most of what this kind of tool offers. The benchmark ran with them off (`--bare`), so this is my view, not a measurement.

`eval/README.md` explains how to reproduce the benchmark.

## 📜 License

MIT
