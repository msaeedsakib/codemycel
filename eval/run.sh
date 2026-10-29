#!/usr/bin/env bash
# Runs the evaluation. Each task in eval/tasks/<repo>.toml runs EVAL_RUNS times (default 3) through
# `claude -p` in every mode named by EVAL_MODES (default: all of them):
#   without    a clean clone with no MCP server
#   codemycel  an indexed clone with only `codemycel mcp` and the guidance `codemycel setup` writes
#   gitnexus   an indexed clone with only `gitnexus mcp` and the CLAUDE.md section `analyze` writes
#   serena     a clone with only Serena's MCP server in its `claude-code` context
#   codegraph  an indexed clone with only `codegraph serve --mcp` and the CLAUDE.md section
#              `codegraph install` writes
# Each mode has its own clone under EVAL_WORK (default ~/.cache/codemycel-eval), outside this
# repository, so the tools' index directories never meet. Raw logs land in
# eval/logs/<repo>/<task>/<mode>/<run>.jsonl, and the table is printed from them at the end.
# Runs whose log already holds a result are skipped, so an interrupted eval resumes.
#
# Usage: eval/run.sh [repo...] (default: hono irixmail rust-analyzer nest)
# Needs: git, cargo, jq, a logged-in `claude`, and on PATH (or named by the variable in brackets)
# `codemycel` [CODEMYCEL], `gitnexus` [GITNEXUS], `serena` [SERENA], `codegraph` [CODEGRAPH] for the
# modes that run. EVAL_TASKS limits the run to the named task ids. EVAL_MODEL picks the model; with ANTHROPIC_API_KEY set, runs use --bare so no
# user-level settings, hooks or instructions reach any mode.
set -euo pipefail

eval_dir=$(cd "$(dirname "$0")" && pwd)
root=$(dirname "$eval_dir")
work=${EVAL_WORK:-${XDG_CACHE_HOME:-$HOME/.cache}/codemycel-eval}
runs=${EVAL_RUNS:-3}
read -r -a modes <<<"${EVAL_MODES:-without codemycel gitnexus serena codegraph}"

declare -A url=(
    [hono]=https://github.com/honojs/hono
    [irixmail]=https://github.com/irixsoft/irixmail
    [rust-analyzer]=https://github.com/rust-lang/rust-analyzer
    [nest]=https://github.com/nestjs/nest
)
declare -A commit=(
    [hono]=ed1d5f72135ccf7e09c17d64dc4af7588dd3e491
    [irixmail]=cff06ca840b0d1a65e65ddc67d5bfbe7aa181577
    [rust-analyzer]=c59cdaa4c26ffe036a3403e4948f1681995d4125
    [nest]=65b4c159129725e849654cd15cfb7ab5a88cae8d
)
declare -A serena_languages=(
    [hono]="--language typescript"
    [irixmail]="--language rust --language typescript"
    [rust-analyzer]="--language rust"
    [nest]="--language typescript"
)
declare -A bin_name=([codemycel]=CODEMYCEL [gitnexus]=GITNEXUS [serena]=SERENA [codegraph]=CODEGRAPH)
declare -A tool
for mode in "${modes[@]}"; do
    if [ "$mode" != without ]; then
        variable=${bin_name[$mode]:?unknown mode $mode}
        tool[$mode]=$(command -v "${!variable:-$mode}")
    fi
done

repos=("$@")
if [ ${#repos[@]} -eq 0 ]; then
    repos=(hono irixmail rust-analyzer nest)
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

# Puts a clone back at its commit after an edit run, keeping the tools' index files.
reset() {
    git -C "$1" reset --quiet
    git -C "$1" checkout --quiet -- .
    git -C "$1" clean --quiet -fd -e .codemycel -e .gitnexus -e .codegraph -e .serena -e .claude \
        -e AGENTS.md -e CLAUDE.md
}

server() {
    local args
    args=$(printf '%s\n' "${@:3}" | jq -R . | jq -sc .)
    jq -cn --arg name "$1" --arg command "$2" --argjson args "$args" \
        '{mcpServers: {($name): {command: $command, args: $args}}}'
}

# Indexes the mode's clone if it has no index yet.
prepare() {
    local mode=$1 repo=$2 dir=$3
    case $mode in
    codemycel)
        [ -f "$dir/.codemycel/index.db" ] ||
            CODEMYCEL_NO_UPDATE_CHECK=1 "${tool[codemycel]}" init --repo "$dir" || [ $? -eq 2 ]
        ;;
    gitnexus) [ -d "$dir/.gitnexus" ] || (cd "$dir" && "${tool[gitnexus]}" analyze .) ;;
    codegraph) [ -d "$dir/.codegraph" ] || "${tool[codegraph]}" init --yes "$dir" ;;
    serena)
        # shellcheck disable=SC2086
        [ -f "$dir/.serena/project.yml" ] ||
            "${tool[serena]}" project create ${serena_languages[$repo]} --index "$dir"
        ;;
    esac
}

