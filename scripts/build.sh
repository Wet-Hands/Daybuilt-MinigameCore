#!/usr/bin/env bash
# Build the plugin and copy the uploadable component to dist/<name>.wasm.
# Usage: scripts/build.sh            (release, optimized)
#        scripts/build.sh --debug    (fast dev build)
set -euo pipefail
cd "$(dirname "$0")/.."

profile=release
[[ "${1:-}" == "--debug" ]] && profile=debug

rustup target add wasm32-wasip2 >/dev/null 2>&1 || true
if [[ $profile == release ]]; then cargo build --release; else cargo build; fi

name=$(sed -n 's/^name *= *"\(.*\)"/\1/p' Cargo.toml | head -n1)
src="target/wasm32-wasip2/$profile/${name//-/_}.wasm"

mkdir -p dist
cp "$src" "dist/$name.wasm"
echo "Built dist/$name.wasm ($(du -h "dist/$name.wasm" | cut -f1))"
