#!/usr/bin/env bash
# Build file_ops_component_aot.wasm from a WASM component:
#   scripts/build-aot.sh IN.wasm OUT.wasm
#
# Same content as the Bazel targets that built it up to v0.2.2
# (rules_wasm_component wasm_precompile + wasm_embed_aot in tinygo/BUILD.bazel):
# `wasmtime compile -O opt-level=2 -D debug-info=n --target <triple>` for six
# targets, then each .cwasm embedded as custom section aot-<name> right after
# the header, in this order. The embedding is kapsel-aot-embed, whose ignored
# test rebuilds the v0.2.2 asset byte for byte from its parts.
#
# A .cwasm loads only in the wasmtime version that compiled it, so the
# version is pinned: set WASMTIME_VERSION (e.g. 50.0.3) and the script fails
# unless `wasmtime --version` reports exactly that.
set -euo pipefail

if [ $# -ne 2 ]; then
  echo "usage: scripts/build-aot.sh IN.wasm OUT.wasm" >&2
  exit 2
fi
in=$1
out=$2
: "${WASMTIME_VERSION:?set WASMTIME_VERSION to the wasmtime version that must compile the AOT code}"

have=$(wasmtime --version | awk '{print $2}')
if [ "$have" != "$WASMTIME_VERSION" ]; then
  echo "wasmtime $have on PATH, expected $WASMTIME_VERSION" >&2
  exit 1
fi

work=$(mktemp -d)
trap 'rm -r "$work"' EXIT

specs=()
for t in \
  linux-x64:x86_64-unknown-linux-gnu \
  linux-arm64:aarch64-unknown-linux-gnu \
  darwin-x64:x86_64-apple-darwin \
  darwin-arm64:aarch64-apple-darwin \
  windows-x64:x86_64-pc-windows-gnu \
  portable:pulley64; do
  name=${t%%:*}
  triple=${t#*:}
  wasmtime compile -O opt-level=2 -D debug-info=n --target "$triple" "$in" -o "$work/$name.cwasm"
  specs+=("$name:$work/$name.cwasm")
done

cargo run --locked --quiet -p kapsel-aot-embed -- embed --input "$in" --output "$out" "${specs[@]}"
cargo run --locked --quiet -p kapsel-aot-embed -- list "$out"
