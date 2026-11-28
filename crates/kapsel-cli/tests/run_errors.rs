//! Run errors through the CLI (REQ-KS-060): a built-in that fails at run time
//! is rendered, after its own stderr line, as `error[run:<class>]` at the
//! command that failed (file:line:col, the source line underlined) with a
//! `= help:` line. stdout and the exit status do not change. Each test runs
//! the real binary with KAPSEL_HOME in a scratch directory.

use assert_cmd::Command;
use tempfile::TempDir;

struct Env {
    dir: TempDir,
}

struct Out {
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

impl Env {
    fn new() -> Env {
        Env {
            dir: tempfile::tempdir().unwrap(),
        }
    }
    fn cmd(&self) -> Command {
        let mut c = Command::cargo_bin("kapsel").unwrap();
        c.current_dir(self.dir.path())
            .env("KAPSEL_HOME", self.dir.path().join(".kapsel-home"));
        c
    }
    fn write(&self, rel: &str, s: &str) {
        std::fs::write(self.dir.path().join(rel), s).unwrap();
    }
    fn run(&self, args: &[&str]) -> Out {
        let o = self.cmd().args(args).output().unwrap();
        Out {
            code: o.status.code(),
            stdout: String::from_utf8_lossy(&o.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&o.stderr).into_owned(),
        }
    }
    fn script(&self, text: &str) -> Out {
        self.write("s.sh", text);
        self.run(&["s.sh"])
    }
}

/// The rendered diagnostics with id `id`: each from its `error[id]` line up
/// to the blank line that ends it.
fn diags<'a>(stderr: &'a str, id: &str) -> Vec<&'a str> {
    let head = format!("error[{id}]");
    let mut v = Vec::new();
    let mut rest = stderr;
    while let Some(i) = rest.find(&head) {
        let d = &rest[i..];
        let end = d.find("\n\n").unwrap_or(d.len());
        v.push(&d[..end]);
        rest = &d[end..];
    }
    v
}

fn one<'a>(o: &'a Out, id: &str) -> &'a str {
    let d = diags(&o.stderr, id);
    assert_eq!(d.len(), 1, "want one {id}: {}", o.stderr);
    d[0]
}

/// Mutation: number sites after the captures (or start at 1) in
/// `kapsel_compile::lower`, or drop the `+ offset` in the CLI: the arrow
/// moves off `s.sh:2:1`.
#[test]
fn a_grants_violation_renders_at_the_command_with_the_grant_help() {
    let e = Env::new();
    let outside = tempfile::tempdir().unwrap();
    let f = outside.path().canonicalize().unwrap().join("secret.txt");
    std::fs::write(&f, "x\n").unwrap();
    let f = f.display().to_string();
    let o = e.script(&format!("echo start\ncat {f}\n"));
    assert_eq!(o.code, Some(1), "{}", o.stderr);
    assert_eq!(o.stdout, "start\n", "stdout unchanged");
    let line = format!("kapsel: {f}: outside the session's grants");
    // The built-in's own line comes first, then the diagnostic.
    let own = o.stderr.find(&format!("{line}\n")).expect("own line");
    let d = one(&o, "run:grants");
    assert!(o.stderr.find(d).unwrap() > own, "{}", o.stderr);
    assert!(
        d.starts_with(&format!("error[run:grants]: {line}\n")),
        "{d}"
    );
    assert!(d.contains(" --> s.sh:2:1\n"), "{d}");
    assert!(d.contains(&format!("2 | cat {f}\n")), "{d}");
    let underline = format!("  | {}\n", "^".repeat(4 + f.len()));
    assert!(d.contains(&underline), "{d}");
    assert!(
        d.contains("= help: ") && d.contains("`--rw DIR` / `--ro DIR`"),
        "{d}"
    );
}

/// Mutation: render the site of the first command (or of `site - 1`): the
/// arrow points at `echo a > a.txt` on line 1.
#[test]
fn a_missing_file_in_the_second_command_points_at_that_command() {
    let e = Env::new();
    let o = e.script("echo a > a.txt\ncat missing.txt\necho done\n");
    assert_eq!(o.code, Some(0), "the script's status is its last command's");
    assert_eq!(o.stdout, "done\n");
    assert!(
        o.stderr
            .contains("cat: missing.txt: No such file or directory\n"),
        "{}",
        o.stderr
    );
    let d = one(&o, "run:not-found");
    assert!(d.contains(" --> s.sh:2:1\n"), "{d}");
    assert!(d.contains("2 | cat missing.txt\n"), "{d}");
    assert!(d.contains("  | ^^^^^^^^^^^^^^^\n"), "{d}");
    assert!(d.contains("= help: `missing.txt` does not exist"), "{d}");
}

