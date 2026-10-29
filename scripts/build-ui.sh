#!/usr/bin/env bash
# Builds the web view into crates/codemycel-ui/dist, which the `ui-bundle` feature embeds.
# Needs rustup; installs the wasm32 target and the wasm-bindgen CLI matching Cargo.lock.
set -euo pipefail
cd "$(dirname "$0")/.."

version=$(awk '/^name = "wasm-bindgen"$/ { getline; gsub(/"/, "", $3); print $3; exit }' Cargo.lock)
rustup target add wasm32-unknown-unknown
if ! wasm-bindgen --version 2>/dev/null | grep -qx "wasm-bindgen $version"; then
    cargo install wasm-bindgen-cli --version "$version" --locked
fi

target_dir=${CARGO_TARGET_DIR:-target}
cargo build --release --locked -p codemycel-ui --bin codemycel-ui-web --features web \
    --target wasm32-unknown-unknown

out=crates/codemycel-ui/dist
rm -rf "$out"
mkdir -p "$out"
wasm-bindgen --target web --no-typescript --out-dir "$out" --out-name codemycel \
    "$target_dir/wasm32-unknown-unknown/release/codemycel-ui-web.wasm"
cp crates/codemycel-ui/web/* "$out/"
