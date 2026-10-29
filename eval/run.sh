#!/usr/bin/env bash
# Runs the before/after evaluation. Each task in eval/tasks/<repo>.toml runs EVAL_RUNS times
# (default 3) through `claude -p`: "without" on a clean clone with no MCP server, and "with" on
# an indexed clone with only the codemycel MCP server and the guidance block `codemycel setup`
# writes for Claude Code. Clones live outside this repository, under EVAL_WORK (default
# ~/.cache/codemycel-eval), so they resolve as their own repositories. Raw logs land in
# eval/logs/<repo>/<task>/<mode>/<run>.jsonl, and the table is printed from them at the end.
# Runs whose log already holds a result are skipped, so an interrupted eval resumes.
#
# Usage: eval/run.sh [hono] [irixmail]
# Needs: git, cargo, a logged-in `claude`, and `codemycel` on PATH (or CODEMYCEL=<path>).
# EVAL_MODEL picks the model; with ANTHROPIC_API_KEY set, runs use --bare so no user-level
# settings or instructions reach either mode.
set -euo pipefail

eval_dir=$(cd "$(dirname "$0")" && pwd)
root=$(dirname "$eval_dir")
work=${EVAL_WORK:-${XDG_CACHE_HOME:-$HOME/.cache}/codemycel-eval}
runs=${EVAL_RUNS:-3}
codemycel=$(command -v "${CODEMYCEL:-codemycel}")

declare -A url=(
    [hono]=https://github.com/honojs/hono
    [irixmail]=https://github.com/irixsoft/irixmail
)
declare -A commit=(
    [hono]=ed1d5f72135ccf7e09c17d64dc4af7588dd3e491
    [irixmail]=cff06ca840b0d1a65e65ddc67d5bfbe7aa181577
)

repos=("$@")
if [ ${#repos[@]} -eq 0 ]; then
    repos=(hono irixmail)
fi

cargo build --quiet --release --manifest-path "$root/Cargo.toml" -p codemycel-eval
target_dir=$(cargo metadata --format-version 1 --no-deps --manifest-path "$root/Cargo.toml" |
    grep -o '"target_directory":"[^"]*"' | cut -d '"' -f 4)
bin="$target_dir/release"

clone() {
    local repo=$1 dir=$2
    if [ ! -d "$dir/.git" ]; then
        git clone --quiet --filter=blob:none "${url[$repo]}" "$dir"
    fi
    git -C "$dir" checkout --quiet --detach "${commit[$repo]}"
}

common=(--output-format stream-json --verbose --no-session-persistence --strict-mcp-config
    --permission-mode dontAsk)
if [ -n "${EVAL_MODEL:-}" ]; then
    common+=(--model "$EVAL_MODEL")
fi
if [ -n "${ANTHROPIC_API_KEY:-}" ]; then
    common+=(--bare)
fi
no_servers='{"mcpServers":{}}'
codemycel_server=$(printf '{"mcpServers":{"codemycel":{"command":"%s","args":["mcp"]}}}' "$codemycel")

setup_home=$(mktemp -d)
trap 'rm -rf "$setup_home"' EXIT
mkdir -p "$setup_home/.claude"
HOME=$setup_home CODEMYCEL_HOME=$setup_home/.codemycel CODEMYCEL_NO_UPDATE_CHECK=1 \
    "$codemycel" setup --harness claude-code >/dev/null
guidance="$setup_home/.claude/CLAUDE.md"

for repo in "${repos[@]}"; do
    plain="$work/$repo"
    indexed="$work/$repo-indexed"
    clone "$repo" "$plain"
    clone "$repo" "$indexed"
    if [ ! -f "$indexed/.codemycel/index.db" ]; then
        CODEMYCEL_NO_UPDATE_CHECK=1 "$codemycel" init --repo "$indexed" || [ $? -eq 2 ]
    fi

    while IFS=$'\t' read -r task prompt; do
        for mode in without with; do
            if [ "$mode" = with ]; then
                dir=$indexed
                mode_args=(--mcp-config "$codemycel_server" --append-system-prompt-file "$guidance"
                    --allowedTools Read Grep Glob mcp__codemycel)
            else
                dir=$plain
                mode_args=(--mcp-config "$no_servers" --allowedTools Read Grep Glob)
            fi
            logs="$eval_dir/logs/$repo/$task/$mode"
            mkdir -p "$logs"
            for run in $(seq 1 "$runs"); do
                log="$logs/$run.jsonl"
                if [ -f "$log" ] && grep -q '"type":"result"' "$log"; then
                    continue
                fi
                echo "$repo $task $mode run $run"
                (cd "$dir" && claude -p "$prompt" "${common[@]}" "${mode_args[@]}" </dev/null >"$log") ||
                    echo "  claude exited $?; the log is kept and the run counts only if it has a result"
            done
        done
    done < <("$bin/tasks" "$eval_dir/tasks/$repo.toml")
done

"$bin/report" "$eval_dir/logs"
