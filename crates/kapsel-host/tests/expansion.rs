//! Word expansion, redirects and pipes as the compiled plan drives them.

mod common;
use common::T;
use kapsel_host::TPart::{self, *};

fn w(parts: Vec<TPart>) -> Vec<TPart> {
    parts
}

fn lit(s: &str) -> Vec<TPart<'_>> {
    vec![Lit(s, true)]
}

#[test]
fn concatenation_and_unset_variables() {
    let mut t = T::new();
    t.s.t_set_var("X", &[Lit("world", true)]);
    let r = t.s.t_expand(&[w(vec![Lit("hello ", true), Var("X", true)]), w(vec![Var("UNSET", true)])]);
    assert_eq!(r, vec!["hello world".to_string(), "".to_string()]);
}

#[test]
fn status_variable_tracks_the_last_call() {
    let mut t = T::new();
    t.run(&["false"]);
    assert_eq!(t.s.t_expand(&[vec![Var("?", true)]]), vec!["1"]);
    t.run(&["true"]);
    assert_eq!(t.s.t_expand(&[vec![Var("?", false)]]), vec!["0"]);
}

#[test]
fn unquoted_variables_split_on_ifs_and_quoted_do_not() {
    let mut t = T::new();
    t.s.t_set_var("X", &[Lit(" a  b\tc\n", true)]);
    assert_eq!(t.s.t_expand(&[vec![Var("X", false)]]), vec!["a", "b", "c"]);
    assert_eq!(t.s.t_expand(&[vec![Var("X", true)]]), vec![" a  b\tc\n"]);
    // prefix and suffix stick to the first and last fields
    t.s.t_set_var("Y", &[Lit("a b", true)]);
    assert_eq!(t.s.t_expand(&[vec![Lit("x", true), Var("Y", false), Lit("z", true)]]), vec!["xa", "bz"]);
}

#[test]
fn empty_unquoted_expansion_makes_no_field_but_quoted_empty_does() {
    let mut t = T::new();
    assert_eq!(t.s.t_expand(&[vec![Var("UNSET", false)]]), Vec::<String>::new());
    assert_eq!(t.s.t_expand(&[vec![Var("UNSET", true)]]), vec![""]);
    assert_eq!(t.s.t_expand(&[vec![Lit("", true)]]), vec![""]);
    assert_eq!(t.s.t_expand(&[vec![Lit("a", true), Var("UNSET", false)]]), vec!["a"]);
    assert_eq!(t.s.t_expand(&[vec![Var("UNSET", false)], lit("b")]), vec!["b"]);
}

#[test]
fn custom_ifs_with_non_whitespace_separator() {
    let mut t = T::new();
    t.s.t_set_var("IFS", &[Lit(":", true)]);
    t.s.t_set_var("P", &[Lit("a:b::c", true)]);
    assert_eq!(t.s.t_expand(&[vec![Var("P", false)]]), vec!["a", "b", "", "c"]);
}

#[test]
fn captured_parts_strip_trailing_newlines_and_split_when_unquoted() {
    let mut t = T::new();
    let p = t.s.t_pipe();
    t.s.t_pipe_write(p, b"one  two\n\n\n");
    assert_eq!(t.s.t_expand(&[vec![Captured(p, true)]]), vec!["one  two"]);
    assert_eq!(t.s.t_expand(&[vec![Captured(p, false)]]), vec!["one", "two"]);
    let q = t.s.t_pipe();
    t.s.t_pipe_write(q, &[b'a', 0xff, b'\n']);
    assert_eq!(t.s.t_expand(&[vec![Captured(q, true)]]), vec!["a\u{fffd}"]);
}