/// Mutation: drop the near-miss search in `run_diag`: no `did you mean`.
#[test]
fn a_missing_file_near_a_written_path_says_did_you_mean() {
    let e = Env::new();
    let o = e.script("echo a > results.txt\ncat result.txt\n");
    let d = one(&o, "run:not-found");
    assert!(d.contains("did you mean `results.txt`"), "{d}");
}

/// Mutation: give a `for` stage a site of its own in the compiler (and the
/// validator), or count only the loop in `Plan::commands`: the arrow moves
/// to the loop on line 1. Identical errors of one command render once,
/// different ones each.
#[test]
fn an_error_in_a_for_loop_body_points_at_the_body_command() {
    let e = Env::new();
    let o = e.script("for f in x y x; do\n  cat \"$f.txt\"\ndone\n");
    assert_eq!(o.code, Some(1));
    let d = diags(&o.stderr, "run:not-found");
    assert_eq!(d.len(), 2, "x.txt once, y.txt once: {}", o.stderr);
    for d in &d {
        assert!(d.contains(" --> s.sh:2:3\n"), "{d}");
        assert!(d.contains("2 |   cat \"$f.txt\"\n"), "{d}");
        assert!(!d.contains("s.sh:1:"), "{d}");
    }
}

/// Mutation: render a captured command at the outer command's site (name
/// the site before the captures run): the arrow covers `echo "$(cat nope)"`.
#[test]
fn an_error_inside_a_capture_points_at_the_captured_command() {
    let e = Env::new();
    let o = e.run(&["-c", "echo \"$(cat nope)\" ok"]);
    let d = one(&o, "run:not-found");
    assert!(d.contains(" --> -c:1:9\n"), "{d}");
    assert!(d.contains("  |         ^^^^^^^^\n"), "{d}");
}

/// Mutation: render against the line text instead of the history (or drop
/// the line's offset): the arrow reads `history:1:1` or `-c`.
#[test]
fn interactive_run_errors_render_against_the_history() {
    let e = Env::new();
    let o = e
        .cmd()
        .write_stdin("echo hi > b.txt\ncat nope.txt\n")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&o.stderr).into_owned();
    assert_eq!(o.status.code(), Some(1), "{stderr}");
    let d = diags(&stderr, "run:not-found");
    assert_eq!(d.len(), 1, "{stderr}");
    assert!(d[0].contains(" --> history:2:1\n"), "{}", d[0]);
    assert!(d[0].contains("2 | cat nope.txt\n"), "{}", d[0]);
}

/// One script per error class: the id, the arrow and a help line. With the
/// class table this shows every class can be produced and is rendered with
/// help. Mutation: classify everything as `failed` in `builtins::classify`
/// / `Ctx::fail`, or drop the `= help:` element in `render_run_error`.
#[test]
fn every_error_class_renders_with_a_help_line() {
    let e = Env::new();
    e.write("data.txt", "not json\n");
    std::fs::create_dir(e.dir.path().join("d")).unwrap();
    std::os::unix::fs::symlink("data.txt", e.dir.path().join("link")).unwrap();
    let cases: &[(&str, &str, &str)] = &[
        ("run:grants", "cat /", "outside the session's grants"),
        ("run:symlink", "echo x > link", "symlink"),
        ("run:not-found", "cat nope", "does not exist"),
        (
            "run:usage",
            "sort -Q data.txt",
            "kapsel's sort supports: -u",
        ),
        ("run:regex", "grep -E 'a(b' data.txt", "column 2 (`(`)"),
        (
            "run:edit-no-match",
            "edit data.txt -s zzz y",
            "`--allow-none`",
        ),
        ("run:parse", "json get .a data.txt", "`--str`"),
        ("run:io", "cat d", "refused the operation"),
        ("run:failed", "seq x", "fix that operand"),
    ];
    let mut seen = Vec::new();
    for (id, src, help) in cases {
        let o = e.run(&["-c", src]);
        assert_ne!(o.code, Some(0), "{src}");
        let d = one(&o, id);
        assert!(d.contains(" --> -c:1:1\n"), "{src}: {d}");
        let h = d.split("= help: ").nth(1).unwrap_or_else(|| panic!("{d}"));
        assert!(h.contains(help), "{src}: help {h:?} lacks {help:?}");
        seen.push(*id);
    }
    // Permission needs a mode the test user cannot bypass (not root).
    let locked = e.dir.path().join("locked.txt");
    std::fs::write(&locked, "x").unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
    if std::fs::read(&locked).is_err() {
        let o = e.run(&["-c", "cat locked.txt"]);
        let d = one(&o, "run:permission");
        assert!(d.contains("= help: the file system refuses access"), "{d}");
        seen.push("run:permission");
    }
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o644)).unwrap();
    for id in kapsel_diag::RUN_ERROR_IDS {
        assert!(
            seen.contains(id) || *id == "run:permission",
            "{id} has no case"
        );
    }
}