common=(--output-format stream-json --verbose --no-session-persistence --strict-mcp-config
    --permission-mode dontAsk)
if [ -n "${EVAL_MODEL:-}" ]; then
    common+=(--model "$EVAL_MODEL")
fi
if [ -n "${ANTHROPIC_API_KEY:-}" ]; then
    common+=(--bare)
fi

setup_home=$(mktemp -d)
trap 'rm -rf "$setup_home"' EXIT
declare -A guidance
for mode in "${modes[@]}"; do
    home=$setup_home/$mode
    mkdir -p "$home/.claude"
    case $mode in
    codemycel)
        HOME=$home CODEMYCEL_HOME=$home/.codemycel CODEMYCEL_NO_UPDATE_CHECK=1 \
            "${tool[codemycel]}" setup --harness claude-code >/dev/null
        guidance[$mode]=$home/.claude/CLAUDE.md
        ;;
    codegraph)
        CODEGRAPH_TELEMETRY=0 HOME=$home "${tool[codegraph]}" install --target claude --location global --yes >/dev/null </dev/null
        guidance[$mode]=$home/.claude/CLAUDE.md
        ;;
    esac
done

declare -A prepared
for repo in "${repos[@]}"; do
    while IFS=$'\t' read -r task kind _ _ prompt; do
        if [ -n "${EVAL_TASKS:-}" ] && [[ " $EVAL_TASKS " != *" $task "* ]]; then
            continue
        fi
        for mode in "${modes[@]}"; do
            case $mode in
            without) dir=$work/$repo ;;
            codemycel) dir=$work/$repo-indexed ;;
            *) dir=$work/$repo-$mode ;;
            esac
            if [ -z "${prepared[$repo-$mode]:-}" ]; then
                clone "$repo" "$dir"
                prepare "$mode" "$repo" "$dir"
                prepared[$repo-$mode]=1
            fi
            allowed=(Read Grep Glob)
            if [ "$kind" = edit ]; then
                allowed+=(Edit Write)
            fi
            if [ "$mode" != without ]; then
                allowed+=("mcp__$mode")
            fi
            mode_args=(--allowedTools "${allowed[@]}")
            case $mode in
            without) mode_args+=(--mcp-config '{"mcpServers":{}}') ;;
            codemycel) mode_args+=(--mcp-config "$(server codemycel "${tool[codemycel]}" mcp)") ;;
            gitnexus)
                mode_args+=(--mcp-config "$(server gitnexus "${tool[gitnexus]}" mcp)")
                guidance[$mode]=$dir/CLAUDE.md
                ;;
            serena)
                mode_args+=(--mcp-config "$(server serena "${tool[serena]}" start-mcp-server \
                    --context=claude-code --project-from-cwd --open-web-dashboard false)")
                ;;
            codegraph) mode_args+=(--mcp-config "$(server codegraph "${tool[codegraph]}" serve --mcp)") ;;
            esac
            if [ -n "${guidance[$mode]:-}" ]; then
                mode_args+=(--append-system-prompt-file "${guidance[$mode]}")
            fi
            logs="$eval_dir/logs/$repo/$task/$mode"
            mkdir -p "$logs"
            for run in $(seq 1 "$runs"); do
                log="$logs/$run.jsonl"
                if [ -f "$log" ] && grep -q '"type":"result"' "$log"; then
                    continue
                fi
                echo "$repo $task $mode run $run"
                if [ "$kind" = edit ]; then
                    reset "$dir"
                fi
                (cd "$dir" && claude -p "$prompt" "${common[@]}" "${mode_args[@]}" </dev/null >"$log") ||
                    echo "  claude exited $?; the log is kept and the run counts only if it has a result"
                if [ "$kind" = edit ]; then
                    tool_files=()
                    for path in .codemycel .gitnexus .codegraph .serena .claude AGENTS.md CLAUDE.md; do
                        if [ -e "$dir/$path" ] && ! git -C "$dir" check-ignore -q "$path"; then
                            tool_files+=(":!$path")
                        fi
                    done
                    git -C "$dir" add --intent-to-add --all -- . "${tool_files[@]}"
                    git -C "$dir" diff >"$logs/$run.patch"
                    reset "$dir"
                fi
            done
        done
    done < <("$bin/tasks" "$eval_dir/tasks/$repo.toml")
done

"$bin/report" "$eval_dir/logs"
