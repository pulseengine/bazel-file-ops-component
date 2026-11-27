//! The `prepare_workspace` subcommand, ported from `tinygo/workspace.go`.

use std::time::Instant;

use kapsel_config::{FileSpec, WorkspaceConfig};

use crate::bridge::WorkspaceInfo;
use crate::gopath::{base, join};
use crate::ops::{self, PathInfo, path_exists};
use crate::security::{require_within, set_security_level};

/// `PrepareWorkspace`.
pub fn prepare_workspace(config: &WorkspaceConfig) -> Result<WorkspaceInfo, String> {
    let start = Instant::now();
    if let Some(sc) = &config.security_config {
        set_security_level(sc.level);
    }
    let wd = config.work_dir.as_str();
    ops::create_directory(wd).map_err(|e| format!("failed to create workspace directory: {e}"))?;
    let mut prepared = Vec::new();
    for (list, what) in [
        (&config.sources, "source"),
        (&config.headers, "header"),
        (&config.dependencies, "dependency"),
    ] {
        for spec in list {
            let files =
                copy_file_spec(spec, wd).map_err(|e| format!("failed to copy {what} file: {e}"))?;
            prepared.extend(files);
        }
    }
    if let Some(b) = &config.bindings_dir
        && path_exists(b) != PathInfo::NotFound
    {
        ops::copy_directory(b, wd)
            .map_err(|e| format!("failed to copy bindings directory: {e}"))?;
        prepared.push(format!("{wd}/* (bindings)"));
    }
    let kind = match config.workspace_type {
        0 => "Rust",
        1 => "Go",
        2 => "C++",
        3 => "JavaScript",
        4 => "Generic",
        _ => "Unknown",
    };
    Ok(WorkspaceInfo {
        message: format!(
            "Successfully prepared {kind} workspace with {} files",
            prepared.len()
        ),
        prepared_files: prepared,
        workspace_path: wd.to_string(),
        preparation_time_ms: start.elapsed().as_millis(),
    })
}

/// `copyFileSpec`.
pub fn copy_file_spec(spec: &FileSpec, dest_dir: &str) -> Result<Vec<String>, String> {
    let mut name = match &spec.destination {
        Some(d) => d.clone(),
        None if spec.preserve_structure => spec.source.clone(),
        None => base(&spec.source),
    };
    if spec.preserve_structure && name.contains('/') {
        let parts: Vec<&str> = name.split('/').collect();
        if parts.len() >= 2 {
            name = join(&parts[parts.len() - 2..]);
        }
    }
    let dest = join(&[dest_dir, &name]);
    require_within(dest_dir, &dest, "destination")?;
    ops::copy_file(&spec.source, &dest)?;
    Ok(vec![dest])
}
