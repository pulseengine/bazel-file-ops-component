# Building and testing

Everything is one Cargo workspace (`crates/`), on Rust 1.100.0
(`rust-toolchain.toml`). There is no Go, Python or Bazel in the repository
from v0.3.0 on; the TinyGo component and its Bazel build were removed when
the Rust component replaced it (REQ-KS-041).

## Workspace

```sh
cargo build --locked --workspace
cargo test --locked --workspace --no-fail-fast
```

## The JSON-config component

`crates/kapsel-fileops` builds the component released as
`file_ops_component.wasm`:

```sh
cargo build --locked -p kapsel-fileops --target wasm32-wasip2 --release
# target/wasm32-wasip2/release/kapsel-fileops.wasm
```

It imports WASI 0.2.12 (including `wasi:cli/exit` `exit-with-code`), so it
needs wasmtime 46.0.0 or later. `scripts/install-wasmtime.sh` installs the
pinned 50.0.3, checked by sha256.

### Equivalence with the TinyGo release

`crates/kapsel-fileops/tests/equivalence.rs` runs every case in
`testdata/kapsel/fileops/*.json` with the released v0.2.2 TinyGo component and
with the Rust one, and compares files, output and exit codes. It downloads the
oracle with `gh` and refuses to run unless its sha256 is the pinned value:

```sh
KAPSEL_FILEOPS_ORACLE_DIR=/some/dir \
  cargo test --locked -p kapsel-fileops --test equivalence -- --ignored --nocapture
```

`KAPSEL_FILEOPS_WASM` points the test at another build, such as the
AOT-embedded asset.

### AOT-embedded component

```sh
WASMTIME_VERSION=50.0.3 scripts/build-aot.sh \
  target/wasm32-wasip2/release/kapsel-fileops.wasm file_ops_component_aot.wasm
```

It precompiles the component with `wasmtime compile` for linux-x64,
linux-arm64, darwin-x64, darwin-arm64, windows-x64 and portable (Pulley) and
embeds each as a custom section `aot-<name>` with `crates/kapsel-aot-embed`,
the layout rules_wasm_component's `wasm_embed_aot` wrote up to v0.2.2.
`cargo run -p kapsel-aot-embed -- list|extract|strip` reads it back. A
section loads only in the wasmtime version that compiled it.

## Checks that gate a pull request

`.github/required-contexts.txt` lists the 8 checks that must pass before a
pull request can merge into `main`. The workflows are in `.github/workflows/`:
`ci.yml` covers the component build, tests, equivalence and integration, and
`security.yml` runs `cargo audit`, `cargo deny`, the vulnerability scans, the
supply-chain checks and the self-hosted fork guard.