#[test]
fn unquoted_globs_match_sorted_hidden_files_excluded() {
    let mut t = T::new();
    for f in ["b.txt", "a.txt", "c.md", ".hidden.txt"] {
        t.write(f, "");
    }
    t.write("d/x.txt", "");
    t.write("d/y.md", "");
    assert_eq!(t.s.t_expand(&[vec![Glob("*.txt")]]), vec!["a.txt", "b.txt"]);
    assert_eq!(t.s.t_expand(&[vec![Glob("?.txt")]]), vec!["a.txt", "b.txt"]);
    assert_eq!(t.s.t_expand(&[vec![Glob("[ab].txt")]]), vec!["a.txt", "b.txt"]);
    assert_eq!(t.s.t_expand(&[vec![Glob("[!a].txt")]]), vec!["b.txt"]);
    assert_eq!(t.s.t_expand(&[vec![Glob(".*.txt")]]), vec![".hidden.txt"]);
    assert_eq!(t.s.t_expand(&[vec![Lit("d/", false), Glob("*")]]), vec!["d/x.txt", "d/y.md"]);
    assert_eq!(t.s.t_expand(&[vec![Glob("*/*.md")]]), vec!["d/y.md"]);
    assert_eq!(t.s.t_expand(&[vec![Glob("*/")]]), vec!["d/"]);
    // no match: the pattern text, as POSIX
    assert_eq!(t.s.t_expand(&[vec![Glob("*.zip")]]), vec!["*.zip"]);
    // two words each expanded on their own
    assert_eq!(t.s.t_expand(&[vec![Glob("*.md")], vec![Glob("a*")]]), vec!["c.md", "a.txt"]);
}

#[test]
fn redirects_write_append_read() {
    let mut t = T::new();
    let none: Vec<Vec<TPart>> = vec![];
    let _ = none;
    let echo = |s: &'static str| vec![lit("echo"), lit(s)];
    assert_eq!(t.s.t_call(&echo("one"), &[(1, "write", lit("f.txt"), 0)], 0, 0), 0);
    assert_eq!(t.read("f.txt"), "one\n");
    assert_eq!(t.s.t_call(&echo("two"), &[(1, "append", lit("f.txt"), 0)], 0, 0), 0);
    assert_eq!(t.read("f.txt"), "one\ntwo\n");
    assert_eq!(t.s.t_call(&echo("three"), &[(1, "write", lit("f.txt"), 0)], 0, 0), 0);
    assert_eq!(t.read("f.txt"), "three\n");
    // read: cat < f.txt writes to the session stream
    t.out.clear();
    t.s.set_io(Box::new(std::io::empty()), t.out.clone(), t.err.clone());
    assert_eq!(t.s.t_call(&[lit("cat")], &[(0, "read", lit("f.txt"), 0)], 0, 0), 0);
    assert_eq!(t.out.string(), "three\n");
    // a missing input file fails the command without running it
    t.err.clear();
    assert_eq!(t.s.t_call(&[lit("cat")], &[(0, "read", lit("nope"), 0)], 0, 0), 1);
    assert_eq!(t.err.string(), "kapsel: nope: No such file or directory\n");
}

#[test]
fn redirect_target_is_expanded_and_must_be_one_word() {
    let mut t = T::new();
    t.s.t_set_var("F", &[Lit("out.txt", true)]);
    t.s.t_call(&[lit("echo"), lit("x")], &[(1, "write", vec![Var("F", false)], 0)], 0, 0);
    assert_eq!(t.read("out.txt"), "x\n");
    t.s.t_set_var("TWO", &[Lit("a b", true)]);
    t.err.clear();
    let st = t.s.t_call(&[lit("echo"), lit("x")], &[(1, "write", vec![Var("TWO", false)], 0)], 0, 0);
    assert_eq!(st, 1);
    assert_eq!(t.err.string(), "kapsel: a b: ambiguous redirect\n");
}

#[test]
fn dup_and_stderr_redirects() {
    let mut t = T::new();
    // `ls missing 2>&1` with stdout in a pipe: the error lands in the pipe
    let p = t.s.t_pipe();
    t.err.clear();
    let st = t.s.t_call(&[lit("ls"), lit("missing")], &[(2, "dup-out", vec![], 1)], 0, p);
    assert_eq!(st, 2);
    assert_eq!(String::from_utf8(t.s.t_pipe_read(p)).unwrap(), "ls: cannot access 'missing': No such file or directory\n");
    assert_eq!(t.err.string(), "");
    // `echo hi >&2`: stdout goes to the session's stderr
    t.out.clear();
    t.s.t_call(&[lit("echo"), lit("hi")], &[(1, "dup-out", vec![], 2)], 0, 0);
    assert_eq!(t.err.string(), "hi\n");
    assert_eq!(t.out.string(), "");
    // `cmd 2>file`
    t.s.t_call(&[lit("ls"), lit("missing")], &[(2, "write", lit("e.txt"), 0)], 0, 0);
    assert_eq!(t.read("e.txt"), "ls: cannot access 'missing': No such file or directory\n");
    // `>/dev/null`
    t.out.clear();
    t.s.t_call(&[lit("echo"), lit("gone")], &[(1, "write", lit("/dev/null"), 0)], 0, 0);
    assert_eq!(t.out.string(), "");
}

