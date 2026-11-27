//! Grant tests, fail closed (REQ-KS-022). Each planted bypass has a control.
//! Layout: <tmp>/work (read-write, cwd), <tmp>/ro (read-only), <tmp>/out (no grant).

mod common;

use std::io::Cursor;
use std::os::unix::fs::symlink;
use std::path::PathBuf;

use kapsel_host::{GrantSet, Session, SharedBuf, TPart};

struct G {
    s: Session,
    base: tempfile::TempDir,
    out: SharedBuf,
    err: SharedBuf,
}

impl G {
    fn new() -> G {
        let base = tempfile::tempdir().unwrap();
        let root = base.path().canonicalize().unwrap();
        for d in ["work", "ro", "out"] {
            std::fs::create_dir(root.join(d)).unwrap();
        }
        std::fs::write(root.join("out/secret"), "secret\n").unwrap();
        std::fs::write(root.join("ro/file"), "readable\n").unwrap();
        let grants = GrantSet { read_write: vec![root.join("work")], read_only: vec![root.join("ro")] };
        let s = Session::with_env(grants, root.join("work"), vec![]).unwrap();
        G { s, base, out: SharedBuf::new(), err: SharedBuf::new() }
    }
    fn p(&self, rel: &str) -> PathBuf {
        self.base.path().canonicalize().unwrap().join(rel)
    }
    fn abs(&self, rel: &str) -> String {
        self.p(rel).display().to_string()
    }
    fn run(&mut self, argv: &[&str]) -> (u32, String, String) {
        self.out.clear();
        self.err.clear();
        self.s.set_io(Box::new(Cursor::new(Vec::new())), self.out.clone(), self.err.clone());
        let st = self.s.call_direct(argv);
        (st, self.out.string(), self.err.string())
    }
    /// Runs `argv` with a redirect.
    fn run_redir(&mut self, argv: &[&str], op: &str, target: &str) -> (u32, String, String) {
        self.out.clear();
        self.err.clear();
        self.s.set_io(Box::new(Cursor::new(Vec::new())), self.out.clone(), self.err.clone());
        let words: Vec<Vec<TPart>> = argv.iter().map(|a| vec![TPart::Lit(a, true)]).collect();
        let fd = if op == "read" { 0 } else { 1 };
        let st = self.s.t_call(&words, &[(fd, op, vec![TPart::Lit(target, true)], 0)], 0, 0);
        (st, self.out.string(), self.err.string())
    }
}

fn outside(p: &str) -> String {
    format!("kapsel: {p}: outside the session's grants\n")
}

#[test]
fn reads_inside_and_read_only_grants_work() {
    let mut g = G::new();
    std::fs::write(g.p("work/mine"), "mine\n").unwrap();
    assert_eq!(g.run(&["cat", "mine"]), (0, "mine\n".into(), "".into()));
    let ro = g.abs("ro/file");
    assert_eq!(g.run(&["cat", &ro]), (0, "readable\n".into(), "".into()));
}

#[test]
fn outside_paths_are_refused_and_untouched() {
    let mut g = G::new();
    let secret = g.abs("out/secret");
    assert_eq!(g.run(&["cat", &secret]), (1, "".into(), outside(&secret)));
    assert_eq!(g.run(&["rm", &secret]), (1, "".into(), outside(&secret)));
    assert!(g.p("out/secret").exists());
    let newf = g.abs("out/new");
    assert_eq!(g.run(&["mkdir", &newf]), (1, "".into(), outside(&newf)));
    assert!(!g.p("out/new").exists());
    let (st, _, err) = g.run_redir(&["echo", "x"], "write", &newf);
    assert_eq!((st, err), (1, outside(&newf)));
    assert!(!g.p("out/new").exists());
    assert_eq!(g.run(&["cp", &secret, "copy"]), (1, "".into(), outside(&secret)));
    assert!(!g.p("work/copy").exists());
    std::fs::write(g.p("work/mine"), "m").unwrap();
    assert_eq!(g.run(&["cp", "mine", &newf]).0, 1);
    assert_eq!(g.run(&["mv", "mine", &newf]).0, 1);
    assert!(g.p("work/mine").exists() && !g.p("out/new").exists());
    assert_eq!(g.run(&["tee", &newf]).0, 1);
    assert_eq!(g.run(&["sed", "-i", "s/s/X/", &secret]).0, 1);
    assert_eq!(std::fs::read_to_string(g.p("out/secret")).unwrap(), "secret\n");
}

