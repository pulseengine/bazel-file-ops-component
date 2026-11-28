//! The JSON config of kapsel's file-operations mode (REQ-KS-041, REQ-KS-017).
//!
//! This is the format `file_ops_component.wasm` reads: a `workspace_dir` and
//! a list of operations. [`parse_json_config`] accepts exactly what the
//! TinyGo component (v0.2.2) accepts and reports what it reports, because
//! Bazel's rules_wasm_component depends on that contract. It decodes the way
//! Go's `encoding/json` does: keys match case-insensitively, unknown keys
//! are ignored, `null` leaves a field unset, and the first type error wins.
//!
//! [`validate`] applies TinyGo's checks with one change that REQ-KS-017
//! requires: a relative `workspace_dir` (rules_wasm_component writes `"."`)
//! is accepted, and with it relative, execroot-relative source paths. An
//! unknown operation type is an error, as in TinyGo. concatenate_files also
//! takes `src_paths` as an alias of `sources` (owner, 2026-11-28).
//!
//! The serde derives give the same field names for code that writes
//! configs. They are not the decoder: use [`parse_json_config`] to read one.

pub mod gojson;

use gojson::{Decoder, GoSlice, Value};
use serde::{Deserialize, Serialize};

/// A batch of file operations run inside one workspace.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct JsonConfig {
    pub workspace_dir: String,
    pub operations: Vec<Operation>,
}

/// One operation. Which fields matter depends on `type`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Operation {
    #[serde(rename = "type")]
    pub op_type: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub src_path: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub dest_path: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub path: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub command: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub work_dir: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub output_file: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub content: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<String>,
    /// An alias of `sources` for concatenate_files, the name
    /// rules_wasm_component's docs and Go wrapper use (owner, 2026-11-28).
    /// The TinyGo component ignores it. Giving both is an error.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub src_paths: Vec<String>,
}

impl Operation {
    /// concatenate_files' sources: `sources`, or its alias `src_paths`.
    pub fn concat_sources(&self) -> &[String] {
        if self.sources.is_empty() {
            &self.src_paths
        } else {
            &self.sources
        }
    }
}

/// The operation types, in the order TinyGo's schema lists them.
pub const OPERATION_TYPES: [&str; 9] = [
    "copy_file",
    "mkdir",
    "copy_directory_contents",
    "run_command",
    "read_file",
    "write_file",
    "append_to_file",
    "concatenate_files",
    "move_path",
];

