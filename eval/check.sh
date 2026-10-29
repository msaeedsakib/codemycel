#!/usr/bin/env bash
# Grades the edit tasks. Each edit run's patch (<run>.patch, saved by run.sh) is applied to a clean
# check clone of the repository, the task's checker runs, and the errors it reports beyond those of
# the untouched tree decide the grade: the definition must read as the task asks, a call site counts
# as updated when the patch changes it and no new error lands on it, and new errors, text that
# silences the checker, and deleted calls are wrong claims. The grade lands beside the run as
# <run>.grade.json. No model is involved. Before a task is graded, its planted patch
# (tasks/planted/<task>.patch: the definition changed, no caller) must break every listed site.
#
# Usage: eval/check.sh [repo...]
# Checkers: `typescript` type-checks the production sources (the repository's tsconfig.build.json)
# with the TypeScript 7 compiler
# codemycel provisions (TSC, default ~/.codemycel/servers/typescript/*/tsc), skipping type packages
# so no node_modules are needed; `cargo` runs `cargo check --workspace`, which leaves tests out.
set -euo pipefail

eval_dir=$(cd "$(dirname "$0")" && pwd)
root=$(dirname "$eval_dir")
work=${EVAL_WORK:-${XDG_CACHE_HOME:-$HOME/.cache}/codemycel-eval}
tsc=${TSC:-$(find "${CODEMYCEL_HOME:-$HOME/.codemycel}/servers/typescript" -name tsc -type f 2>/dev/null | sort | tail -n 1)}

repos=("$@")
if [ ${#repos[@]} -eq 0 ]; then
    repos=(hono irixmail rust-analyzer nest)
fi

cargo build --quiet --release --manifest-path "$root/Cargo.toml" -p codemycel-eval
target_dir=$(cargo metadata --format-version 1 --no-deps --manifest-path "$root/Cargo.toml" |
    grep -o '"target_directory":"[^"]*"' | cut -d '"' -f 4)
bin="$target_dir/release"

# The checker's errors for the tree in $1, one per line.
errors() {
    local dir=$1 checker=$2
    case $checker in
    typescript)
        printf '%s' '{"extends":"./tsconfig.build.json","compilerOptions":{"types":[],"skipLibCheck":true,"noEmit":true,"noUnusedLocals":false,"noUnusedParameters":false}}' \
            >"$dir/.eval-check.json"
        (cd "$dir" && "$tsc" -p .eval-check.json 2>&1 || true) | grep ': error TS' || true
        rm -f "$dir/.eval-check.json"
        ;;
    cargo)
        (cd "$dir" && CARGO_TARGET_DIR=$work/check-target/$(basename "$dir") \
            cargo check --quiet --workspace --message-format short 2>&1 || true) |
            grep ': error' || true
        ;;
    *) echo "unknown checker $checker" >&2 && return 1 ;;
    esac
}

for repo in "${repos[@]}"; do
    tasks=$eval_dir/tasks/$repo.toml
    check=$work/$repo-check
    while IFS=$'\t' read -r task kind checker _ _; do
        [ "$kind" = edit ] || continue
        task_logs=$eval_dir/logs/$repo/$task
        [ -d "$task_logs" ] || continue
        if [ ! -d "$check/.git" ]; then
            git clone --quiet "$work/$repo" "$check"
        fi
        git -C "$check" checkout --quiet --detach "$(git -C "$work/$repo" rev-parse HEAD)"
        baseline=$task_logs/baseline-errors.txt
        if [ ! -f "$baseline" ]; then
            errors "$check" "$checker" >"$baseline"
        fi

        # Self-test: the planted patch changes the definition and no caller, so the checker must
        # break every listed site and the grade must count none of them as updated.
        planted=$eval_dir/tasks/planted/$task.patch
        scratch=$(mktemp -d)
        cp "$planted" "$scratch/0.patch"
        echo '{"type":"result","is_error":false,"result":"","modelUsage":{}}' >"$scratch/0.jsonl"
        git -C "$check" apply --whitespace=nowarn "$planted"
        errors "$check" "$checker" >"$scratch/0.errors.txt"
        "$bin/grade" edit "$tasks" "$task_logs" "$scratch/0.jsonl" "$baseline" "$scratch/0.errors.txt" "$check" >/dev/null
        if ! grep -q '"completeness": 0.0' "$scratch/0.grade.json" ||
            grep -q 'not changed as asked' "$scratch/0.grade.json"; then
            echo "$repo $task: the planted break was not caught; not grading this task" >&2
            cat "$scratch/0.grade.json" >&2
            rm -rf "$scratch"
            continue
        fi
        rm -rf "$scratch"

        for patch in "$task_logs"/*/*.patch; do
            [ -f "$patch" ] || continue
            log=${patch%.patch}.jsonl
            if [ -f "${patch%.patch}.grade.json" ] || ! grep -q '"type":"result"' "$log" 2>/dev/null; then
                continue
            fi
            git -C "$check" checkout --quiet -- .
            git -C "$check" clean --quiet -fd
            if [ -s "$patch" ]; then
                git -C "$check" apply --whitespace=nowarn "$patch" || echo "  the patch does not apply" >&2
            fi
            errors "$check" "$checker" >"${patch%.patch}.errors.txt"
            echo -n "$repo $task $(basename "$(dirname "$patch")") $(basename "$patch" .patch): "
            "$bin/grade" edit "$tasks" "$task_logs" "$log" "$baseline" "${patch%.patch}.errors.txt" "$check"
        done
        git -C "$check" checkout --quiet -- .
        git -C "$check" clean --quiet -fd
    done < <("$bin/tasks" "$tasks")
done

"$bin/report" "$eval_dir/logs"