#[test]
fn read_only_grant_cannot_be_written() {
    let mut g = G::new();
    let f = g.abs("ro/file");
    let n = g.abs("ro/new");
    assert_eq!(g.run(&["rm", &f]), (1, "".into(), outside(&f)));
    assert_eq!(g.run(&["tee", &n]).0, 1);
    let (st, _, err) = g.run_redir(&["echo", "x"], "append", &f);
    assert_eq!((st, err), (1, outside(&f)));
    assert_eq!(std::fs::read_to_string(g.p("ro/file")).unwrap(), "readable\n");
    assert!(!g.p("ro/new").exists());
}

#[test]
fn symlink_inside_rw_pointing_outside_is_refused() {
    let mut g = G::new();
    symlink(g.p("out/secret"), g.p("work/link")).unwrap();
    // read through the link: refused (target is outside)
    assert_eq!(g.run(&["cat", "link"]), (1, "".into(), outside("link")));
    // write through the link: refused, secret untouched
    let (st, _, err) = g.run_redir(&["echo", "pwned"], "write", "link");
    assert_eq!(st, 1);
    assert_eq!(err, outside("link"));
    assert_eq!(std::fs::read_to_string(g.p("out/secret")).unwrap(), "secret\n");
    assert_eq!(g.run(&["tee", "link"]).0, 1);
    std::fs::write(g.p("work/f"), "f").unwrap();
    assert_eq!(g.run(&["cp", "f", "link"]).0, 1);
    assert_eq!(g.run(&["sed", "-i", "s/s/X/", "link"]).0, 1);
    assert_eq!(std::fs::read_to_string(g.p("out/secret")).unwrap(), "secret\n");
    // control: rm acts on the link itself and leaves the target alone
    assert_eq!(g.run(&["rm", "link"]), (0, "".into(), "".into()));
    assert!(!g.p("work/link").exists() && g.p("out/secret").exists());
}

#[test]
fn symlinked_directory_pointing_outside_is_refused() {
    let mut g = G::new();
    symlink(g.p("out"), g.p("work/linkdir")).unwrap();
    assert_eq!(g.run(&["cat", "linkdir/secret"]), (1, "".into(), outside("linkdir/secret")));
    assert_eq!(g.run(&["mkdir", "linkdir/x"]), (1, "".into(), outside("linkdir/x")));
    assert_eq!(g.run(&["mkdir", "-p", "linkdir/x/y"]).0, 1);
    assert!(!g.p("out/x").exists());
    assert_eq!(g.run(&["ls", "linkdir"]).0, 2);
    assert_eq!(g.run(&["cd", "linkdir"]), (1, "".into(), outside("linkdir")));
    assert_eq!(g.run(&["rm", "linkdir/secret"]).0, 1);
    assert!(g.p("out/secret").exists());
}

#[test]
fn dangling_symlink_to_outside_is_not_a_way_to_create_files() {
    let mut g = G::new();
    symlink(g.p("out/new"), g.p("work/dangling")).unwrap();
    let (st, _, err) = g.run_redir(&["echo", "x"], "write", "dangling");
    assert_eq!((st, err.as_str()), (1, outside("dangling").as_str()));
    assert_eq!(g.run(&["mkdir", "dangling"]).0, 1);
    assert_eq!(g.run(&["tee", "dangling"]).0, 1);
    assert!(!g.p("out/new").exists());
}

