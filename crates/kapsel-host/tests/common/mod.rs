#![allow(dead_code)]

use std::io::Cursor;
use std::path::PathBuf;

use kapsel_host::{GrantSet, Session, SharedBuf};
use tempfile::TempDir;

pub struct T {
    pub s: Session,
    pub dir: TempDir,
    pub out: SharedBuf,
    pub err: SharedBuf,
}

impl T {
    pub fn new() -> T {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let grants = GrantSet { read_write: vec![root.clone()], read_only: vec![] };
        let s = Session::with_env(grants, root.clone(), vec![("HOME".to_string(), root.display().to_string())]).unwrap();
        let mut t = T { s, dir, out: SharedBuf::new(), err: SharedBuf::new() };
        t.s.set_io(Box::new(Cursor::new(Vec::new())), t.out.clone(), t.err.clone());
        t
    }

    pub fn root(&self) -> PathBuf {
        self.dir.path().canonicalize().unwrap()
    }

    pub fn path(&self, rel: &str) -> PathBuf {
        self.root().join(rel)
    }

    pub fn write(&self, rel: &str, content: &str) {
        let p = self.path(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, content).unwrap();
    }

    pub fn read(&self, rel: &str) -> String {
        std::fs::read_to_string(self.path(rel)).unwrap()
    }

    pub fn exists(&self, rel: &str) -> bool {
        std::fs::symlink_metadata(self.path(rel)).is_ok()
    }

    /// Runs a built-in with stdin, returns (status, stdout, stderr).
    pub fn run_in(&mut self, argv: &[&str], stdin: &str) -> (u32, String, String) {
        self.out.clear();
        self.err.clear();
        self.s.set_io(Box::new(Cursor::new(stdin.as_bytes().to_vec())), self.out.clone(), self.err.clone());
        let st = self.s.call_direct(argv);
        (st, self.out.string(), self.err.string())
    }

    pub fn run(&mut self, argv: &[&str]) -> (u32, String, String) {
        self.run_in(argv, "")
    }

    /// Asserts status, stdout and stderr exactly.
    #[track_caller]
    pub fn check(&mut self, argv: &[&str], stdin: &str, status: u32, out: &str, err: &str) {
        let (s, o, e) = self.run_in(argv, stdin);
        assert_eq!((s, o.as_str(), e.as_str()), (status, out, err), "argv {argv:?} stdin {stdin:?}");
    }
}
