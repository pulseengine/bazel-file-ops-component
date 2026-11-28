# kapsel

*kapsel: German for capsule, [ˈkapsl̩], "KAP-sel"; nerd reading "kap-shell".*

kapsel, formerly bazel-file-ops-component, is a WebAssembly component that prepares build workspaces (copy, create, concatenate, move) from one JSON config, for Bazel rules and other build tools that need the same file operations on every host.

## Vision

The goal is a WASM capability shell for AI agents running under [zwinger](https://github.com/pulseengine/zwinger). Common commands run as WASI components that see only the directories zwinger grants. A check stage analyzes each command before anything runs and refuses syntax kapsel does not support, along with constructs that hide failures such as `|| true` ([#140](https://github.com/pulseengine/kapsel/issues/140)). Bazel builds keep a JSON-config mode.

## Why use it

- **One component for Linux and macOS.** A single `file_ops_component.wasm` runs a JSON list of file operations under a WASI runtime. From v0.3.0 it is built from Rust ([`crates/kapsel-fileops`](crates/kapsel-fileops)) and replaces the TinyGo component under the same asset names. On every pull request CI builds it on Linux and macOS, runs its tests and runs it against the released v0.2.2 TinyGo component on every case of an equivalence corpus; a disagreement fails the check and blocks the release. [CI workflow](.github/workflows/ci.yml), [equivalence test](crates/kapsel-fileops/tests/equivalence.rs)
- **It opens only what the runtime preopens.** It runs inside a WASI runtime, so it can open only the directories the runtime preopens, such as `--dir` in wasmtime. The quick start below runs it that way. [Config reference](docs/json-config.md)
- **Releases you can check.** Each release publishes the component with a sha256 checksum and as a cosign-signed OCI artifact, and 8 required checks gate every pull request to `main`. [Required checks](.github/required-contexts.txt), [releases](https://github.com/pulseengine/kapsel/releases)

The OCI artifact moves to `ghcr.io/pulseengine/kapsel` with the next release. The pre-rename path `ghcr.io/pulseengine/bazel-file-ops-component` receives the same signed artifacts through v0.3.x and stops at v0.4.0; its manifests carry an `org.opencontainers.image.description` annotation that names the new path.

## Quick start

You need [wasmtime](https://wasmtime.dev) and the GitHub CLI. These commands fetch v0.2.2, the last TinyGo release. The Rust component from v0.3.0 on runs the same way but needs wasmtime 46.0.0 or later (it imports WASI 0.2.12).

```sh
gh release download v0.2.2 --repo pulseengine/kapsel -p 'file_ops_component.wasm*'
shasum -a 256 -c file_ops_component.wasm.sha256
mkdir -p src && echo hello > src/a.txt
```

```json
{"workspace_dir": "/work/out",
 "operations": [
   {"type": "mkdir", "path": "include"},
   {"type": "copy_file", "src_path": "/work/src/a.txt", "dest_path": "include/a.txt"}]}
```

Save that as `ops.json`, then run:

```sh
wasmtime run --dir .::/work file_ops_component.wasm /work/ops.json   # writes out/include/a.txt
```

## Status

| Part | State |
|---|---|
| JSON-config operations in the WASI component (9 operation types) | works, released as v0.2.2 (TinyGo); the Rust replacement for v0.3.0 is equivalent on the whole corpus, with each allowlisted difference stated in its case |
| CLI subcommands `copy_file`, `copy_directory`, `create_directory`, `prepare_workspace` | works; covered by the equivalence corpus ([`06-cli.json`](testdata/kapsel/fileops/06-cli.json)) |
| AOT-compiled component (embedded code for Linux, macOS and Windows targets, and Pulley) | published with each release since v0.2.0. From v0.3.0 the code is compiled by wasmtime 50.0.3 and loads only in that version; CI runs the linux-x64 code and the equivalence test on the embedded component |
| Release archives `file-ops-tinygo-*.tar.gz` (v0.1.0 to v0.2.2) | Bazel build output and a TinyGo cache, not a CLI binary; not published from v0.3.0 |
| WIT interfaces in [`wit/file-operations.wit`](wit/file-operations.wit) | declared only; the shipped component is a `wasi:cli` command and does not export them |
| Use from Bazel | rules_wasm_component downloads v0.2.0, but its toolchain runs its own Go wrapper. [rules_wasm_component#775](https://github.com/pulseengine/rules_wasm_component/issues/775) tracks the mismatch, and [#140](https://github.com/pulseengine/kapsel/issues/140) records moving that toolchain to kapsel's JSON-config mode |
| Dependency and advisory checks in the required checks | `cargo audit` and `cargo deny` (advisories, bans, licenses, sources), blocking from v0.3.0; they replaced gosec, which blocked from v0.2.1 to v0.2.2 |
| The shell, check stage and native-tool access | design in [#140](https://github.com/pulseengine/kapsel/issues/140); not built |
| Windows | not tested |

## Where to find more

- [JSON config and CLI reference](docs/json-config.md)
- [Building and testing](docs/development.md)
- [Security policy and reporting](SECURITY.md)
- [kapsel design and interface contracts](https://github.com/pulseengine/kapsel/issues/140)

## Licence

Apache-2.0. Issues and pull requests are welcome.

---

<div align="center">

<sub>Part of <a href="https://github.com/pulseengine">PulseEngine</a> &mdash; a WebAssembly Component Model engine for safety-critical systems</sub>

</div>