#[test]
fn symlink_inside_rw_pointing_inside_is_refused_for_write_but_readable() {
    // TinyGo rule (refuseSymlink): never write through an existing symlink.
    let mut g = G::new();
    std::fs::write(g.p("work/real"), "real\n").unwrap();
    symlink(g.p("work/real"), g.p("work/alias")).unwrap();
    assert_eq!(g.run(&["cat", "alias"]), (0, "real\n".into(), "".into()));
    let (st, _, err) = g.run_redir(&["echo", "x"], "write", "alias");
    assert_eq!((st, err.as_str()), (1, "kapsel: alias: refusing to write through symlink\n"));
    assert_eq!(std::fs::read_to_string(g.p("work/real")).unwrap(), "real\n");
    // control: a plain file is writable
    let (st, _, err) = g.run_redir(&["echo", "x"], "write", "real");
    assert_eq!((st, err.as_str()), (0, ""));
    assert_eq!(std::fs::read_to_string(g.p("work/real")).unwrap(), "x\n");
}

// ---- planted: copy_tree and redirects through a link to /outside ----------

#[test]
fn cp_r_and_redirect_through_link_to_outside_dir_are_refused() {
    let mut g = G::new();
    symlink(g.p("out"), g.p("work/link")).unwrap();
    std::fs::create_dir_all(g.p("work/src/sub")).unwrap();
    std::fs::write(g.p("work/src/sub/a"), "a").unwrap();
    assert_eq!(g.run(&["cp", "-r", "src", "link/x"]).0, 1);
    assert!(!g.p("out/x").exists());
    let (st, _, _) = g.run_redir(&["echo", "x"], "write", "link/f");
    assert_eq!(st, 1);
    assert!(!g.p("out/f").exists());
    // control: the same copy into a real directory works
    std::fs::create_dir(g.p("work/real")).unwrap();
    assert_eq!(g.run(&["cp", "-r", "src", "real/x"]), (0, "".into(), "".into()));
    assert_eq!(std::fs::read_to_string(g.p("work/real/x/sub/a")).unwrap(), "a");
    let (st, _, _) = g.run_redir(&["echo", "x"], "write", "real/f");
    assert_eq!(st, 0);
}

#[test]
fn cp_r_does_not_write_through_a_link_inside_the_destination_tree() {
    // dest/s is a link to the outside; the source has a directory s/ and a file t.
    let mut g = G::new();
    std::fs::create_dir_all(g.p("work/src/s")).unwrap();
    std::fs::write(g.p("work/src/s/payload"), "evil").unwrap();
    std::fs::write(g.p("work/src/t"), "t").unwrap();
    std::fs::create_dir_all(g.p("work/dest/src")).unwrap();
    symlink(g.p("out"), g.p("work/dest/src/s")).unwrap();
    g.run(&["cp", "-r", "src", "dest"]);
    assert!(!g.p("out/payload").exists(), "wrote through dest/s");
    // a file-level link as well
    std::fs::write(g.p("work/src/f"), "new").unwrap();
    symlink(g.p("out/secret"), g.p("work/dest/src/f")).unwrap();
    g.run(&["cp", "-r", "src", "dest"]);
    assert_eq!(std::fs::read_to_string(g.p("out/secret")).unwrap(), "secret\n");
}

#[test]
fn cp_r_and_mv_refuse_to_recreate_symlinks() {
    let mut g = G::new();
    // Planted: a relative link that is harmless where it is (resolves inside
    // work/) but would resolve to base/out/secret one level deeper, and the
    // reverse direction.
    std::fs::create_dir_all(g.p("work/a")).unwrap();
    std::fs::write(g.p("work/secret"), "inside\n").unwrap();
    symlink("../secret", g.p("work/a/l")).unwrap();
    std::fs::write(g.p("work/a/ok"), "ok").unwrap();
    std::fs::create_dir_all(g.p("work/deep/er")).unwrap();
    let (st, _, err) = g.run(&["cp", "-r", "a", "deep/er/a2"]);
    assert_eq!(st, 1);
    assert!(err.contains("refusing to copy a symlink"), "{err}");
    assert!(std::fs::symlink_metadata(g.p("work/deep/er/a2/l")).is_err());
    // shallower: a link that is fine deep down, copied up
    symlink("../../../out/secret", g.p("work/deep/er/m")).unwrap();
    std::fs::create_dir_all(g.p("work/d2")).unwrap();
    let (st, _, err) = g.run(&["cp", "-r", "deep", "d2/x"]);
    assert_eq!(st, 1);
    assert!(err.contains("refusing to copy a symlink"), "{err}");
    // mv of a link to another directory is refused; in place is fine
    let (st, _, err) = g.run(&["mv", "a/l", "deep"]);
    assert_eq!((st, err.as_str()), (1, "kapsel: a/l: refusing to copy a symlink\n"));
    assert!(std::fs::symlink_metadata(g.p("work/a/l")).is_ok());
    assert_eq!(g.run(&["mv", "a/l", "a/l2"]), (0, "".into(), "".into()));
    // control: a tree without links copies
    std::fs::remove_file(g.p("work/a/l2")).unwrap();
    assert_eq!(g.run(&["cp", "-r", "a", "deep/er/a3"]), (0, "".into(), "".into()));
    assert_eq!(std::fs::read_to_string(g.p("work/deep/er/a3/ok")).unwrap(), "ok");
}

