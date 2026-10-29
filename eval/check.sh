#!/usr/bin/env bash
# Grades the edit tasks. Each edit run's patch (<run>.patch, saved by run.sh) is applied to a clean
# check clone of the repository, the task's checker runs, and the errors it reports beyond those of
# the untouched tree decide the grade: the definition must read as the task asks, and each new error
# is a call site the edit missed or broke. The grade lands beside the run as <run>.grade.json. No
# model is involved.
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
    while IFS=$'\t' read -r task kind checker signature _; do
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
        for patch in "$task_logs"/*/*.patch; do
            [ -f "$patch" ] || continue
            log=${patch%.patch}.jsonl
            if [ -f "${patch%.patch}.grade.json" ] || ! grep -q '"type":"result"' "$log" 2>/dev/null; then
                continue
            fi
            git -C "$check" checkout --quiet -- .
            git -C "$check" clean --quiet -fd
            found=0
            if [ -s "$patch" ] && git -C "$check" apply --whitespace=nowarn "$patch"; then
                if git -C "$check" grep -qF -- "$signature"; then
                    found=1
                fi
            fi
            errors "$check" "$checker" >"${patch%.patch}.errors.txt"
            echo -n "$repo $task $(basename "$(dirname "$patch")") $(basename "$patch" .patch): "
            "$bin/grade" edit "$tasks" "$task_logs" "$log" "$baseline" "${patch%.patch}.errors.txt" "$found"
        done
        git -C "$check" checkout --quiet -- .
        git -C "$check" clean --quiet -fd
    done < <("$bin/tasks" "$tasks")
done

"$bin/report" "$eval_dir/logs"
