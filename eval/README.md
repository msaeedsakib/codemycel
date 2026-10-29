# Evaluation

Eighteen tasks, given to Claude Code with codemycel, with three other code-intelligence MCP servers,
and with none, on four public repositories. Answers to questions are graded blind for correctness
and completeness; edits are graded by the compiler. `report` renders the numbers from the raw logs
and grades in `logs/`; nobody types them in. The results of the run published with v0.1.0 are in
[RESULTS.md](RESULTS.md). The raw logs are not committed.

| Repository | Commit | Languages |
|---|---|---|
| [honojs/hono](https://github.com/honojs/hono) | `ed1d5f72135ccf7e09c17d64dc4af7588dd3e491` | TypeScript |
| [irixsoft/irixmail](https://github.com/irixsoft/irixmail) | `cff06ca840b0d1a65e65ddc67d5bfbe7aa181577` | Rust, TypeScript frontend |
| [rust-lang/rust-analyzer](https://github.com/rust-lang/rust-analyzer) | `c59cdaa4c26ffe036a3403e4948f1681995d4125` | Rust |
| [nestjs/nest](https://github.com/nestjs/nest) | `65b4c159129725e849654cd15cfb7ab5a88cae8d` | TypeScript |

## Tasks

`tasks/<repo>.toml` holds the tasks, of three kinds:

- **Impact:** "who calls X, what implements it, and what breaks if its signature changes".
- **Orientation:** "where is X handled and how does a request reach it", including multi-hop traces.
- **Edit:** "give X a new parameter and update every caller outside tests".

Several tasks cross a trait object or an interface. Each question records the shape of a correct
answer (`expected`) and the checklist completeness is graded against (`items`). The checklists of
the impact tasks were checked against language-server references (rust-analyzer and TypeScript 7
`references` and `implementation`). Each edit task also lists its call sites (`sites`), the
function whose calls must survive (`callee`), the text the new definition must contain
(`signature`), the checker that judges it (`check`), and a planted patch in `tasks/planted/` that
changes the definition and no caller.

## How a run works

`run.sh` gives each mode its own clone of each repository at the pinned commit, and indexes it the
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

When `ANTHROPIC_API_KEY` is set, runs use `--bare`. That keeps user-level settings, hooks,
`CLAUDE.md` discovery and Claude Code's own language-server integration out of every mode. Hooks the
competitors' setups install (CodeGraph's prompt hook, GitNexus's tool hooks) therefore do not run.
In this mode every run gets the same built-in tools: `Bash`, `Read` and `Edit`. `--permission-mode
dontAsk` approves reading and the mode's MCP tools, and also `Edit` and `Write` for edit tasks.
After an edit run, its diff is saved as `<run>.patch` and the clone is reset.

## Grading

**Questions.** `grade.sh` grades every answered, ungraded run. For each task, one `claude -p`
session (Opus by default; `GRADE_MODEL` changes it) runs in the plain clone. It sees the question,
the expected shape, the checklist, and every ungraded answer under a neutral number, in an order
unrelated to the mode. It checks the claims against the code and returns, per answer:

- **correctness**, 0–100, with each false claim listed. A claim counts as false only if the grader
  cites code that contradicts it; failing to find something by text search is not proof.
- the **checklist items** the answer names correctly. `report` turns these into completeness and
  lists the missing items.

**Edits.** `check.sh` applies each patch to a clean check clone and runs the task's checker:
TypeScript 7 `tsc` on the production sources, or `cargo check --workspace`. Errors are compared with
the untouched tree's errors as a multiset of file and message, ignoring positions. Scoring:

- Nothing counts unless the definition contains the asked-for signature, ignoring whitespace.
- A call site is updated when the patch changes it and no new error lands on it.
- Each of these is a false claim worth 25 points:
  - a new error;
  - an added line that silences the checker (`as any`, `@ts-ignore`, `todo!()`, `#[allow(…)]`, …);
  - a hunk that deletes a call to the target.

Before a task is graded, its planted patch must break every listed site. Otherwise the task is not
graded.

The verdicts land beside each run as `<run>.grade.json`, and the grader's own log as
`grader-<time>.jsonl` in the task's directory.

## Report

For each task and mode, `report` prints:

- **Tokens:** the median over the runs of input, output, cache-read and cache-creation tokens, over
  every model the run used.
- **Uncached:** the same, without cache reads.
- **Cost:** the median cost in dollars.
- **Correctness and completeness:** the means over the runs.
- **Tool calls:** the median MCP, Bash, Read and Edit calls, and the characters of tool results the
  model read.

Then it prints:

- totals per mode;
- totals per task kind;
- for each competitor, the tasks where codemycel is at least as correct and as complete with at most
  half the tokens. Next to that, it counts near misses within 3 points and gives an overall verdict
  on the summed totals.

## Reproduce

```sh
cargo build --release -p codemycel
CODEMYCEL=target/release/codemycel EVAL_MODEL=sonnet eval/run.sh
eval/check.sh
eval/grade.sh
```

- `EVAL_MODES` and `EVAL_TASKS` limit a run to some modes or task ids.
- The competitors are found on `PATH` or through `GITNEXUS`, `SERENA` and `CODEGRAPH`.
- Indexing the Rust repositories needs a Rust toolchain. Building irixmail's `zstd-sys` needs
  libclang (`clang` and `libclang-dev` on Ubuntu).
- Runs whose log already holds a result are skipped, so an interrupted eval resumes where it
  stopped.
