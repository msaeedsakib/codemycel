# Changelog

## Unreleased

First version, not yet published.

- Index Rust, TypeScript and JavaScript repositories:
  - tree-sitter extracts the structure;
  - rust-analyzer, TypeScript 7 or a SCIP index verifies every cross-file edge;
  - references no server can resolve are counted as unresolved, with a reason.
- Markdown and package manifests are indexed repository-wide.
- Incremental `sync`, plus a watcher and warm language servers during MCP sessions.
- Communities, labelled as derived.
- Notes that agents attach to symbols, stored as Markdown under `.codemycel/notes/`.
- Nine tools over MCP and the CLI: `search`, `symbol`, `impact`, `trace`, `overview`, `unresolved`, `status`, `note`, `notes`.
- `codemycel ui`: a read-only web view on 127.0.0.1 with a community map, search, node pages, impact, status and notes.
- `setup` for Claude Code, Codex, Cursor, Gemini CLI and OpenCode; also `doctor`, `upgrade` and a daily version check.
- Installers for Linux, macOS and Windows, and release archives for five targets.
- No telemetry. A test guards the three permitted kinds of network call.
