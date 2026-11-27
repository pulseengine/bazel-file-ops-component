//! File operations, ported from `tinygo/operations.go` with the same order
//! of checks and system calls and the same error texts.

use std::fs::File;
use std::io::{self, Read, Write};

use crate::goerr::{is_not_exist, path_err};
use crate::gofs;
use crate::gopath::{dir, join};
use crate::security::{refuse_symlink, validate_path};

/// Go's `os.MkdirAll`.
pub fn mkdir_all(path: &str) -> Result<(), String> {
    match gofs::stat(path) {
        Ok(md) if md.is_dir() => return Ok(()),
        Ok(_) => return Err(format!("mkdir {path}: errno 54")), // ENOTDIR
        Err(_) => {}
    }
    let b = path.as_bytes();
    let mut i = b.len();
    while i > 0 && b[i - 1] == b'/' {
        i -= 1;
    }
    let mut j = i;
    while j > 0 && b[j - 1] != b'/' {
        j -= 1;
    }
    if j > 1 {
        mkdir_all(&path[..j - 1])?;
    }
    if let Err(e) = gofs::mkdir(path) {
        if let Ok(md) = gofs::lstat(path)
            && md.is_dir()
        {
            return Ok(());
        }
        return Err(path_err("mkdir", path, &e));
    }
    Ok(())
}

/// Create the parent of `p` unless it is "." or "/".
fn mkdir_parent(p: &str, what: &str) -> Result<(), String> {
    let d = dir(p);
    if d != "." && d != "/" {
        mkdir_all(&d).map_err(|e| format!("failed to create {what} {d}: {e}"))?;
    }
    Ok(())
}

/// Go's `io.Copy` with the errors as `*PathError`s.
fn copy_stream(
    src: &mut File,
    src_path: &str,
    dest: &mut File,
    dest_path: &str,
) -> Result<(), String> {
    let mut buf = vec![0u8; 32 * 1024];
    loop {
        let n = match src.read(&mut buf) {
            Ok(0) => return Ok(()),
            Ok(n) => n,
            Err(e) => return Err(read_err(src_path, &e)),
        };
        dest.write_all(&buf[..n])
            .map_err(|e| path_err("write", dest_path, &e))?;
    }
}

/// A read error. Reading a directory is EBADF in TinyGo's wasip2 `os`.
fn read_err(p: &str, e: &io::Error) -> String {
    if std::fs::metadata(gofs::sys(p)).is_ok_and(|m| m.is_dir()) {
        return format!("read {p}: errno 8");
    }
    path_err("read", p, e)
}

/// Go's `os.ReadFile`.
fn read_file_bytes(p: &str) -> Result<Vec<u8>, String> {
    let mut f = gofs::open(p).map_err(|e| path_err("open", p, &e))?;
    let mut out = Vec::new();
    f.read_to_end(&mut out).map_err(|e| read_err(p, &e))?;
    Ok(out)
}

/// Go's `os.WriteFile`.
fn write_file_bytes(p: &str, data: &[u8]) -> Result<(), String> {
    let mut f = gofs::create_trunc(p).map_err(|e| path_err("open", p, &e))?;
    f.write_all(data).map_err(|e| path_err("write", p, &e))
}

/// Go's `os.ReadDir`: names sorted, with "is a directory" from the entry
/// itself (a symlink is not a directory).
fn read_dir_sorted(d: &str) -> Result<Vec<(String, bool)>, String> {
    let rd = gofs::read_dir(d).map_err(|e| path_err("open", d, &e))?;
    let mut out = Vec::new();
    for entry in rd {
        let entry = entry.map_err(|e| path_err("readdirent", d, &e))?;
        let is_dir = entry.file_type().is_ok_and(|t| t.is_dir());
        out.push((entry.file_name().to_string_lossy().into_owned(), is_dir));
    }
    out.sort();
    Ok(out)
}

/// `CopyFile`.
pub fn copy_file(src: &str, dest: &str) -> Result<(), String> {
    validate_path(src).map_err(|e| format!("security validation failed for source: {e}"))?;
    validate_path(dest).map_err(|e| format!("security validation failed: {e}"))?;
    mkdir_parent(dest, "destination directory")?;
    let mut s = gofs::open(src).map_err(|e| {
        format!(
            "failed to open source file {src}: {}",
            path_err("open", src, &e)
        )
    })?;
    refuse_symlink(dest)?;
    let mut d = gofs::create_trunc(dest).map_err(|e| {
        format!(
            "failed to create destination file {dest}: {}",
            path_err("open", dest, &e)
        )
    })?;
    copy_stream(&mut s, src, &mut d, dest).map_err(|e| format!("failed to copy file contents: {e}"))
}

/// `CopyDirectory`.
pub fn copy_directory(src: &str, dest: &str) -> Result<(), String> {
    validate_path(src).map_err(|e| format!("security validation failed for source: {e}"))?;
    validate_path(dest).map_err(|e| format!("security validation failed: {e}"))?;
    match gofs::stat(src) {
        Err(_) => return Err(format!("source directory does not exist: {src}")),
        Ok(md) if !md.is_dir() => return Err(format!("source is not a directory: {src}")),
        Ok(_) => {}
    }
    refuse_symlink(dest)?;
    mkdir_all(dest).map_err(|e| format!("failed to create destination directory {dest}: {e}"))?;
    copy_directory_contents(src, dest)
}

fn copy_directory_contents(src: &str, dest: &str) -> Result<(), String> {
    let entries =
        read_dir_sorted(src).map_err(|e| format!("failed to read source directory {src}: {e}"))?;
    for (name, is_dir) in entries {
        let s = join(&[src, &name]);
        let d = join(&[dest, &name]);
        if is_dir {
            refuse_symlink(&d)?;
            mkdir_all(&d).map_err(|e| format!("failed to create subdirectory {d}: {e}"))?;
            copy_directory_contents(&s, &d)?;
        } else {
            copy_file(&s, &d).map_err(|e| format!("failed to copy file {name}: {e}"))?;
        }
    }
    Ok(())
}

