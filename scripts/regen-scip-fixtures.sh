#!/usr/bin/env bash
# Regenerates the committed SCIP indexes of the fixtures. Maintainers only: CI and users never
# run an indexer, and codemycel only imports SCIP.
#
# Needs rust-analyzer (`rustup component add rust-analyzer`) and Bun. Each fixture is indexed in
# a temporary copy, so Cargo.lock, target/ and Bun's lockfile never land in the fixture, and only
# index.scip is copied back. Document paths inside the index are relative to the fixture root.
#
# scip-typescript runs under Bun (checked with Bun 1.4.4 and scip-typescript from npm on
# 2026-10-28). If a later version stops running under Bun, run the same command with
# `npx @sourcegraph/scip-typescript` on a machine with Node; the output is identical.
#
# The JavaScript package of the TypeScript fixture has no tsconfig.json, so the index has no
# documents for it and its files stay with the language server.
set -euo pipefail

repo="$(cd "$(dirname "$0")/.." && pwd)"
scratch="$(mktemp -d)"
trap 'rm -rf "$scratch"' EXIT

index_copy() {
  local fixture="$1"
  shift
  local copy="$scratch/$(basename "$fixture")"
  cp -a "$repo/$fixture" "$copy"
  (cd "$copy" && "$@")
  cp "$copy/index.scip" "$repo/$fixture/index.scip"
  echo "wrote $fixture/index.scip"
}

# The copy lies outside the repository, so resolve rust-analyzer from the repository's toolchain.
rust_analyzer="$(cd "$repo" && rustup which rust-analyzer)"
CARGO_TARGET_DIR="$scratch/target" index_copy fixtures/rust/workspace-basic \
  "$rust_analyzer" scip . --output index.scip

index_copy fixtures/typescript/monorepo-basic \
  bunx @sourcegraph/scip-typescript index --output index.scip \
  packages/core/tsconfig.json packages/app/tsconfig.json
