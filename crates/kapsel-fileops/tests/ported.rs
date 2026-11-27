//! The behaviour tests of `tinygo/*_test.go`, ported. They run natively
//! against the library; the equivalence test runs the same rules through
//! the wasm component.

use std::fs;
use std::os::unix::fs::symlink;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use kapsel_config::{FileSpec, JsonConfig, Operation, WorkspaceConfig};
use kapsel_fileops::ops::{self, PathInfo, path_exists};
use kapsel_fileops::security::is_within;
use kapsel_fileops::workspace::prepare_workspace;
use kapsel_fileops::{bridge, process_json_config};

/// A fresh directory per test, with symlinks resolved.
fn temp_dir() -> String {
    static N: AtomicUsize = AtomicUsize::new(0);
    let base = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("ported");
    let d = base.join(format!(
        "{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d.canonicalize().unwrap().display().to_string()
}

fn p(dir: &str, rel: &str) -> String {
    format!("{dir}/{rel}")
}

fn write(path: &str, content: &str) {
    fs::create_dir_all(PathBuf::from(path).parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
}

fn read(path: &str) -> String {
    fs::read_to_string(path).unwrap()
}

fn op(t: &str) -> Operation {
    Operation {
        op_type: t.into(),
        ..Default::default()
    }
}

fn run_config(ws: &str, ops: Vec<Operation>) -> Result<bridge::WorkspaceInfo, String> {
    let json = serde_json::to_string(&JsonConfig {
        workspace_dir: ws.into(),
        operations: ops,
    })
    .unwrap();
    process_json_config(json.as_bytes())
}

// --- operations_test.go ------------------------------------------------------

#[test]
fn copy_file() {
    let t = temp_dir();
    write(&p(&t, "source.txt"), "Hello, World!");
    ops::copy_file(&p(&t, "source.txt"), &p(&t, "dest.txt")).unwrap();
    assert_eq!(read(&p(&t, "dest.txt")), "Hello, World!");
}

#[test]
fn copy_file_to_subdirectory() {
    let t = temp_dir();
    write(&p(&t, "source.txt"), "test content");
    ops::copy_file(&p(&t, "source.txt"), &p(&t, "subdir/dest.txt")).unwrap();
    assert_eq!(path_exists(&p(&t, "subdir/dest.txt")), PathInfo::File);
}

#[test]
fn create_directory_and_nested() {
    let t = temp_dir();
    ops::create_directory(&p(&t, "testdir")).unwrap();
    assert_eq!(path_exists(&p(&t, "testdir")), PathInfo::Directory);
    ops::create_directory(&p(&t, "level1/level2/level3")).unwrap();
    assert_eq!(
        path_exists(&p(&t, "level1/level2/level3")),
        PathInfo::Directory
    );
}

#[test]
fn copy_directory() {
    let t = temp_dir();
    let files = [
        ("file1.txt", "content1"),
        ("file2.txt", "content2"),
        ("subdir/file3.txt", "content3"),
    ];
    for (f, c) in files {
        write(&p(&t, &format!("source/{f}")), c);
    }
    ops::copy_directory(&p(&t, "source"), &p(&t, "dest")).unwrap();
    for (f, c) in files {
        assert_eq!(read(&p(&t, &format!("dest/{f}"))), c);
    }
}

#[test]
fn path_exists_kinds() {
    let t = temp_dir();
    assert_eq!(path_exists(&p(&t, "nonexistent")), PathInfo::NotFound);
    write(&p(&t, "testfile.txt"), "test");
    assert_eq!(path_exists(&p(&t, "testfile.txt")), PathInfo::File);
    fs::create_dir(p(&t, "testdir")).unwrap();
    assert_eq!(path_exists(&p(&t, "testdir")), PathInfo::Directory);
    symlink("testdir", p(&t, "link")).unwrap();
    assert_eq!(path_exists(&p(&t, "link")), PathInfo::Symlink);
}

#[test]
fn list_directory() {
    let t = temp_dir();
    for f in ["file1.txt", "file2.log", "file3.txt"] {
        write(&p(&t, f), "test");
    }
    fs::create_dir(p(&t, "subdir")).unwrap();
    assert_eq!(
        ops::list_directory(&t).unwrap(),
        ["file1.txt", "file2.log", "file3.txt", "subdir"]
    );
}

#[test]
fn remove_path() {
    let t = temp_dir();
    write(&p(&t, "testfile.txt"), "test");
    ops::remove_path(&p(&t, "testfile.txt")).unwrap();
    assert_eq!(path_exists(&p(&t, "testfile.txt")), PathInfo::NotFound);
    ops::remove_path(&p(&t, "testfile.txt")).unwrap();
}

#[test]
fn read_file() {
    let t = temp_dir();
    write(&p(&t, "test.txt"), "Hello, World!\nLine 2\nLine 3");
    assert_eq!(
        ops::read_file(&p(&t, "test.txt")).unwrap(),
        b"Hello, World!\nLine 2\nLine 3"
    );
    assert!(ops::read_file(&p(&t, "nonexistent.txt")).is_err());
}

#[test]
fn write_file_overwrites_and_creates_parent() {
    let t = temp_dir();
    ops::write_file(&p(&t, "output.txt"), b"Test content\nSecond line").unwrap();
    assert_eq!(read(&p(&t, "output.txt")), "Test content\nSecond line");
    ops::write_file(&p(&t, "output.txt"), b"Overwritten content").unwrap();
    assert_eq!(read(&p(&t, "output.txt")), "Overwritten content");
    ops::write_file(&p(&t, "subdir/output.txt"), b"Test content").unwrap();
    assert_eq!(path_exists(&p(&t, "subdir/output.txt")), PathInfo::File);
}

#[test]
fn append_to_file() {
    let t = temp_dir();
    ops::append_to_file(&p(&t, "append.txt"), b"First line\n").unwrap();
    assert_eq!(read(&p(&t, "append.txt")), "First line\n");
    ops::append_to_file(&p(&t, "append.txt"), b"Second line\n").unwrap();
    assert_eq!(read(&p(&t, "append.txt")), "First line\nSecond line\n");
}

#[test]
fn concatenate_files() {
    let t = temp_dir();
    let srcs: Vec<String> = (1..=3).map(|i| p(&t, &format!("file{i}.txt"))).collect();
    for (i, s) in srcs.iter().enumerate() {
        write(s, &format!("Content from file {}\n", i + 1));
    }
    ops::concatenate_files(&srcs, &p(&t, "concatenated.txt")).unwrap();
    assert_eq!(
        read(&p(&t, "concatenated.txt")),
        "Content from file 1\nContent from file 2\nContent from file 3\n"
    );
    assert!(ops::concatenate_files(&[], &p(&t, "output.txt")).is_err());
}

#[test]
fn move_path_file_and_directory() {
    let t = temp_dir();
    write(&p(&t, "source.txt"), "Test content for move");
    ops::move_path(&p(&t, "source.txt"), &p(&t, "destination.txt")).unwrap();
    assert_eq!(path_exists(&p(&t, "source.txt")), PathInfo::NotFound);
    assert_eq!(read(&p(&t, "destination.txt")), "Test content for move");

    write(&p(&t, "sourcedir/test.txt"), "test");
    ops::move_path(&p(&t, "sourcedir"), &p(&t, "destdir")).unwrap();
    assert_eq!(path_exists(&p(&t, "sourcedir")), PathInfo::NotFound);
    assert_eq!(path_exists(&p(&t, "destdir/test.txt")), PathInfo::File);
}

// --- json_bridge_test.go -----------------------------------------------------

#[test]
fn process_json_config_mkdir_and_copy() {
    let t = temp_dir();
    write(&p(&t, "src/main.cpp"), "int main() { return 0; }");
    let ws = p(&t, "workspace");
    let r = run_config(
        &ws,
        vec![
            Operation {
                path: "include".into(),
                ..op("mkdir")
            },
            Operation {
                src_path: p(&t, "src/main.cpp"),
                dest_path: "main.cpp".into(),
                ..op("copy_file")
            },
        ],
    )
    .unwrap();
    assert_eq!(r.workspace_path, ws);
    assert_eq!(r.prepared_files.len(), 2);
    assert_eq!(path_exists(&p(&ws, "include")), PathInfo::Directory);
    assert_eq!(read(&p(&ws, "main.cpp")), "int main() { return 0; }");
}

#[test]
fn json_copy_directory_contents() {
    let t = temp_dir();
    write(&p(&t, "headers/header1.h"), "#pragma once\n// Header 1");
    write(&p(&t, "headers/header2.h"), "#pragma once\n// Header 2");
    let ws = p(&t, "workspace");
    let r = run_config(
        &ws,
        vec![Operation {
            src_path: p(&t, "headers"),
            dest_path: "include".into(),
            ..op("copy_directory_contents")
        }],
    )
    .unwrap();
    assert_eq!(
        read(&p(&ws, "include/header1.h")),
        "#pragma once\n// Header 1"
    );
    assert_eq!(
        read(&p(&ws, "include/header2.h")),
        "#pragma once\n// Header 2"
    );
    assert_eq!(r.prepared_files.len(), 2);
}

/// TestJsonConfigRunCommand expected echo to run. A WASI component cannot
/// start a process (the TinyGo component cannot either), so the run fails
/// after the output directory is made, with TinyGo's error.
#[test]
fn json_run_command_cannot_spawn() {
    let t = temp_dir();
    let ws = p(&t, "workspace");
    let err = run_config(
        &ws,
        vec![Operation {
            command: "echo".into(),
            args: vec!["Hello, World!".into()],
            output_file: "output.txt".into(),
            ..op("run_command")
        }],
    )
    .unwrap_err();
    assert_eq!(
        err,
        "operation 0 failed: command failed: exec: \"echo\": executable file not found in $PATH"
    );
    assert_eq!(path_exists(&p(&ws, "output.txt")), PathInfo::NotFound);
}

// --- confinement_test.go -----------------------------------------------------

#[test]
fn is_within_cases() {
    let t = temp_dir();
    let root = p(&t, "work");
    fs::create_dir_all(p(&root, "sub")).unwrap();
    fs::create_dir_all(p(&t, "outside")).unwrap();
    symlink(p(&t, "outside"), p(&root, "link")).unwrap();
    symlink(p(&t, "missing"), p(&root, "dangling")).unwrap();
    let cases = [
        (root.clone(), true),
        (p(&root, "sub"), true),
        (p(&root, "sub/new/file.txt"), true),
        (p(&root, "a..b"), true),
        (p(&t, "work-other"), false),
        (p(&t, "work-other/file.txt"), false),
        (t.clone(), false),
        (p(&root, "link/file.txt"), false),
        (p(&root, "dangling"), false),
        (p(&root, "dangling/file.txt"), false),
    ];
    for (path, want) in cases {
        assert_eq!(is_within(&root, &path), want, "is_within({root}, {path})");
    }
}

#[test]
fn copy_file_rejects_relative_traversal_source() {
    let t = temp_dir();
    let dest = p(&t, "dest.txt");
    assert!(ops::copy_file("../outside.txt", &dest).is_err());
    assert_eq!(path_exists(&dest), PathInfo::NotFound);
}

#[test]
fn json_destinations_stay_in_workspace() {
    let t = temp_dir();
    let src = p(&t, "src.txt");
    write(&src, "data");
    let esc = "../escape.txt".to_string();
    let cases = [
        Operation {
            path: esc.clone(),
            content: "x".into(),
            ..op("write_file")
        },
        Operation {
            path: esc.clone(),
            content: "x".into(),
            ..op("append_to_file")
        },
        Operation {
            path: esc.clone(),
            ..op("mkdir")
        },
        Operation {
            src_path: src.clone(),
            dest_path: esc.clone(),
            ..op("copy_file")
        },
        Operation {
            sources: vec![src.clone()],
            dest_path: esc.clone(),
            ..op("concatenate_files")
        },
        Operation {
            src_path: src.clone(),
            dest_path: esc.clone(),
            ..op("move_path")
        },
    ];
    for o in cases {
        let name = o.op_type.clone();
        let ws = p(&t, &format!("ws-{name}"));
        assert!(
            run_config(&ws, vec![o]).is_err(),
            "{name} with dest outside the workspace succeeded"
        );
        assert_eq!(
            path_exists(&p(&t, "escape.txt")),
            PathInfo::NotFound,
            "{name} wrote outside"
        );
        assert_eq!(
            path_exists(&src),
            PathInfo::File,
            "{name} removed the source"
        );
    }
}

#[test]
fn json_move_source_stays_in_workspace() {
    let t = temp_dir();
    let ws = p(&t, "ws");
    let outside = p(&t, "outside.txt");
    write(&outside, "keep");
    assert!(
        run_config(
            &ws,
            vec![Operation {
                src_path: outside.clone(),
                dest_path: "moved.txt".into(),
                ..op("move_path")
            }]
        )
        .is_err()
    );
    assert_eq!(path_exists(&outside), PathInfo::File);

    let inside = p(&ws, "in.txt");
    run_config(
        &ws,
        vec![
            Operation {
                path: "in.txt".into(),
                content: "x".into(),
                ..op("write_file")
            },
            Operation {
                src_path: inside.clone(),
                dest_path: "moved.txt".into(),
                ..op("move_path")
            },
        ],
    )
    .unwrap();
    assert_eq!(path_exists(&p(&ws, "moved.txt")), PathInfo::File);
    assert_eq!(path_exists(&inside), PathInfo::NotFound);
}

#[test]
fn json_destination_through_symlink_rejected() {
    let t = temp_dir();
    let ws = p(&t, "ws");
    fs::create_dir_all(&ws).unwrap();
    fs::create_dir_all(p(&t, "outside")).unwrap();
    symlink(p(&t, "outside"), p(&ws, "link")).unwrap();
    assert!(
        run_config(
            &ws,
            vec![Operation {
                path: "link/f.txt".into(),
                content: "x".into(),
                ..op("write_file")
            }]
        )
        .is_err()
    );
    assert_eq!(path_exists(&p(&t, "outside/f.txt")), PathInfo::NotFound);
}

/// TestJsonRunCommandBounds: the bounds are checked before the command
/// would start; in bounds, the run fails only because it cannot spawn.
#[test]
fn json_run_command_bounds() {
    let t = temp_dir();
    let ws = p(&t, "out/ws");
    let echo = |extra: Operation| Operation {
        command: "echo".into(),
        args: vec!["x".into()],
        ..extra
    };

    let err = run_config(
        &ws,
        vec![echo(Operation {
            work_dir: ".".into(),
            output_file: "../bindings.txt".into(),
            ..op("run_command")
        })],
    )
    .unwrap_err();
    assert!(
        err.contains("command failed"),
        "output_file in the workspace parent refused: {err}"
    );

    let err = run_config(
        &ws,
        vec![echo(Operation {
            output_file: "../../escape.txt".into(),
            ..op("run_command")
        })],
    )
    .unwrap_err();
    assert!(
        err.contains("output_file") && err.contains("resolves outside"),
        "{err}"
    );
    assert_eq!(path_exists(&p(&t, "escape.txt")), PathInfo::NotFound);

    let err = run_config(
        &ws,
        vec![echo(Operation {
            work_dir: t.clone(),
            ..op("run_command")
        })],
    )
    .unwrap_err();
    assert!(
        err.contains("work_dir") && err.contains("resolves outside"),
        "{err}"
    );
}

#[test]
fn prepare_workspace_destination_stays_in_work_dir() {
    let t = temp_dir();
    let src = p(&t, "a.h");
    write(&src, "x");
    let cfg = WorkspaceConfig {
        work_dir: p(&t, "ws"),
        sources: vec![FileSpec {
            source: src,
            destination: Some("../../escape.h".into()),
            ..Default::default()
        }],
        ..Default::default()
    };
    assert!(prepare_workspace(&cfg).is_err());
    let parent = PathBuf::from(&t).parent().unwrap().join("escape.h");
    assert!(!parent.exists());
}

#[test]
fn move_source_symlink_dot_dot_refused() {
    let t = temp_dir();
    let ws = p(&t, "ws");
    fs::create_dir_all(&ws).unwrap();
    fs::create_dir_all(p(&t, "outside/deep")).unwrap();
    write(&p(&t, "outside/x"), "victim");
    symlink(p(&t, "outside/deep"), p(&ws, "link")).unwrap();
    let err = run_config(
        &ws,
        vec![Operation {
            src_path: format!("{ws}/link/../x"),
            dest_path: "stolen".into(),
            ..op("move_path")
        }],
    )
    .unwrap_err();
    assert!(err.contains("\"..\" components are not allowed"), "{err}");
    assert_eq!(path_exists(&p(&t, "outside/x")), PathInfo::File);
}

#[test]
fn dot_dot_in_work_dir_and_workspace_refused() {
    let t = temp_dir();
    let ws = p(&t, "out/ws");
    assert!(
        run_config(
            &ws,
            vec![Operation {
                command: "echo".into(),
                work_dir: format!("{ws}/../ws"),
                ..op("run_command")
            }]
        )
        .unwrap_err()
        .contains("\"..\" components are not allowed")
    );
    assert!(
        run_config(
            &format!("{t}/a/../ws2"),
            vec![Operation {
                path: "d".into(),
                ..op("mkdir")
            }]
        )
        .is_err()
    );
}

#[test]
fn copy_into_existing_symlink_refused() {
    let t = temp_dir();
    let ws = p(&t, "ws");
    fs::create_dir_all(&ws).unwrap();
    fs::create_dir_all(p(&t, "outside")).unwrap();
    write(&p(&t, "src/sub/evil.txt"), "x");
    symlink(p(&t, "outside"), p(&ws, "sub")).unwrap();
    assert!(
        run_config(
            &ws,
            vec![Operation {
                src_path: p(&t, "src"),
                dest_path: ".".into(),
                ..op("copy_directory_contents")
            }]
        )
        .is_err()
    );
    assert_eq!(path_exists(&p(&t, "outside/evil.txt")), PathInfo::NotFound);
}

#[test]
fn write_through_existing_file_symlink_refused() {
    let t = temp_dir();
    let ws = p(&t, "ws");
    fs::create_dir_all(&ws).unwrap();
    let target = p(&t, "target.txt");
    write(&target, "keep");
    symlink(&target, p(&ws, "f.txt")).unwrap();
    for o in [
        Operation {
            path: "f.txt".into(),
            content: "x".into(),
            ..op("write_file")
        },
        Operation {
            path: "f.txt".into(),
            content: "x".into(),
            ..op("append_to_file")
        },
    ] {
        assert!(run_config(&ws, vec![o]).is_err());
    }
    // The rule on its own, with the link inside the workspace (Go:
    // SetupGoModule writing go.mod through a symlink).
    write(&p(&ws, "real.txt"), "keep");
    symlink("real.txt", p(&ws, "go.mod")).unwrap();
    assert_eq!(
        ops::write_file(&p(&ws, "go.mod"), b"module m\n").unwrap_err(),
        format!("refusing to write through symlink {ws}/go.mod")
    );
    assert_eq!(read(&target), "keep");
    assert_eq!(read(&p(&ws, "real.txt")), "keep");
}

// --- REQ-KS-017 --------------------------------------------------------------

#[test]
fn unknown_operation_fails_before_anything_runs() {
    let t = temp_dir();
    let ws = p(&t, "ws");
    let err = run_config(
        &ws,
        vec![
            Operation {
                path: "a".into(),
                ..op("mkdir")
            },
            op("bogus"),
        ],
    )
    .unwrap_err();
    assert_eq!(
        err,
        "invalid JSON config: operation 1: unknown operation type: bogus"
    );
    assert_eq!(path_exists(&ws), PathInfo::NotFound);
}
