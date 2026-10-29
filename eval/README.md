# Before/after evaluation

The same ten questions, asked of Claude Code with and without codemycel, on two public
repositories. The numbers in the top-level README come from the raw logs in `logs/`, rendered by
`report`; nobody types them in.

| Repository | Commit | Languages |
|---|---|---|
| [honojs/hono](https://github.com/honojs/hono) | `ed1d5f72135ccf7e09c17d64dc4af7588dd3e491` | TypeScript |
| [irixsoft/irixmail](https://github.com/irixsoft/irixmail) | `cff06ca840b0d1a65e65ddc67d5bfbe7aa181577` | Rust, TypeScript frontend |

## Tasks

`tasks/<repo>.toml` holds five tasks per repository. They mix two kinds of question:

- **Caller and impact:** "who calls X and what breaks if its signature changes".
- **Orientation:** "where is X handled and how does a request reach it".

Two of the tasks cross a trait object or an interface. Each task also records the shape of a correct answer, so a reader can judge the answers in the logs.

## How a run works

`run.sh` clones each repository twice at its pinned commit and indexes one clone with `codemycel init`. Then it runs every task three times in each mode through `claude -p --output-format stream-json`:

- **without:** the plain clone, `--strict-mcp-config` with no MCP servers, tools `Read`, `Grep`, `Glob`.
- **with:** the indexed clone, `--strict-mcp-config` with only `codemycel mcp`, tools `Read`, `Grep`, `Glob` and the codemycel tools. The guidance block that `codemycel setup` writes into Claude Code's global instructions is passed with `--append-system-prompt-file`. It is produced by running the real `setup` in a scratch home.

Clones live outside this repository, under `EVAL_WORK` (default `~/.cache/codemycel-eval`), so each one resolves as its own repository.

No other tool is allowed in either mode (`--permission-mode dontAsk`), so neither can edit files or spawn subagents. When `ANTHROPIC_API_KEY` is set, runs use `--bare`, which keeps user-level settings and instructions out of both modes. Without it, the runner's own Claude Code configuration applies to both modes equally.

For each run, `report` counts:

- **tokens:** input, output, cache-read and cache-creation tokens over every model the run used, from the final `result` event;
- **tool calls:** the `tool_use` blocks.

It prints the median per task and mode.

## Reproduce

```sh
cargo build --release -p codemycel
CODEMYCEL=target/release/codemycel EVAL_MODEL=sonnet eval/run.sh
cargo run -p codemycel-eval --bin report
```

Indexing irixmail needs a Rust toolchain; rustup installs the version the repository pins. Runs whose log already holds a result are skipped, so an interrupted eval resumes where it stopped.
