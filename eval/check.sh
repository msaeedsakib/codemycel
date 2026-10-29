#!/usr/bin/env bash
# Grades the edit tasks. Each edit run's patch (<run>.patch, saved by run.sh) is applied to a clean
# check clone of the repository, the task's checker runs, and the errors it reports beyond those of
# the untouched tree decide the grade: the definition must read as the task asks, a call site counts
# as updated when the patch changes it and no new error lands on it, and new errors, text that
# silences the checker, and deleted calls are wrong claims. The grade lands beside the run as
# <run>.grade.json. No model is involved.
#
# Before a task is graded, a self-test proves the checker catches each listed site. Every patch in
# tasks/planted/<task>/ changes the definition as the task asks and leaves some callers unfixed;
# its definition must hold the task's signature, and every site must be caught by at least one
# planted patch: the patch leaves the site unchanged and the checker reports a new error on it.
# A task needs several planted patches when one break hides another, as when `cargo check` stops
# at the first crate that fails and never reaches a caller in a downstream crate. A task whose
# self-test fails is not graded.
#
# Usage: eval/check.sh [repo...]
# Checkers: `typescript` type-checks the production sources (the repository's tsconfig.build.json)
# with the TypeScript 7 compiler codemycel provisions (TSC, default
# ~/.codemycel/servers/typescript/*/tsc), skipping type packages so no node_modules are needed;
# `cargo` runs `cargo check --workspace`, which leaves tests out. A checker that cannot be found
# stops the script.
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

require_checker() {
    case $1 in
    typescript)
        if [ -z "$tsc" ] || [ ! -x "$tsc" ]; then
            echo "no TypeScript compiler found; run codemycel once on a TypeScript repository or set TSC" >&2
            exit 1
        fi
        ;;
    cargo) command -v cargo >/dev/null || { echo "cargo is not on PATH" >&2 && exit 1; } ;;
    *) echo "unknown checker $1" >&2 && exit 1 ;;
    esac
}

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
    esac
}

reset_check() {
    git -C "$check" checkout --quiet -- .
    git -C "$check" clean --quiet -fd
}

# Runs the task's planted patches and prints the sites none of them caught. Fails when a planted
# patch does not apply or its definition does not hold the task's signature.
self_test() {
    local scratch planted uncaught= status=0
    scratch=$(mktemp -d)
    for planted in "$eval_dir/tasks/planted/$task"/*.patch; do
        if [ ! -f "$planted" ]; then
            echo "no planted patch in tasks/planted/$task" >&2
            status=1 && break
        fi
        reset_check
        git -C "$check" apply --whitespace=nowarn "$planted" || { status=1 && break; }
        errors "$check" "$checker" >"$scratch/errors.txt"
        uncaught=$("$bin/grade" planted "$tasks" "$task_logs" "$planted" "$baseline" \
            "$scratch/errors.txt" "$check" "$scratch/caught.txt") || { status=1 && break; }
    done
    rm -rf "$scratch"
    printf '%s' "$uncaught"
    return $status
}

for repo in "${repos[@]}"; do
    tasks=$eval_dir/tasks/$repo.toml
    check=$work/$repo-check
    while IFS=$'\t' read -r task kind checker _ _; do
        [ "$kind" = edit ] || continue
        task_logs=$eval_dir/logs/$repo/$task
        [ -d "$task_logs" ] || continue
        require_checker "$checker"
        if [ ! -d "$check/.git" ]; then
            git clone --quiet "$work/$repo" "$check"
        fi
        reset_check
        git -C "$check" checkout --quiet --detach "$(git -C "$work/$repo" rev-parse HEAD)"
        baseline=$task_logs/baseline-errors.txt
        if [ ! -f "$baseline" ]; then
            errors "$check" "$checker" >"$baseline"
        fi

        if ! uncaught=$(self_test); then
            reset_check
            echo "$repo $task: a planted patch failed; not grading this task" >&2
            continue
        fi
        reset_check
        if [ -n "$uncaught" ]; then
            echo "$repo $task: no planted patch broke these sites; not grading this task:" >&2
            echo "$uncaught" >&2
            continue
        fi

        for patch in "$task_logs"/*/*.patch; do
            [ -f "$patch" ] || continue
            log=${patch%.patch}.jsonl
            if [ -f "${patch%.patch}.grade.json" ] || ! grep -q '"type":"result"' "$log" 2>/dev/null; then
                continue
            fi
            reset_check
            if [ -s "$patch" ]; then
                git -C "$check" apply --whitespace=nowarn "$patch" || echo "  the patch does not apply" >&2
            fi
            errors "$check" "$checker" >"${patch%.patch}.errors.txt"
            echo -n "$repo $task $(basename "$(dirname "$patch")") $(basename "$patch" .patch): "
            "$bin/grade" edit "$tasks" "$task_logs" "$log" "$baseline" "${patch%.patch}.errors.txt" "$check"
        done
        reset_check
    done < <("$bin/tasks" "$tasks")
done

"$bin/report" "$eval_dir/logs"
