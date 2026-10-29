# Evaluation

The same ten questions, asked of Claude Code with codemycel, with three other code-intelligence MCP
servers, and with none, on two public repositories. Every answer is graded blind for correctness
and completeness. The numbers come from the raw logs and grades in `logs/`, rendered by `report`;
nobody types them in.

| Repository | Commit | Languages |
|---|---|---|
| [honojs/hono](https://github.com/honojs/hono) | `ed1d5f72135ccf7e09c17d64dc4af7588dd3e491` | TypeScript |
| [irixsoft/irixmail](https://github.com/irixsoft/irixmail) | `cff06ca840b0d1a65e65ddc67d5bfbe7aa181577` | Rust, TypeScript frontend |

## Tasks

`tasks/<repo>.toml` holds five tasks per repository. They mix two kinds of question:

- **Caller and impact:** "who calls X and what breaks if its signature changes".
- **Orientation:** "where is X handled and how does a request reach it".

Two of the tasks cross a trait object or an interface. Each task records the shape of a correct
answer (`expected`) and the checklist taken from it (`items`) that completeness is graded against.

## How a run works

`run.sh` gives each mode its own clone of each repository at the pinned commit and indexes it the
way that tool's documentation says. Then it runs every task three times in each mode through
`claude -p --output-format stream-json`, with `--strict-mcp-config` and only that mode's server:

| Mode | MCP server | Instructions passed with `--append-system-prompt-file` |
|---|---|---|
| `without` | none | none |
| `codemycel` | `codemycel mcp` | the block `codemycel setup` writes into Claude Code's global instructions |
| `gitnexus` | `gitnexus mcp` | the `CLAUDE.md` section `gitnexus analyze` writes into the repository |
| `serena` | `serena start-mcp-server --context=claude-code --project-from-cwd` | none; Serena sends its own through MCP |
| `codegraph` | `codegraph serve --mcp` | the block `codegraph install` writes into Claude Code's global instructions |

The instructions come from running each tool's real setup in a scratch home. Clones live outside
this repository, under `EVAL_WORK` (default `~/.cache/codemycel-eval`), so each one resolves as its
own repository.

`--permission-mode dontAsk` pre-approves only reading and the mode's MCP tools, so no mode can edit
files or spawn subagents. When `ANTHROPIC_API_KEY` is set, runs use `--bare`, which keeps
user-level settings, hooks and instructions out of every mode. Hooks the competitors' setups
install (CodeGraph's prompt hook, GitNexus's tool hooks) therefore do not run. Without the key, the
runner's own Claude Code configuration applies to every mode equally.

## Grading

`grade.sh` grades every answered, ungraded run. For each task, one `claude -p` session (Opus by
default, `GRADE_MODEL` to change it) in the plain clone sees the question, the expected shape, the
checklist and every ungraded answer under a neutral number, in an order unrelated to the mode. It
checks the answers' claims against the code and returns, per answer:

- **correctness**, 0–100, with each false claim listed;
- the **checklist items** the answer names correctly, from which `report` computes completeness
  and lists the missing items.

The verdicts land beside each run as `<run>.grade.json`, and the grader's own log as
`grader-<time>.jsonl` in the task's directory.

## Report

For each task and mode, `report` prints the median tokens (input, output, cache-read and
cache-creation tokens over every model the run used) and tool calls, and the mean correctness and
completeness. Then it prints totals per mode, and for each competitor the tasks where codemycel is
at least as correct and as complete with at most half the tokens.

## Reproduce

```sh
cargo build --release -p codemycel
CODEMYCEL=target/release/codemycel EVAL_MODEL=sonnet eval/run.sh
eval/grade.sh
```

`EVAL_MODES` and `EVAL_TASKS` limit a run to some modes or task ids. The competitors are found on
`PATH` or through `GITNEXUS`, `SERENA` and `CODEGRAPH`. Indexing irixmail needs a Rust toolchain;
rustup installs the version the repository pins. Runs whose log already holds a result are skipped,
so an interrupted eval resumes where it stopped.
