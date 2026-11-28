# JSON config and CLI reference

This describes `file_ops_component.wasm`. Up to v0.2.2 it is the TinyGo component
(`tinygo/json_bridge.go`, `tinygo/main.go`, removed in v0.3.0). From v0.3.0 it is the Rust component
`crates/kapsel-fileops` (config model in `crates/kapsel-config`), which keeps the same
command line, output and exit codes. The differences are listed under
[Changes in the Rust component](#changes-in-the-rust-component). `--help` is not
recognised: like any unknown first argument, it prints the usage and exits 1. This page is
the reference.

## Running it

The component is a `wasi:cli` command. Pass a config file as the only argument:

```sh
wasmtime run --dir <host-dir>::<guest-dir> file_ops_component.wasm <guest-dir>/config.json
```

Paths inside the config are guest paths, so they must lie under a directory you
preopen with `--dir`. The component cannot reach anything else.

For a relative `workspace_dir` (the Rust component only), preopen the working directory as
`.`, for example `--dir .::.`, and give execroot-relative paths.

An argument that ends in `.json`, or names an existing file, is treated as a
config file. Otherwise the first argument is a subcommand (below).

## Config format

```json
{
  "workspace_dir": "/build/workspace",
  "operations": [
    {"type": "copy_file", "src_path": "/src/main.cpp", "dest_path": "main.cpp"},
    {"type": "mkdir", "path": "include/foundation"},
    {"type": "copy_directory_contents", "src_path": "/headers", "dest_path": "include"}
  ]
}
```

- `workspace_dir` is created if it does not exist. The TinyGo component requires an
  absolute path. The Rust component also accepts a relative one, such as `"."`
  (REQ-KS-017), and then also accepts relative source paths.
- Operations run in order. The first failure stops the run with a non-zero exit.
- An unknown `type` is rejected before anything runs.
- Keys match case-insensitively and unknown keys are ignored, as Go's `encoding/json` does.

## Operations

| `type` | Fields | Notes |
|---|---|---|
| `copy_file` | `src_path` (absolute), `dest_path` (relative) | `dest_path` is joined to `workspace_dir` |
| `mkdir` | `path` (relative) | creates parents as needed |
| `copy_directory_contents` | `src_path` (absolute), `dest_path` (relative) | recursive |
| `concatenate_files` | `sources` or `src_paths` (absolute paths), `dest_path` (relative) | in the order given; `src_paths` is Rust only, and giving both is an error |
| `move_path` | `src_path` (absolute, inside the workspace), `dest_path` (relative) | rename, falling back to copy and remove |
| `read_file` | `path` (absolute), optional `output_file` (relative) | copies the content to `output_file` |
| `write_file` | `path` (relative), `content` | overwrites |
| `append_to_file` | `path` (relative), `content` | creates the file if missing |
| `run_command` | `command`, `args`, optional `work_dir`, optional `output_file` | accepted, but a WASI component cannot start a process: the operation always fails after its `work_dir` and `output_file` checks |

"Absolute" source paths may be relative when `workspace_dir` is relative (Rust only).

Every destination must resolve inside `workspace_dir` after symlinks are followed. A path
with a `..` component is refused, and nothing is written through an existing symlink.
`run_command`'s `work_dir` and `output_file` may also lie in the parent of `workspace_dir`.

The removed TinyGo source (`GetJsonSchema` in `tinygo/json_bridge.go`, up to v0.2.2) held a JSON schema for its format; the Rust source has none. The config types are in `crates/kapsel-config/src/lib.rs`.

## Changes in the Rust component

The equivalence test `crates/kapsel-fileops/tests/equivalence.rs` runs both components on
`testdata/kapsel/fileops/`. Each difference it allows belongs to one of these classes:

- **Relative `workspace_dir`** (REQ-KS-017). rules_wasm_component writes `"."` with
  execroot-relative paths. The Rust component accepts it. TinyGo stops with
  "workspace_dir must be an absolute path".
- **`src_paths` alias** (owner, 2026-11-28). `concatenate_files` takes `src_paths`, the name
  rules_wasm_component's docs and Go wrapper use, as an alias of `sources`. Giving both is
  an error: "concatenate_files takes sources or src_paths, not both". TinyGo ignores
  `src_paths` and fails with "requires sources".
- **Same-path preopen** (owner, 2026-11-28). Under `--dir /ws::/ws`, where the directories
  above the preopen are not visible to the guest, TinyGo v0.2.2 refuses every destination
  ("… resolves outside …"). The Rust component resolves symlinks from the preopen root
  instead, so writes inside the workspace work. Escapes are still refused.
- **No crash on a directory**. Where TinyGo crashes reading a directory as a file (exit
  134), the Rust component reports an error and exits 1.

## Subcommands

```text
file_ops copy_file --src <src> --dest <dest>
file_ops copy_directory --src <src> --dest <dest>
file_ops create_directory --path <path>
file_ops process_json_config --config <config_file>
file_ops prepare_workspace --config <workspace_config>
```

`prepare_workspace` takes a workspace config (`WorkspaceConfig` in
`crates/kapsel-config`): a work directory plus lists of sources, headers and
dependencies, each a file spec with a source and an optional destination.

## WIT interfaces

`wit/file-operations.wit` (`bazel:file-ops@1.0.0`) declares the
`file-operations`, `json-batch-operations`, `workspace-management` and
`security-operations` interfaces. The shipped component is built as a
`wasi:cli/command` (`crates/kapsel-fileops`) and does not export them. They are a
design record, not a callable API.

## Release assets

Moved here from the README front page when it was rewritten for v0.3.0.

| Asset | What it is |
|---|---|
| `file_ops_component.wasm` (+ `.sha256`) | The JSON-config component. v0.1.0 to v0.2.2 were TinyGo; from v0.3.0 it is built from Rust (`crates/kapsel-fileops`) and runs the released v0.2.2 component's equivalence corpus on every pull request and before every release. It imports WASI 0.2.12, so it needs wasmtime 46.0.0 or later. |
| `file_ops_component_aot.wasm` (+ `.sha256`) | The same component with precompiled code embedded for Linux, macOS and Windows targets and Pulley, published since v0.2.0. From v0.3.0 the code is compiled by wasmtime 50.0.3 and loads only in that version; CI runs the linux-x64 code and the equivalence test on the embedded component. |
| `kapsel-compile-proofs.tar.gz` (+ `.sha256`, `.sigstore.json`) | From v0.3.0: the ordeal certificates for the compiler's proofs, cosign-signed; see `crates/kapsel-compile/proofs/README.md`. |
| OCI artifacts | `ghcr.io/pulseengine/kapsel:<tag>` and `:<tag>-aot`, cosign-signed with SLSA provenance. The pre-rename path `ghcr.io/pulseengine/bazel-file-ops-component` receives the same signed artifacts through v0.3.x and stops at v0.4.0; its manifests carry an `org.opencontainers.image.description` annotation naming the new path. |
| `file-ops-tinygo-*.tar.gz` | v0.1.0 to v0.2.2 only: Bazel build output and a TinyGo cache, not a CLI binary. Not published from v0.3.0. |

The interfaces in `wit/file-operations.wit` are declared only: the shipped component is a `wasi:cli` command and does not export them. Bazel's rules_wasm_component pins v0.2.0 and runs its own Go wrapper today; switching its toolchain to this component's JSON mode with `workspace_dir: "."` is planned for after v0.3.0 (rules_wasm_component#775, kapsel#140).