#[test]
fn rm_r_never_follows_a_symlink_into_its_target() {
    let mut g = G::new();
    std::fs::create_dir_all(g.p("work/tree")).unwrap();
    symlink(g.p("out"), g.p("work/tree/l")).unwrap();
    assert_eq!(g.run(&["rm", "-r", "tree"]), (0, "".into(), "".into()));
    assert!(!g.p("work/tree").exists());
    assert!(g.p("out/secret").exists(), "rm -r followed the link");
    // rm -r on the link itself removes the link only
    symlink(g.p("out"), g.p("work/l2")).unwrap();
    assert_eq!(g.run(&["rm", "-r", "l2"]), (0, "".into(), "".into()));
    assert!(g.p("out/secret").exists());
    // and a path through the link is refused
    symlink(g.p("out"), g.p("work/l3")).unwrap();
    assert_eq!(g.run(&["rm", "-r", "l3/secret"]).0, 1);
    assert!(g.p("out/secret").exists());
}

// ---- `..` and loops ---------------------------------------------------------

#[test]
fn dotdot_is_resolved_physically_after_symlinks() {
    let mut g = G::new();
    std::fs::create_dir(g.p("work/sub")).unwrap();
    // `..` out of the grant is refused by the grant check on the result
    assert_eq!(g.run(&["cat", "../out/secret"]), (1, "".into(), outside("../out/secret")));
    assert_eq!(g.run(&["cat", "sub/../../out/secret"]).0, 1);
    // Planted: link -> <outside dir>. Lexically `link/../secret` is
    // work/secret (inside); the kernel goes to the parent of the target,
    // <base>/secret... here out/.. is base, so use a link to out/deeper.
    std::fs::create_dir(g.p("out/deeper")).unwrap();
    std::fs::write(g.p("out/secret2"), "outside\n").unwrap();
    std::fs::write(g.p("work/secret2"), "inside\n").unwrap();
    symlink(g.p("out/deeper"), g.p("work/link")).unwrap();
    assert_eq!(g.run(&["cat", "link/../secret2"]), (1, "".into(), outside("link/../secret2")));
    let (st, _, err) = g.run_redir(&["echo", "x"], "write", "link/../new");
    assert_eq!((st, err.as_str()), (1, outside("link/../new").as_str()));
    assert!(!g.p("out/new").exists() && !g.p("work/new").exists());
    assert_eq!(g.run(&["mkdir", "link/../d"]).0, 1);
    assert!(!g.p("out/d").exists());
    // Controls: the same shape through a real directory works.
    std::fs::create_dir(g.p("work/real")).unwrap();
    assert_eq!(g.run(&["cat", "real/../secret2"]), (0, "inside\n".into(), "".into()));
    assert_eq!(g.run(&["mkdir", "real/../d2"]), (0, "".into(), "".into()));
    assert!(g.p("work/d2").is_dir());
    // cd sub; cat ../a.txt and > ../where.txt work inside the grant
    std::fs::write(g.p("work/a.txt"), "A\n").unwrap();
    assert_eq!(g.run(&["cd", "sub"]).0, 0);
    assert_eq!(g.run(&["cat", "../a.txt"]), (0, "A\n".into(), "".into()));
    let (st, _, err) = g.run_redir(&["pwd"], "write", "../where.txt");
    assert_eq!((st, err.as_str()), (0, ""));
    assert_eq!(std::fs::read_to_string(g.p("work/where.txt")).unwrap(), format!("{}\n", g.abs("work/sub")));
    // cd .. back to the top grant works; cd .. above it fails with the grants message
    assert_eq!(g.run(&["cd", ".."]).0, 0);
    assert_eq!(g.s.cwd(), g.p("work"));
    assert_eq!(g.run(&["cd", ".."]), (1, "".into(), outside("..")));
    assert_eq!(g.s.cwd(), g.p("work"));
}

