#!/usr/bin/env bash
# Grades every answered, ungraded run in eval/logs blind. For each task, one `claude -p` session in
# the plain clone sees the question, the shape of a correct answer, the task's checklist and every
# ungraded answer under a neutral number, checks the claims against the code with Read, Grep and
# Glob, and returns correctness and the checklist items named per answer. The verdicts land beside
# each run as <run>.grade.json and the grader's own log as grader-<time>.jsonl in the task's
# directory. Run it after eval/run.sh; the report then shows the grades.
#
# Usage: eval/grade.sh [repo...] (default: hono irixmail rust-analyzer nest)
# GRADE_MODEL picks the grader's model (default opus); with ANTHROPIC_API_KEY set, it runs --bare.
set -euo pipefail

eval_dir=$(cd "$(dirname "$0")" && pwd)
root=$(dirname "$eval_dir")
work=${EVAL_WORK:-${XDG_CACHE_HOME:-$HOME/.cache}/codemycel-eval}

repos=("$@")
if [ ${#repos[@]} -eq 0 ]; then
    repos=(hono irixmail rust-analyzer nest)
fi

cargo build --quiet --release --manifest-path "$root/Cargo.toml" -p codemycel-eval
target_dir=$(cargo metadata --format-version 1 --no-deps --manifest-path "$root/Cargo.toml" |
    grep -o '"target_directory":"[^"]*"' | cut -d '"' -f 4)
bin="$target_dir/release"

args=(--output-format stream-json --verbose --no-session-persistence --strict-mcp-config
    --mcp-config '{"mcpServers":{}}' --permission-mode dontAsk --allowedTools Read Grep Glob
    --model "${GRADE_MODEL:-opus}" --json-schema "$("$bin/grade" schema)")
if [ -n "${ANTHROPIC_API_KEY:-}" ]; then
    args+=(--bare)
fi

for repo in "${repos[@]}"; do
    tasks=$eval_dir/tasks/$repo.toml
    for task_logs in "$eval_dir/logs/$repo"/*/; do
        task_logs=${task_logs%/}
        prompt=$("$bin/grade" prompt "$tasks" "$task_logs")
        if [ -z "$prompt" ]; then
            continue
        fi
        grader_log=$task_logs/grader-$(date +%Y%m%dT%H%M%S).jsonl
        echo "grading $repo $(basename "$task_logs")"
        (cd "$work/$repo" && claude -p "$prompt" "${args[@]}" </dev/null >"$grader_log") ||
            echo "  claude exited $?"
        "$bin/grade" store "$tasks" "$task_logs" "$grader_log" ||
            echo "  no grades stored; the next run of grade.sh retries this task"
    done
done

"$bin/report" "$eval_dir/logs"