/// The class table, the ids and the explain entries agree: every host class
/// has an id `run:<class>` with an explanation and a help line, and every
/// id names a class. Mutation: add a class without an id, or delete an
/// explanation.
#[test]
fn every_run_error_id_has_a_class_an_explanation_and_help() {
    let ids: Vec<String> = kapsel_host::ErrorClass::ALL
        .iter()
        .map(|c| format!("run:{}", c.as_str()))
        .collect();
    assert_eq!(
        ids,
        kapsel_diag::RUN_ERROR_IDS,
        "one id per class, in order"
    );
    for c in kapsel_host::ErrorClass::ALL {
        let id = format!("run:{}", c.as_str());
        let text = kapsel_diag::explain(&id).unwrap_or_else(|| panic!("{id}: no explain"));
        assert!(text.starts_with(&id), "{id}");
        assert!(text.contains("What to do instead:"), "{id}");
        for operand in [None, Some("missing.txt")] {
            let h = kapsel_host::help_for(*c, "cat", operand, None);
            assert!(!h.trim().is_empty(), "{id}: empty help");
        }
        // Run error ids never collide with rule ids (`error[KS-…]` marks
        // kapsel's own refusals, docs/first-shell.md).
        assert!(!kapsel_diag::RULE_IDS.contains(&id.as_str()));
        assert!(!id.starts_with("KS-"));
    }
}

/// `kapsel explain run:grants` works like a rule id.
#[test]
fn explain_covers_run_error_ids() {
    let e = Env::new();
    for id in kapsel_diag::RUN_ERROR_IDS {
        let o = e.run(&["explain", id]);
        assert_eq!(o.code, Some(0), "{id}: {}", o.stderr);
        assert!(o.stdout.starts_with(id), "{id}: {}", o.stdout);
    }
}

/// The help-examples self-check for run errors: every backticked fragment of
/// every class's help (and of the not-found "did you mean") goes through
/// kapsel's parser and check stage, so a help line never suggests syntax
/// kapsel refuses. `--rw DIR` / `--ro DIR` are flags of kapsel's own command
/// line, not shell. Mutation: write the edit help as `edit ... || true`.
#[test]
fn run_error_help_examples_pass_kapsel_s_own_check() {
    const OUTER: &[&str] = &["--rw DIR", "--ro DIR"];
    let mut helps: Vec<String> = Vec::new();
    for c in kapsel_host::ErrorClass::ALL {
        helps.push(kapsel_host::help_for(
            *c,
            "sort",
            Some("missing.txt"),
            Some("-u -r"),
        ));
        helps.push(kapsel_host::help_for(*c, "sort", None, None));
    }
    let mut checked = 0;
    for help in &helps {
        for (i, frag) in help.split('`').enumerate() {
            // The operand and the built-in name are quoted, not suggested.
            if i % 2 == 0 || OUTER.contains(&frag) || frag == "missing.txt" || frag == "sort" {
                continue;
            }
            // A flag is checked on the built-in it belongs to.
            let src = match frag {
                "--allow-none" => "edit f.txt -s a b --allow-none".to_string(),
                "--str" => "json set .a 1 f.json --str".to_string(),
                f if f.starts_with('-') => panic!("flag `{f}` has no sample command"),
                f => f.to_string(),
            };
            let script = kapsel_syntax::parse(&src)
                .unwrap_or_else(|e| panic!("help fragment `{frag}` does not parse: {e:?}"));
            let refused = match kapsel_check::check(&src, &script) {
                kapsel_plan::CheckOutcome::Refused(r) => r.findings,
                kapsel_plan::CheckOutcome::Plan(_) => Vec::new(),
            };
            assert!(
                refused.is_empty(),
                "help `{help}` suggests `{frag}`, refused as {src:?}: {refused:?}"
            );
            checked += 1;
        }
    }
    assert!(checked >= 2, "only {checked} fragments checked");
}

/// Second review, non-blocking 3: a file name holding ESC reaches the
/// `error[run:…]` diagnostic escaped (`\x1b`), from the script name and the
/// message alike. (The built-in's own stderr line before it is the file
/// name as the built-in reports it.)
#[test]
fn control_characters_in_a_run_error_are_escaped() {
    let e = Env::new();
    e.write("s\u{1b}[2J.sh", "cat \"$(printf 'x\\033[2Jy')\"\n");
    let o = e.run(&["s\u{1b}[2J.sh"]);
    assert_eq!(o.code, Some(1), "{}", o.stderr);
    let diag = &o.stderr[o.stderr.find("error[run:").expect("a run error")..];
    assert!(!diag.contains('\u{1b}'), "{diag:?}");
    assert!(diag.contains("x\\x1b[2Jy"), "{diag}");
    assert!(diag.contains("s\\x1b[2J.sh"), "{diag}");
}