#[test]
fn symlink_loops_are_denied_not_guessed() {
    let mut g = G::new();
    symlink(g.p("work/b"), g.p("work/a")).unwrap();
    symlink(g.p("work/a"), g.p("work/b")).unwrap();
    assert_eq!(g.run(&["cat", "a"]), (1, "".into(), "kapsel: a: cannot resolve (symlink loop)\n".into()));
    let (st, _, err) = g.run_redir(&["echo", "x"], "write", "b");
    assert_eq!((st, err.as_str()), (1, "kapsel: b: cannot resolve (symlink loop)\n"));
    assert_eq!(g.run(&["mkdir", "-p", "a/c"]).0, 1);
    // a loop that is a prefix of a deeper path
    assert_eq!(g.run(&["cat", "a/x/y"]).0, 1);
}

// ---- cd, globs ---------------------------------------------------------------

#[test]
fn cd_outside_grants_fails_and_cd_to_read_only_works() {
    let mut g = G::new();
    let out = g.abs("out");
    assert_eq!(g.run(&["cd", &out]), (1, "".into(), outside(&out)));
    assert_eq!(g.s.cwd(), g.p("work"));
    assert_eq!(g.run(&["cd", "/"]), (1, "".into(), outside("/")));
    let ro = g.abs("ro");
    assert_eq!(g.run(&["cd", &ro]).0, 0);
    assert_eq!(g.s.cwd(), g.p("ro"));
}

#[test]
fn glob_outside_grants_stays_literal_and_probes_nothing() {
    let mut g = G::new();
    // only cwd is granted: /etc/pass* must not expand
    let r = g.s.t_expand(&[vec![TPart::Lit("/etc/pass", false), TPart::Glob("*")]]);
    assert_eq!(r, vec!["/etc/pass*".to_string()]);
    let r = g.s.t_expand(&[vec![TPart::Glob("/e?c/passwd")]]);
    assert_eq!(r, vec!["/e?c/passwd".to_string()]);
    // a literal path outside is not "found" either
    let out = g.abs("out");
    let r = g.s.t_expand(&[vec![TPart::Lit(&out, false), TPart::Lit("/", false), TPart::Glob("*")]]);
    assert_eq!(r, vec![format!("{out}/*")]);
    // `..` in a pattern is not followed
    let r = g.s.t_expand(&[vec![TPart::Lit("../out/", false), TPart::Glob("*")]]);
    assert_eq!(r, vec!["../out/*".to_string()]);
    // control: inside the grant it expands
    std::fs::write(g.p("work/a.txt"), "").unwrap();
    let r = g.s.t_expand(&[vec![TPart::Glob("*.txt")]]);
    assert_eq!(r, vec!["a.txt".to_string()]);
    // control: a read-only grant is visible to globs
    let ro = g.abs("ro");
    let r = g.s.t_expand(&[vec![TPart::Lit(&ro, false), TPart::Lit("/", false), TPart::Glob("f*")]]);
    assert_eq!(r, vec![format!("{ro}/file")]);
}

#[test]
fn invalid_grants_are_errors() {
    let g = GrantSet { read_write: vec![PathBuf::from("/definitely/not/here")], read_only: vec![] };
    assert!(Session::new(g).is_err());
    let f = tempfile::NamedTempFile::new().unwrap();
    let g = GrantSet { read_write: vec![f.path().to_path_buf()], read_only: vec![] };
    let e = Session::new(g).err().unwrap();
    assert!(e.0.contains("not a directory"), "{}", e.0);
}
