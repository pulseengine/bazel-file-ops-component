//! File system calls as TinyGo's wasip2 `os` package makes them.
//!
//! Two differences from a POSIX `os` decide results, so every call goes
//! through here:
//! - TinyGo cleans a path lexically before the call (`a/../b` is `b` even
//!   when `a` does not exist), keeping a trailing slash;
//! - `os.Stat` does not follow a final symlink: it reports the link itself,
//!   as `os.Lstat` does.
//!
//! Opening a file follows symlinks, as everywhere.

use std::fs::{self, File, Metadata, OpenOptions};
use std::io;

use crate::gopath::clean;

/// The path TinyGo hands to WASI for `p`.
pub fn sys(p: &str) -> String {
    if p.is_empty() {
        return String::new();
    }
    let c = clean(p);
    if p.ends_with('/') && c != "/" {
        format!("{c}/")
    } else {
        c
    }
}

/// `os.Stat` (TinyGo: does not follow a final symlink).
pub fn stat(p: &str) -> io::Result<Metadata> {
    fs::symlink_metadata(sys(p))
}

/// `os.Lstat`.
pub fn lstat(p: &str) -> io::Result<Metadata> {
    fs::symlink_metadata(sys(p))
}

pub fn open(p: &str) -> io::Result<File> {
    File::open(sys(p))
}

/// `os.OpenFile(p, O_WRONLY|O_CREATE|O_TRUNC, …)`.
pub fn create_trunc(p: &str) -> io::Result<File> {
    OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(sys(p))
}

/// `os.OpenFile(p, O_APPEND|O_CREATE|O_WRONLY, …)`.
pub fn open_append(p: &str) -> io::Result<File> {
    OpenOptions::new().append(true).create(true).open(sys(p))
}

pub fn open_write(p: &str) -> io::Result<File> {
    OpenOptions::new().write(true).open(sys(p))
}

pub fn mkdir(p: &str) -> io::Result<()> {
    fs::create_dir(sys(p))
}

pub fn read_dir(p: &str) -> io::Result<fs::ReadDir> {
    fs::read_dir(sys(p))
}

pub fn rename(from: &str, to: &str) -> io::Result<()> {
    fs::rename(sys(from), sys(to))
}

pub fn remove_dir_all(p: &str) -> io::Result<()> {
    fs::remove_dir_all(sys(p))
}

pub fn remove_file(p: &str) -> io::Result<()> {
    fs::remove_file(sys(p))
}

pub fn read_link(p: &str) -> io::Result<String> {
    fs::read_link(sys(p)).map(|t| t.to_string_lossy().into_owned())
}