#[test]
fn heredoc_body_is_expanded_into_stdin() {
    let mut t = T::new();
    t.s.t_set_var("X", &[Lit("1", true)]);
    let p = t.s.t_pipe();
    t.s.t_call(&[lit("cat")], &[(0, "heredoc", vec![Lit("v=", true), Var("X", true), Lit("\nline two\n", true)], 0)], 0, p);
    assert_eq!(String::from_utf8(t.s.t_pipe_read(p)).unwrap(), "v=1\nline two\n");
}

#[test]
fn pipeline_stages_run_in_order_through_pipes() {
    let mut t = T::new();
    // echo hello | wc -c
    let p1 = t.s.t_pipe();
    assert_eq!(t.s.t_call(&[lit("echo"), lit("hello")], &[], 0, p1), 0);
    t.out.clear();
    t.s.set_io(Box::new(std::io::empty()), t.out.clone(), t.err.clone());
    assert_eq!(t.s.t_call(&[lit("wc"), lit("-c")], &[], p1, 0), 0);
    assert_eq!(t.out.string(), "6\n");
    // cat a | sort -u | head -n 1
    t.write("a", "b\na\nb\n");
    let q1 = t.s.t_pipe();
    let q2 = t.s.t_pipe();
    t.s.t_call(&[lit("cat"), lit("a")], &[], 0, q1);
    t.s.t_call(&[lit("sort"), lit("-u")], &[], q1, q2);
    t.out.clear();
    t.s.t_call(&[lit("head"), lit("-n"), lit("1")], &[], q2, 0);
    assert_eq!(t.out.string(), "a\n");
}

#[test]
fn field_splitting_applies_to_command_arguments() {
    let mut t = T::new();
    t.write("a", "A\n");
    t.write("b", "B\n");
    t.s.t_set_var("FILES", &[Lit("a b", true)]);
    t.out.clear();
    t.s.set_io(Box::new(std::io::empty()), t.out.clone(), t.err.clone());
    t.s.t_call(&[lit("cat"), vec![Var("FILES", false)]], &[], 0, 0);
    assert_eq!(t.out.string(), "A\nB\n");
    // a glob in argv
    t.out.clear();
    t.s.t_call(&[lit("cat"), vec![Glob("[ab]")]], &[], 0, 0);
    assert_eq!(t.out.string(), "A\nB\n");
}

#[test]
fn scopes_restore_variables_exports_cwd_and_status_but_keep_pipes() {
    let mut t = T::new();
    t.write("sub/f", "");
    let root = t.root();
    t.s.t_set_var("X", &[Lit("1", true)]);
    t.run(&["true"]);
    t.s.t_scope_push();
    t.s.t_set_var("X", &[Lit("2", true)]);
    t.run(&["export", "NEW=1"]);
    t.run(&["cd", "sub"]);
    t.run(&["false"]);
    // a pipe made and written inside the scope is still valid after the pop
    let p = t.s.t_pipe();
    t.s.t_pipe_write(p, b"captured\n");
    // nested scope
    t.s.t_scope_push();
    t.s.t_set_var("X", &[Lit("3", true)]);
    t.s.t_scope_pop();
    assert_eq!(t.s.var("X").as_deref(), Some("2"));
    t.s.t_scope_pop();
    assert_eq!(t.s.var("X").as_deref(), Some("1"));
    assert!(!t.s.is_exported("NEW") && t.s.var("NEW").is_none());
    assert_eq!(t.s.cwd(), root);
    assert_eq!(t.s.status(), 0);
    assert_eq!(t.s.t_pipe_read(p), b"captured\n");
}

#[test]
fn bad_pipe_handle_traps_instead_of_guessing() {
    let mut t = T::new();
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| t.s.t_call(&[lit("cat")], &[], 99, 0)));
    assert!(r.is_err());
}
