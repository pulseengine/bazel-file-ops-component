//! The JSON-config batch, ported from `tinygo/json_bridge.go`.

use std::time::Instant;

use kapsel_config::{JsonConfig, Operation, parse_json_config, validate};

use crate::gopath::{base, dir, is_abs, join, quote};
use crate::ops;
use crate::security::require_within;

/// What a run reports (`WorkspaceInfo`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkspaceInfo {
    pub prepared_files: Vec<String>,
    pub workspace_path: String,
    pub message: String,
    pub preparation_time_ms: u128,
}

/// `ProcessJsonConfig`.
pub fn process_json_config(config_json: &[u8]) -> Result<WorkspaceInfo, String> {
    let start = Instant::now();
    let config =
        parse_json_config(config_json).map_err(|e| format!("failed to parse JSON config: {e}"))?;
    run_config(&config, start)
}

/// Run a decoded config.
pub fn run_config(config: &JsonConfig, start: Instant) -> Result<WorkspaceInfo, String> {
    validate(config).map_err(|e| format!("invalid JSON config: {e}"))?;
    let ws = config.workspace_dir.as_str();
    ops::create_directory(ws).map_err(|e| format!("failed to create workspace directory: {e}"))?;
    let mut prepared = Vec::new();
    for (i, op) in config.operations.iter().enumerate() {
        let files = execute(op, ws).map_err(|e| format!("operation {i} failed: {e}"))?;
        prepared.extend(files);
    }
    Ok(WorkspaceInfo {
        prepared_files: prepared,
        workspace_path: ws.to_string(),
        message: format!(
            "Successfully processed {} operations",
            config.operations.len()
        ),
        preparation_time_ms: start.elapsed().as_millis(),
    })
}

/// `workspacePath`: `rel` joined to the workspace, required to resolve
/// inside it.
fn workspace_path(ws: &str, rel: &str, what: &str) -> Result<String, String> {
    let p = join(&[ws, rel]);
    require_within(ws, &p, what)?;
    Ok(p)
}

fn execute(op: &Operation, ws: &str) -> Result<Vec<String>, String> {
    match op.op_type.as_str() {
        "copy_file" => {
            let dest = workspace_path(ws, &op.dest_path, "dest_path")?;
            ops::copy_file(&op.src_path, &dest)?;
            Ok(vec![dest])
        }
        "mkdir" => {
            let p = workspace_path(ws, &op.path, "path")?;
            ops::create_directory(&p)?;
            Ok(vec![p])
        }
        "copy_directory_contents" => {
            let dest = workspace_path(ws, &op.dest_path, "dest_path")?;
            ops::copy_directory(&op.src_path, &dest)?;
            match ops::list_directory(&dest) {
                Ok(names) => Ok(names.iter().map(|n| join(&[&dest, n])).collect()),
                Err(_) => Ok(vec![dest]),
            }
        }
        "run_command" => run_command(op, ws),
        "read_file" => {
            let content = ops::read_file(&op.path)?;
            if !op.output_file.is_empty() {
                let out = workspace_path(ws, &op.output_file, "output_file")?;
                ops::write_file(&out, &content)
                    .map_err(|e| format!("failed to write output: {e}"))?;
                return Ok(vec![out]);
            }
            Ok(vec![op.path.clone()])
        }
        "write_file" => {
            let p = workspace_path(ws, &op.path, "path")?;
            ops::write_file(&p, op.content.as_bytes())?;
            Ok(vec![p])
        }
        "append_to_file" => {
            let p = workspace_path(ws, &op.path, "path")?;
            ops::append_to_file(&p, op.content.as_bytes())?;
            Ok(vec![p])
        }
        "concatenate_files" => {
            let dest = workspace_path(ws, &op.dest_path, "dest_path")?;
            ops::concatenate_files(&op.sources, &dest)?;
            Ok(vec![dest])
        }
        "move_path" => {
            let dest = workspace_path(ws, &op.dest_path, "dest_path")?;
            // A move removes its source, so the source must resolve inside
            // the workspace too.
            require_within(ws, &op.src_path, "src_path")?;
            ops::move_path(&op.src_path, &dest)?;
            Ok(vec![dest])
        }
        other => Err(format!("unsupported operation type: {other}")),
    }
}

/// `run_command`. A WASI component cannot start a process: the TinyGo
/// component's `os/exec` fails before it would, and so does this. The
/// confinement checks and the output directory come first, as there, and
/// the error is the one TinyGo gives:
/// - a bare command name: PATH lookup is not supported on WASI;
/// - a path: os/exec opens `/dev/null` for the child's stdin, then fails
///   to create the stdout pipe (`output_file`) or to start the process.
fn run_command(op: &Operation, ws: &str) -> Result<Vec<String>, String> {
    // work_dir and output_file stay inside the workspace or its parent:
    // rules_wasm_component's wit_bindgen.bzl writes to "../<out>".
    let bound = dir(ws);
    let work_dir = if op.work_dir.is_empty() {
        ws.to_string()
    } else if is_abs(&op.work_dir) {
        op.work_dir.clone()
    } else {
        join(&[ws, &op.work_dir])
    };
    require_within(&bound, &work_dir, "work_dir")?;

    let lookup_err = (base(&op.command) == op.command).then(|| {
        format!(
            "exec: {}: executable file not found in $PATH",
            quote(&op.command)
        )
    });

    if !op.output_file.is_empty() {
        let out = join(&[ws, &op.output_file]);
        require_within(&bound, &out, "output_file")?;
        ops::create_directory(&dir(&out))
            .map_err(|e| format!("failed to create output directory: {e}"))?;
        let err = match lookup_err {
            Some(e) => e,
            None => match ops::open_dev_null(false) {
                Err(e) => e,
                Ok(()) => "errno 52".to_string(), // ENOSYS creating the stdout pipe
            },
        };
        return Err(format!("command failed: {err}"));
    }

    let err = match lookup_err {
        Some(e) => e,
        None => ops::open_dev_null(false)
            .and_then(|()| ops::open_dev_null(true))
            .and_then(|()| ops::open_dev_null(true))
            .err()
            .unwrap_or_else(|| "operation not implemented".to_string()),
    };
    Err(format!("command failed: {err}"))
}