/// The `prepare_workspace` subcommand's config (`WorkspaceConfig` in
/// `tinygo/workspace.go`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceConfig {
    pub work_dir: String,
    pub sources: Vec<FileSpec>,
    pub headers: Vec<FileSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bindings_dir: Option<String>,
    pub dependencies: Vec<FileSpec>,
    /// 0 Rust, 1 Go, 2 C++, 3 JavaScript, 4 Generic.
    pub workspace_type: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub security_config: Option<SecurityConfig>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileSpec {
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub destination: Option<String>,
    pub preserve_permissions: bool,
    pub preserve_structure: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecurityConfig {
    /// 0 standard, 1 high, 2 strict.
    pub level: i64,
    pub allowed_dirs: Vec<String>,
    pub denied_patterns: Vec<String>,
    pub enforce_validation: bool,
}

// --- decoding -------------------------------------------------------------

#[derive(Default, Clone)]
struct RawOperation {
    op_type: String,
    src_path: String,
    dest_path: String,
    path: String,
    command: String,
    args: GoSlice<String>,
    work_dir: String,
    output_file: String,
    content: String,
    sources: GoSlice<String>,
    src_paths: GoSlice<String>,
}

const OPERATION_FIELDS: [&str; 11] = [
    "type",
    "src_path",
    "dest_path",
    "path",
    "command",
    "args",
    "work_dir",
    "output_file",
    "content",
    "sources",
    "src_paths",
];

fn decode_strings(d: &mut Decoder, v: &Value, out: &mut GoSlice<String>) {
    d.slice(v, out, "[]string", |d, item, s| d.string(item, s));
}

fn decode_operation(d: &mut Decoder, v: &Value, op: &mut RawOperation) {
    d.object(
        v,
        "Operation",
        "main.Operation",
        &OPERATION_FIELDS,
        |d, name, val| match name {
            "type" => d.string(val, &mut op.op_type),
            "src_path" => d.string(val, &mut op.src_path),
            "dest_path" => d.string(val, &mut op.dest_path),
            "path" => d.string(val, &mut op.path),
            "command" => d.string(val, &mut op.command),
            "args" => decode_strings(d, val, &mut op.args),
            "work_dir" => d.string(val, &mut op.work_dir),
            "output_file" => d.string(val, &mut op.output_file),
            "content" => d.string(val, &mut op.content),
            "sources" => decode_strings(d, val, &mut op.sources),
            _ => decode_strings(d, val, &mut op.src_paths),
        },
    );
}

/// Decode a JSON config the way TinyGo's `json.Unmarshal` does. The error
/// is the text Go would give (without the "failed to parse JSON config: "
/// prefix the component adds).
pub fn parse_json_config(data: &[u8]) -> Result<JsonConfig, String> {
    let v = gojson::parse(data)?;
    let mut d = Decoder::new();
    let mut ws = String::new();
    let mut ops: GoSlice<RawOperation> = GoSlice::default();
    d.object(
        &v,
        "JsonConfig",
        "main.JsonConfig",
        &["workspace_dir", "operations"],
        |d, name, val| {
            if name == "workspace_dir" {
                d.string(val, &mut ws);
            } else {
                d.slice(val, &mut ops, "[]main.Operation", decode_operation);
            }
        },
    );
    d.finish()?;
    Ok(JsonConfig {
        workspace_dir: ws,
        operations: ops
            .to_vec()
            .into_iter()
            .map(|o| Operation {
                op_type: o.op_type,
                src_path: o.src_path,
                dest_path: o.dest_path,
                path: o.path,
                command: o.command,
                args: o.args.to_vec(),
                work_dir: o.work_dir,
                output_file: o.output_file,
                content: o.content,
                sources: o.sources.to_vec(),
                src_paths: o.src_paths.to_vec(),
            })
            .collect(),
    })
}

#[derive(Default, Clone)]
struct RawFileSpec {
    source: String,
    destination: Option<String>,
    preserve_permissions: bool,
    preserve_structure: bool,
}

#[derive(Default, Clone)]
struct RawSecurity {
    level: i64,
    allowed_dirs: GoSlice<String>,
    denied_patterns: GoSlice<String>,
    enforce_validation: bool,
}

fn decode_file_specs(d: &mut Decoder, v: &Value, out: &mut GoSlice<RawFileSpec>) {
    d.slice(v, out, "[]main.FileSpec", |d, item, spec| {
        d.object(
            item,
            "FileSpec",
            "main.FileSpec",
            &[
                "source",
                "destination",
                "preserve_permissions",
                "preserve_structure",
            ],
            |d, name, val| match name {
                "source" => d.string(val, &mut spec.source),
                "destination" => d.opt_string(val, &mut spec.destination),
                "preserve_permissions" => d.boolean(val, &mut spec.preserve_permissions),
                _ => d.boolean(val, &mut spec.preserve_structure),
            },
        )
    });
}

fn file_specs(s: &GoSlice<RawFileSpec>) -> Vec<FileSpec> {
    s.to_vec()
        .into_iter()
        .map(|f| FileSpec {
            source: f.source,
            destination: f.destination,
            preserve_permissions: f.preserve_permissions,
            preserve_structure: f.preserve_structure,
        })
        .collect()
}

/// Decode a `prepare_workspace` config the way TinyGo's `json.Unmarshal`
/// does.
pub fn parse_workspace_config(data: &[u8]) -> Result<WorkspaceConfig, String> {
    let v = gojson::parse(data)?;
    let mut d = Decoder::new();
    let mut work_dir = String::new();
    let mut sources = GoSlice::default();
    let mut headers = GoSlice::default();
    let mut dependencies = GoSlice::default();
    let mut bindings_dir = None;
    let mut workspace_type = 0i64;
    let mut security: Option<RawSecurity> = None;
    d.object(
        &v,
        "WorkspaceConfig",
        "main.WorkspaceConfig",
        &[
            "work_dir",
            "sources",
            "headers",
            "bindings_dir",
            "dependencies",
            "workspace_type",
            "security_config",
        ],
        |d, name, val| match name {
            "work_dir" => d.string(val, &mut work_dir),
            "sources" => decode_file_specs(d, val, &mut sources),
            "headers" => decode_file_specs(d, val, &mut headers),
            "bindings_dir" => d.opt_string(val, &mut bindings_dir),
            "dependencies" => decode_file_specs(d, val, &mut dependencies),
            "workspace_type" => d.int(val, &mut workspace_type, "main.WorkspaceType"),
            _ => match val {
                Value::Null => security = None,
                Value::Object(_) => {
                    let sc = security.get_or_insert_with(RawSecurity::default);
                    d.object(
                        val,
                        "SecurityConfig",
                        "main.SecurityConfig",
                        &[
                            "level",
                            "allowed_dirs",
                            "denied_patterns",
                            "enforce_validation",
                        ],
                        |d, name, val| match name {
                            "level" => d.int(val, &mut sc.level, "main.SecurityLevel"),
                            "allowed_dirs" => decode_strings(d, val, &mut sc.allowed_dirs),
                            "denied_patterns" => decode_strings(d, val, &mut sc.denied_patterns),
                            _ => d.boolean(val, &mut sc.enforce_validation),
                        },
                    );
                }
                other => d.type_error(other.kind(), "main.SecurityConfig"),
            },
        },
    );
    d.finish()?;
    Ok(WorkspaceConfig {
        work_dir,
        sources: file_specs(&sources),
        headers: file_specs(&headers),
        bindings_dir,
        dependencies: file_specs(&dependencies),
        workspace_type,
        security_config: security.map(|s| SecurityConfig {
            level: s.level,
            allowed_dirs: s.allowed_dirs.to_vec(),
            denied_patterns: s.denied_patterns.to_vec(),
            enforce_validation: s.enforce_validation,
        }),
    })
}

// --- validation -------------------------------------------------------------

/// Go's `filepath.IsAbs` on Unix.
pub fn is_abs(p: &str) -> bool {
    p.starts_with('/')
}

/// Check a decoded config before anything runs. The error is the text
/// TinyGo gives after "invalid JSON config: ".
///
/// Two differences from TinyGo:
/// - REQ-KS-017: a relative `workspace_dir` is accepted, and when it is,
///   source paths may be relative too (execroot-relative). Destinations
///   must be relative either way.
/// - concatenate_files takes `src_paths` as an alias of `sources`
///   (owner, 2026-11-28); a non-empty value for both is an error.
pub fn validate(config: &JsonConfig) -> Result<(), String> {
    if config.workspace_dir.is_empty() {
        return Err("workspace_dir cannot be empty".to_string());
    }
    let relative_mode = !is_abs(&config.workspace_dir);
    for (i, op) in config.operations.iter().enumerate() {
        validate_operation(op, i, relative_mode)?;
    }
    Ok(())
}

fn validate_operation(op: &Operation, i: usize, relative_mode: bool) -> Result<(), String> {
    let src_abs = |what: &str, p: &str| -> Result<(), String> {
        if !relative_mode && !is_abs(p) {
            return Err(format!("operation {i}: {what} must be absolute: {p}"));
        }
        Ok(())
    };
    let rel = |what: &str, p: &str| -> Result<(), String> {
        if is_abs(p) {
            return Err(format!("operation {i}: {what} must be relative: {p}"));
        }
        Ok(())
    };
    match op.op_type.as_str() {
        t @ ("copy_file" | "copy_directory_contents" | "move_path") => {
            if op.src_path.is_empty() || op.dest_path.is_empty() {
                return Err(format!(
                    "operation {i}: {t} requires src_path and dest_path"
                ));
            }
            src_abs("src_path", &op.src_path)?;
            rel("dest_path", &op.dest_path)?;
        }
        "mkdir" => {
            if op.path.is_empty() {
                return Err(format!("operation {i}: mkdir requires path"));
            }
            if is_abs(&op.path) {
                return Err(format!(
                    "operation {i}: mkdir path must be relative: {}",
                    op.path
                ));
            }
        }
        "run_command" => {
            if op.command.is_empty() {
                return Err(format!("operation {i}: run_command requires command"));
            }
        }
        "read_file" => {
            if op.path.is_empty() {
                return Err(format!("operation {i}: read_file requires path"));
            }
            src_abs("path", &op.path)?;
        }
        t @ ("write_file" | "append_to_file") => {
            if op.path.is_empty() {
                return Err(format!("operation {i}: {t} requires path"));
            }
            rel("path", &op.path)?;
        }
        "concatenate_files" => {
            if !op.sources.is_empty() && !op.src_paths.is_empty() {
                return Err(format!(
                    "operation {i}: concatenate_files takes sources or src_paths, not both"
                ));
            }
            if op.concat_sources().is_empty() {
                return Err(format!("operation {i}: concatenate_files requires sources"));
            }
            if op.dest_path.is_empty() {
                return Err(format!(
                    "operation {i}: concatenate_files requires dest_path"
                ));
            }
            for (j, s) in op.concat_sources().iter().enumerate() {
                if !relative_mode && !is_abs(s) {
                    return Err(format!("operation {i}: source {j} must be absolute: {s}"));
                }
            }
            rel("dest_path", &op.dest_path)?;
        }
        other => return Err(format!("operation {i}: unknown operation type: {other}")),
    }
    Ok(())
}

#[cfg(test)]
mod tests;
