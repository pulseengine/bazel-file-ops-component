#!/usr/bin/env bash
# Install the pinned wasmtime into DIR (default: $RUNNER_TEMP/wasmtime) and,
# under GitHub Actions, put DIR on PATH for the following steps:
#   scripts/install-wasmtime.sh [DIR]
#
# 50.0.3 is the version rules_wasm_component registers (MODULE.bazel,
# checksums/tools/wasmtime.json, same sha256 values). Components built by
# Rust 1.100 for wasm32-wasip2 import wasi:cli/exit@0.2.12 exit-with-code and
# need wasmtime >= 46.0.0 (ordeal#309). The AOT code in
# file_ops_component_aot.wasm is compiled by this version and loads only in it.
set -euo pipefail

VERSION=50.0.3
dir=${1:-${RUNNER_TEMP:?set RUNNER_TEMP or pass DIR}/wasmtime}

case "$(uname -s)-$(uname -m)" in
  Linux-x86_64)  plat=x86_64-linux;  sum=c59133a9334e96eaca5e26f0d4c35199bfe43ab34049aa254ef4b47af1361476 ;;
  Linux-aarch64) plat=aarch64-linux; sum=c6aabff1e38536c05b0cb90d60a6a93770f3c49bacd5f7881653bc2c41263268 ;;
  Darwin-arm64)  plat=aarch64-macos; sum=b16fc3c60341fb20b85adb3ba04549eeb3e99b9e56a6d317a142584c07a505d4 ;;
  Darwin-x86_64) plat=x86_64-macos;  sum=e932d811801e6d7646683b59cfb3b30e292049dbe04f26e324eb6f6a069b927b ;;
  *) echo "no pinned wasmtime $VERSION for $(uname -s)-$(uname -m)" >&2; exit 1 ;;
esac

name="wasmtime-v${VERSION}-${plat}"
mkdir -p "$dir"
tarball="$dir/$name.tar.xz"
curl -fsSLo "$tarball" "https://github.com/bytecodealliance/wasmtime/releases/download/v${VERSION}/${name}.tar.xz"
echo "${sum}  ${tarball}" | shasum -a 256 -c -
tar xJf "$tarball" --strip-components=1 -C "$dir" "$name/wasmtime"
rm "$tarball"
"$dir/wasmtime" --version
if [ -n "${GITHUB_PATH:-}" ]; then
  echo "$dir" >> "$GITHUB_PATH"
fi
