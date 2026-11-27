//! The command line, ported from `tinygo/main.go`: a config path as the only
//! argument, or a subcommand. Output, messages and exit codes are the TinyGo
//! component's.

use crate::bridge::process_json_config;
use crate::ops;
use crate::workspace::prepare_workspace;
use std::io::{Read, Write};

/// Collected output of one run.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub stdout: String,
    pub stderr: String,
    pub code: i32,
}

impl Outcome {
    fn out(&mut self, s: &str) {
        self.stdout.push_str(s);
        self.stdout.push('\n');
    }
    fn fail(mut self, s: String) -> Self {
        self.stderr.push_str(&s);
        self.stderr.push('\n');
        self.code = 1;
        self
    }
}

const USAGE: &str = "TinyGo File Operations Component
Usage: file_ops <operation> [args...]

Operations:
  copy_file --src <src> --dest <dest>
  copy_directory --src <src> --dest <dest>
  create_directory --path <path>
  process_json_config --config <config_file>
  prepare_workspace --config <workspace_config>
";

/// Run with `args` (without the program name) and print the result.
pub fn main_with(args: &[String]) -> i32 {
    let o = run(args);
    let _ = std::io::stdout().write_all(o.stdout.as_bytes());
    let _ = std::io::stderr().write_all(o.stderr.as_bytes());
    let _ = std::io::stdout().flush();
    o.code
}

/// Run with `args` (without the program name).
pub fn run(args: &[String]) -> Outcome {
    let mut o = Outcome::default();
    let Some(operation) = args.first() else {
        o.stdout.push_str(USAGE);
        o.code = 1;
        return o;
    };
    if is_json_config_file(operation) {
        return json_config(o, operation);
    }
    let rest = &args[1..];
    match operation.as_str() {
        "copy_file" | "copy_directory" => {
            let (src, dest) = match parse_copy_args(rest) {
                Ok(v) => v,
                Err(e) => return o.fail(format!("Error parsing arguments: {e}")),
            };
            if operation == "copy_file" {
                if let Err(e) = ops::copy_file(&src, &dest) {
                    return o.fail(format!("Error copying file: {e}"));
                }
                o.out(&format!("Successfully copied {src} to {dest}"));
            } else {
                if let Err(e) = ops::copy_directory(&src, &dest) {
                    return o.fail(format!("Error copying directory: {e}"));
                }
                o.out(&format!("Successfully copied directory {src} to {dest}"));
            }
            o
        }
        "create_directory" => {
            if rest.len() < 2 || rest[0] != "--path" {
                return o.fail("Error parsing arguments: expected --path <path>".to_string());
            }
            if let Err(e) = ops::create_directory(&rest[1]) {
                return o.fail(format!("Error creating directory: {e}"));
            }
            o.out(&format!("Successfully created directory {}", rest[1]));
            o
        }
        "process_json_config" => match parse_config_arg(rest) {
            Ok(path) => json_config(o, &path),
            Err(e) => o.fail(format!("Error parsing arguments: {e}")),
        },
        "prepare_workspace" => {
            let path = match parse_config_arg(rest) {
                Ok(p) => p,
                Err(e) => return o.fail(format!("Error parsing arguments: {e}")),
            };
            let data = match read_config(&path) {
                Ok(d) => d,
                Err(e) => return o.fail(format!("Error reading config file: {e}")),
            };
            let config = match kapsel_config::parse_workspace_config(&data) {
                Ok(c) => c,
                Err(e) => return o.fail(format!("Error parsing workspace config: {e}")),
            };
            match prepare_workspace(&config) {
                Ok(r) => {
                    o.out("Workspace prepared successfully:");
                    o.out(&format!("  Path: {}", r.workspace_path));
                    o.out(&format!("  Files: {}", r.prepared_files.len()));
                    o.out(&format!("  Message: {}", r.message));
                    o.out(&format!("  Time: {} ms", r.preparation_time_ms));
                    o
                }
                Err(e) => o.fail(format!("Error preparing workspace: {e}")),
            }
        }
        _ => {
            o.stderr
                .push_str(&format!("Unknown operation: {operation}\n"));
            o.stdout.push_str(USAGE);
            o.code = 1;
            o
        }
    }
}

/// `isJSONConfigFile`: ends in ".json", or names an existing non-directory.
fn is_json_config_file(p: &str) -> bool {
    if p.len() > 5 && p.ends_with(".json") {
        return true;
    }
    crate::gofs::stat(p).is_ok_and(|m| !m.is_dir())
}

/// `os.ReadFile` of the config.
fn read_config(p: &str) -> Result<Vec<u8>, String> {
    let mut f = crate::gofs::open(p).map_err(|e| crate::goerr::path_err("open", p, &e))?;
    let mut data = Vec::new();
    f.read_to_end(&mut data)
        .map_err(|e| crate::goerr::path_err("read", p, &e))?;
    Ok(data)
}

fn json_config(mut o: Outcome, path: &str) -> Outcome {
    let data = match read_config(path) {
        Ok(d) => d,
        Err(e) => return o.fail(format!("Error reading config file: {e}")),
    };
    match process_json_config(&data) {
        Ok(r) => {
            o.out("JSON config processed successfully:");
            o.out(&format!("  Workspace: {}", r.workspace_path));
            o.out(&format!("  Files: {}", r.prepared_files.len()));
            o.out(&format!("  Time: {} ms", r.preparation_time_ms));
            o
        }
        Err(e) => o.fail(format!("Error processing JSON config: {e}")),
    }
}

fn parse_copy_args(args: &[String]) -> Result<(String, String), String> {
    if args.len() < 4 {
        return Err("copy operations require --src <src> --dest <dest>".to_string());
    }
    let (mut src, mut dest) = (String::new(), String::new());
    let mut i = 0;
    while i + 1 < args.len() {
        match args[i].as_str() {
            "--src" => src = args[i + 1].clone(),
            "--dest" => dest = args[i + 1].clone(),
            other => return Err(format!("unknown argument: {other}")),
        }
        i += 2;
    }
    if src.is_empty() {
        return Err("--src is required".to_string());
    }
    if dest.is_empty() {
        return Err("--dest is required".to_string());
    }
    Ok((src, dest))
}

fn parse_config_arg(args: &[String]) -> Result<String, String> {
    if args.len() < 2 || args[0] != "--config" {
        return Err("expected --config <config_file>".to_string());
    }
    Ok(args[1].clone())
}
