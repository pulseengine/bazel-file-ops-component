# bazel-file-ops-component

A WebAssembly component that prepares build workspaces (copy, create, concatenate, move) from one JSON config, for Bazel rules and other build tools that need the same file operations on every host.

## Vision

The goal is kapsel, a WASM capability shell for AI agents running under [zwinger](https://github.com/pulseengine/zwinger). kapsel is German for capsule, [ˈkapsl̩], "KAP-sel"; the nerd reading is "kap-shell". Common commands will run as WASI components that see only the directories zwinger grants, and shell syntax kapsel does not support is refused instead of handed to bash ([#140](https://github.com/pulseengine/bazel-file-ops-component/issues/140)). The repo will be renamed after the 0.2.1 release, and Bazel builds keep a JSON-config mode.

## Why use it

- **One component, the same operations on each host.** A single `file_ops_component.wasm` runs a JSON list of file operations under any WASI runtime. CI builds and tests it on Linux and macOS for every pull request. [CI workflow](.github/workflows/ci.yml), [tests](tinygo/json_bridge_test.go)
- **It touches only what the runtime grants.** Run under a WASI runtime, the component can reach only the directories you preopen, such as `--dir` in wasmtime. The quick start below shows it. [Config reference](docs/json-config.md)
- **Checked releases.** Each release publishes the component with a sha256 checksum. Every pull request to `main` must pass 9 required checks, including vulnerability, dependency and supply-chain scans. [Required checks](.github/required-contexts.txt), [releases](https://github.com/pulseengine/bazel-file-ops-component/releases)

## Quick start

You need [wasmtime](https://wasmtime.dev) and the GitHub CLI.

```sh
gh release download v0.2.0 --repo pulseengine/bazel-file-ops-component -p 'file_ops_component.wasm*'
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
| JSON-config operations in the WASI component (9 operation types) | works, released as v0.2.0 |
| CLI subcommands `copy_file`, `copy_directory`, `create_directory`, `prepare_workspace` | works; the subcommand handlers have no tests of their own |
| AOT-compiled component and native CLI archives for Linux and macOS | published with each release |
| WIT interfaces in [`wit/file-operations.wit`](wit/file-operations.wit) | declared only; the shipped component is a `wasi:cli` command and does not export them |
| Use from Bazel | rules_wasm_component downloads the component, but its toolchain runs its own Go wrapper today; switching to this component is planned ([rules_wasm_component#775](https://github.com/pulseengine/rules_wasm_component/issues/775)) |
| Windows | not tested |

## Where to find more

- [JSON config and CLI reference](docs/json-config.md)
- [Building and testing](docs/development.md)
- [Documentation site sources](docs-site/src/content/docs)
- [Security policy and reporting](SECURITY.md)
- [kapsel design and interface contract](https://github.com/pulseengine/bazel-file-ops-component/issues/140)

## Licence

Apache-2.0. Issues and pull requests are welcome.

---

<div align="center">

<sub>Part of <a href="https://github.com/pulseengine">PulseEngine</a> &mdash; toolchain for safety-critical systems</sub>

</div>
