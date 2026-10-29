#!/bin/sh
# Installs codemycel with install.sh from release archives in a local directory, then indexes
# the TypeScript fixture with nothing pre-installed and the Rust fixture with only rustup.
#   scripts/clean-machine/unix.sh <directory with codemycel-<target>.tar.gz and .sha256> [log directory]
# Runs on Linux and macOS with POSIX sh, curl and tar.
set -eu

artefacts=$(cd "$1" && pwd)
repo=$(cd "$(dirname "$0")/../.." && pwd)
work=$(mktemp -d)
logs=${2:-$work/logs}
mkdir -p "$logs"
logs=$(cd "$logs" && pwd)

export RUSTUP_HOME="${RUSTUP_HOME:-$HOME/.rustup}"
export CARGO_HOME="${CARGO_HOME:-$HOME/.cargo}"
export HOME="$work/home"
mkdir -p "$HOME"
unset CODEMYCEL_HOME CODEMYCEL_INSTALL_BASE_URL
export CODEMYCEL_NO_UPDATE_CHECK=1
base_path=/usr/bin:/bin:/usr/sbin:/sbin
bin="$HOME/.local/bin"

fail() {
    echo "clean-machine: $*" >&2
    exit 1
}

expect() {
    grep -F -q -- "$2" "$1" || fail "$1 lacks: $2"
}

for tool in node npm npx tsc tsgo bun deno rust-analyzer; do
    if PATH=$base_path command -v "$tool" >/dev/null 2>&1; then
        fail "$tool is on the base PATH ($(PATH=$base_path command -v "$tool"))"
    fi
done

echo "== install"
PATH=$base_path CODEMYCEL_INSTALL_BASE_URL="file://$artefacts" sh -s <"$repo/install.sh" \
    >"$logs/install.log" 2>&1 || {
    cat "$logs/install.log"
    fail "install.sh failed"
}
cat "$logs/install.log"
expect "$logs/install.log" "installed $bin/codemycel"
expect "$logs/install.log" "$bin is not on your PATH"
expect "$logs/install.log" "run codemycel setup to configure your agents"
PATH=$base_path "$bin/codemycel" --version

# Indexes a copy of fixtures/<fixture> with PATH set to <path>, without the committed SCIP
# index, so the language server does the verification.
index() {
    name=$1
    fixture=$2
    path=$3
    copy="$work/$name"
    cp -R "$repo/fixtures/$fixture" "$copy"
    rm -f "$copy/index.scip"
    echo "== init $name"
    if ! PATH=$path "$bin/codemycel" init --repo "$copy" >"$logs/$name-init.log" 2>&1; then
        cat "$logs/$name-init.log"
        fail "init on $fixture failed"
    fi
    cat "$logs/$name-init.log"
    PATH=$path "$bin/codemycel" status --json --repo "$copy" >"$logs/$name-status.json" ||
        fail "status on $fixture is degraded"
    if grep -q degraded "$logs/$name-status.json"; then
        cat "$logs/$name-status.json"
        fail "$fixture has a degraded root"
    fi
}

symbol() {
    name=$1
    path=$2
    target=$3
    PATH=$path "$bin/codemycel" symbol "$target" --repo "$work/$name" >"$logs/$name-symbol.txt" ||
        fail "symbol $target failed"
    cat "$logs/$name-symbol.txt"
}

ts_path="$bin:$base_path"
index typescript typescript/monorepo-basic "$ts_path"
expect "$logs/typescript-init.log" "(download), verified by lsp"
symbol typescript "$ts_path" packages/core/src/math.ts::double
expect "$logs/typescript-symbol.txt" "← calls  packages/app/src/main.ts::build (function)"
expect "$logs/typescript-symbol.txt" "→ calls  packages/core/src/math.ts::scale (function)"

rust_path="$bin:$CARGO_HOME/bin:$base_path"
PATH=$rust_path command -v rustup >/dev/null || fail "rustup is not in $CARGO_HOME/bin"
index rust rust/workspace-basic "$rust_path"
expect "$logs/rust-init.log" "root . (rust): verified by lsp"
symbol rust "$rust_path" crates/core/src/lib.rs::total_area
expect "$logs/rust-symbol.txt" "← calls  crates/cli/src/main.rs::main (function)"
expect "$logs/rust-symbol.txt" "→ calls  crates/core/src/geometry.rs::Area::area (method)"

echo "clean-machine: ok"