/// `CreateDirectory`.
pub fn create_directory(p: &str) -> Result<(), String> {
    validate_path(p).map_err(|e| format!("security validation failed: {e}"))?;
    refuse_symlink(p)?;
    mkdir_all(p).map_err(|e| format!("failed to create directory {p}: {e}"))
}

/// `RemovePath`: a missing path is not an error.
pub fn remove_path(p: &str) -> Result<(), String> {
    validate_path(p).map_err(|e| format!("security validation failed: {e}"))?;
    let r = match gofs::lstat(p) {
        Err(e) => Err(e),
        Ok(md) if md.is_dir() => gofs::remove_dir_all(p),
        Ok(_) => gofs::remove_file(p),
    };
    match r {
        Err(e) if !is_not_exist(&e) => Err(format!(
            "failed to remove path {p}: {}",
            path_err("unlinkat", p, &e)
        )),
        _ => Ok(()),
    }
}

/// `ListDirectory(dir, nil)`.
pub fn list_directory(d: &str) -> Result<Vec<String>, String> {
    validate_path(d).map_err(|e| format!("security validation failed: {e}"))?;
    let entries = read_dir_sorted(d).map_err(|e| format!("failed to read directory {d}: {e}"))?;
    Ok(entries.into_iter().map(|(n, _)| n).collect())
}

/// `ReadFile`.
pub fn read_file(p: &str) -> Result<Vec<u8>, String> {
    validate_path(p).map_err(|e| format!("security validation failed: {e}"))?;
    read_file_bytes(p).map_err(|e| format!("failed to read file {p}: {e}"))
}

/// `WriteFile`.
pub fn write_file(p: &str, content: &[u8]) -> Result<(), String> {
    validate_path(p).map_err(|e| format!("security validation failed: {e}"))?;
    mkdir_parent(p, "parent directory")?;
    refuse_symlink(p)?;
    write_file_bytes(p, content).map_err(|e| format!("failed to write file {p}: {e}"))
}

/// `AppendToFile`.
pub fn append_to_file(p: &str, content: &[u8]) -> Result<(), String> {
    validate_path(p).map_err(|e| format!("security validation failed: {e}"))?;
    mkdir_parent(p, "parent directory")?;
    refuse_symlink(p)?;
    let mut f = gofs::open_append(p).map_err(|e| {
        format!(
            "failed to open file {p} for appending: {}",
            path_err("open", p, &e)
        )
    })?;
    f.write_all(content)
        .map_err(|e| format!("failed to append to file {p}: {}", path_err("write", p, &e)))
}

/// `ConcatenateFiles`.
pub fn concatenate_files(sources: &[String], dest: &str) -> Result<(), String> {
    validate_path(dest).map_err(|e| format!("security validation failed for destination: {e}"))?;
    if sources.is_empty() {
        return Err("no source files provided for concatenation".to_string());
    }
    mkdir_parent(dest, "destination directory")?;
    refuse_symlink(dest)?;
    let mut d = gofs::create_trunc(dest).map_err(|e| {
        format!(
            "failed to create destination file {dest}: {}",
            path_err("open", dest, &e)
        )
    })?;
    for (i, s) in sources.iter().enumerate() {
        validate_path(s)
            .map_err(|e| format!("security validation failed for source {i} ({s}): {e}"))?;
        let content =
            read_file_bytes(s).map_err(|e| format!("failed to read source file {s}: {e}"))?;
        d.write_all(&content).map_err(|e| {
            format!(
                "failed to write content from {s} to destination: {}",
                path_err("write", dest, &e)
            )
        })?;
    }
    Ok(())
}

/// `MovePath`: rename, falling back to copy and remove.
pub fn move_path(src: &str, dest: &str) -> Result<(), String> {
    validate_path(src).map_err(|e| format!("security validation failed for source: {e}"))?;
    validate_path(dest).map_err(|e| format!("security validation failed for destination: {e}"))?;
    mkdir_parent(dest, "destination directory")?;
    refuse_symlink(dest)?;
    if gofs::rename(src, dest).is_ok() {
        return Ok(());
    }
    let md = gofs::stat(src)
        .map_err(|e| format!("failed to stat source {src}: {}", path_err("stat", src, &e)))?;
    if md.is_dir() {
        copy_directory(src, dest)
            .map_err(|e| format!("failed to copy directory during move: {e}"))?;
    } else {
        copy_file(src, dest).map_err(|e| format!("failed to copy file during move: {e}"))?;
    }
    remove_path(src).map_err(|e| format!("failed to remove source after move: {e}"))
}

/// `PathExists`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathInfo {
    NotFound,
    File,
    Directory,
    Symlink,
    Other,
}

pub fn path_exists(p: &str) -> PathInfo {
    match gofs::lstat(p) {
        Err(_) => PathInfo::NotFound,
        Ok(md) if md.is_file() => PathInfo::File,
        Ok(md) if md.is_dir() => PathInfo::Directory,
        Ok(md) if md.file_type().is_symlink() => PathInfo::Symlink,
        Ok(_) => PathInfo::Other,
    }
}

/// `/dev/null` opened as os/exec opens it for a child's stdin or stdout.
pub fn open_dev_null(write: bool) -> Result<(), String> {
    let r = if write {
        gofs::open_write("/dev/null")
    } else {
        gofs::open("/dev/null")
    };
    r.map(|_| ()).map_err(|e| path_err("open", "/dev/null", &e))
}
