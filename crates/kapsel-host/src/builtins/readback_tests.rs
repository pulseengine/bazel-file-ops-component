//! DD-KS-020: a structured edit that does not read back as the intended
//! document exits 1 and leaves the file byte-identical, with no temp file.
//! The mismatch is injected through `BREAK_READBACK`, which exists in test
//! builds only.

use std::io::Cursor;

use super::BREAK_READBACK;
use crate::{GrantSet, Session, SharedBuf};

struct Guard;

impl Guard {
    fn on() -> Guard {
        BREAK_READBACK.set(true);
        Guard
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        BREAK_READBACK.set(false);
    }
}

fn names(dir: &std::path::Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    v.sort();
    v
}

/// `(status, stderr)` of `argv` run in `root`.
fn run(root: &std::path::Path, argv: &[&str]) -> (u32, String) {
    let grants = GrantSet {
        read_write: vec![root.to_path_buf()],
        read_only: vec![],
        ..Default::default()
    };
    let mut s = Session::with_env(grants, root.to_path_buf(), vec![]).unwrap();
    let err = SharedBuf::new();
    s.set_io(
        Box::new(Cursor::new(Vec::new())),
        SharedBuf::new(),
        err.clone(),
    );
    let st = s.call_direct(argv);
    (st, err.string())
}

/// Each case: file name, content, `argv` without the file name, the file
/// content after the edit succeeds.
const CASES: &[(&str, &str, &[&str], &str)] = &[
    (
        "f.yaml",
        "# keep\na: 1  # note\nb:\n  c: x\n",
        &["yaml", "set", ".a", "2"],
        "# keep\na: 2  # note\nb:\n  c: x\n",
    ),
    (
        "f.toml",
        "# keep\n[p]\na = 1 # note\n",
        &["toml", "set", ".p.a", "2"],
        "# keep\n[p]\na = 2 # note\n",
    ),
    (
        "f.json",
        "{\"a\": 1, \"b\": [1,  2]}\n",
        &["json", "set", ".a", "2"],
        "{\"a\": 2, \"b\": [1,  2]}\n",
    ),
];

#[test]
fn a_read_back_mismatch_exits_1_and_leaves_the_file_and_directory_alone() {
    for (file, before, argv, after) in CASES {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        std::fs::write(root.join(file), before).unwrap();
        let mut full: Vec<&str> = argv.to_vec();
        full.push(file);

        // Control: without the fault the same command succeeds and edits.
        let (st, err) = run(&root, &full);
        assert_eq!((st, err.as_str()), (0, ""), "{file}: control");
        assert_eq!(std::fs::read_to_string(root.join(file)).unwrap(), *after);
        std::fs::write(root.join(file), before).unwrap();

        let _fault = Guard::on();
        let (st, err) = run(&root, &full);
        assert_eq!(st, 1, "{file}: {err}");
        assert!(
            err.contains("did not read back") && err.contains("unchanged"),
            "{file}: {err}"
        );
        assert_eq!(
            std::fs::read(root.join(file)).unwrap(),
            before.as_bytes(),
            "{file}"
        );
        assert_eq!(names(&root), vec![file.to_string()], "{file}: temp left");
    }
}
