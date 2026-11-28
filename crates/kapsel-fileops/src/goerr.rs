//! Error texts as TinyGo's wasip2 `os` package prints them.
//!
//! TinyGo turns ENOENT into `fs.ErrNotExist` ("file does not exist") and
//! EEXIST into `fs.ErrExist`; every other errno prints as "errno N" with
//! WASI's numbering. A path outside every preopen prints as "errno 2".

use std::io;

const EACCES: i32 = 2;
const EEXIST: i32 = 20;
const ENOENT: i32 = 44;

/// The WASI errno an I/O error stands for.
pub fn errno(e: &io::Error) -> i32 {
    #[cfg(target_os = "wasi")]
    if let Some(n) = e.raw_os_error() {
        return n;
    }
    use io::ErrorKind as K;
    match e.kind() {
        K::NotFound => ENOENT,
        K::AlreadyExists => EEXIST,
        K::PermissionDenied => EACCES,
        K::NotADirectory => 54,
        K::IsADirectory => 31,
        K::DirectoryNotEmpty => 55,
        K::InvalidInput => 28,
        K::Unsupported => 58,
        K::CrossesDevices => 75,
        // Rust's WASI std reports a path no preopen covers without an errno.
        _ if e.to_string().contains("pre-opened") => EACCES,
        _ => 29, // EIO
    }
}

/// What TinyGo prints for an errno.
pub fn errno_text(e: &io::Error) -> String {
    match errno(e) {
        ENOENT => "file does not exist".to_string(),
        EEXIST => "file already exists".to_string(),
        n => format!("errno {n}"),
    }
}

/// A Go `*PathError`: "op path: err".
pub fn path_err(op: &str, path: &str, e: &io::Error) -> String {
    format!("{op} {path}: {}", errno_text_at(path, e))
}

/// [`errno_text`] for an error on `path`. TinyGo reports a path that no
/// preopen covers as EACCES ("errno 2"), Rust's WASI std as ENOENT.
pub fn errno_text_at(path: &str, e: &io::Error) -> String {
    if errno(e) == ENOENT && !covered(path) {
        return format!("errno {EACCES}");
    }
    errno_text(e)
}

/// Whether TinyGo finds a preopen for `path`. It takes the first preopen
/// whose name is a string prefix of the path (so "/w" covers "/wx/a").
/// Some string prefix of the path can be looked up exactly when one is.
/// The empty path has none. A relative path is resolved against the `.`
/// preopen here (REQ-KS-017), so it counts as covered.
fn covered(path: &str) -> bool {
    if path.is_empty() {
        return false;
    }
    if !path.starts_with('/') {
        return true;
    }
    path.char_indices()
        .map(|(i, c)| &path[..i + c.len_utf8()])
        .any(|prefix| std::fs::symlink_metadata(prefix).is_ok())
}

pub fn is_not_exist(e: &io::Error) -> bool {
    errno(e) == ENOENT
}
