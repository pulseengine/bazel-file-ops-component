# JSON config and CLI reference

This describes `file_ops_component.wasm` as released in v0.2.0; the Rust component of v0.3.0 behaves the same on the equivalence corpus (`testdata/kapsel/fileops/`), apart from the differences its cases allowlist with a reason (REQ-KS-041). The source is
`crates/kapsel-config` and `crates/kapsel-fileops` (before v0.3.0, `tinygo/json_bridge.go` and `tinygo/main.go`). `--help` is not recognised: like any unknown
first argument, it prints the usage and exits 1. This page is the reference.

## Running it

The component is a `wasi:cli` command. Pass a config file as the only argument:

```sh
wasmtime run --dir <host-dir>::<guest-dir> file_ops_component.wasm <guest-dir>/config.json
```

Paths inside the config are guest paths, so they must lie under a directory you
preopen with `--dir`. The component cannot reach anything else.

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

- `workspace_dir` must be an absolute path. It is created if it does not exist.
- Operations run in order. The first failure stops the run with a non-zero exit.
- An unknown `type` is rejected before anything runs.

## Operations

| `type` | Fields | Notes |
|---|---|---|
| `copy_file` | `src_path` (absolute), `dest_path` (relative) | `dest_path` is joined to `workspace_dir` |
| `mkdir` | `path` (relative) | creates parents as needed |
| `copy_directory_contents` | `src_path` (absolute), `dest_path` (relative) | recursive |
| `concatenate_files` | `sources` (absolute paths), `dest_path` (relative) | in the order given |
| `move_path` | `src_path` (absolute), `dest_path` (relative) | rename, falling back to copy and remove |
| `read_file` | `path` (absolute), optional `output_file` (relative) | copies the content to `output_file` |
| `write_file` | `path` (relative), `content` | overwrites |
| `append_to_file` | `path` (relative), `content` | creates the file if missing |
| `run_command` | `command`, `args`, optional `work_dir`, optional `output_file` | runs the command with its arguments as an argv (no shell); `output_file` captures stdout |

`crates/kapsel-config/src/lib.rs` holds the config types. The removed TinyGo source also held a JSON schema (`GetJsonSchema`); the Rust source has none.

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
