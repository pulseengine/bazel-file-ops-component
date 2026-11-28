//! The real chain: parse -> check -> compile -> host. Complements e2e.rs,
//! whose components are hand-built.

mod common;
use common::T;

use kapsel_plan::CheckOutcome;

fn build(src: &str) -> Vec<u8> {
    let script = kapsel_syntax::parse(src).unwrap_or_else(|e| panic!("parse {src:?}: {e:?}"));
    match kapsel_check::check(src, &script) {
        CheckOutcome::Plan(p) => {
            kapsel_compile::compile(&p).unwrap_or_else(|e| panic!("compile {src:?}: {e}"))
        }
        CheckOutcome::Refused(r) => panic!("refused {src:?}: {:?}", r.findings),
    }
}

/// Runs `src` in the session; returns (status, stdout, stderr).
fn run(t: &mut T, src: &str) -> (u32, String, String) {
    t.out.clear();
    t.err.clear();
    let c = build(src);
    let st =
        t.s.run_component(&c)
            .unwrap_or_else(|e| panic!("run {src:?}: {e}"));
    (st, t.out.string(), t.err.string())
}

#[test]
fn pipeline_of_two_and_three_stages() {
    let mut t = T::new();
    assert_eq!(
        run(&mut t, "echo hello | wc -c"),
        (0, "6\n".into(), "".into())
    );
    t.write("a", "b\na\nb\n");
    assert_eq!(
        run(&mut t, "cat a | sort -u | head -n 1"),
        (0, "a\n".into(), "".into())
    );
}

#[test]
fn acceptance_lines() {
    let mut t = T::new();
    assert_eq!(
        run(
            &mut t,
            "mkdir -p out && echo hi > out/a.txt && cat out/a.txt"
        ),
        (0, "hi\n".into(), "".into())
    );
    assert_eq!(
        run(&mut t, "grep -n foo missing.txt || echo none"),
        (
            0,
            "none\n".into(),
            "grep: missing.txt: No such file or directory\n".into()
        )
    );
    assert_eq!(
        run(&mut t, "X=world; echo \"hello $X\""),
        (0, "hello world\n".into(), "".into())
    );
}

#[test]
fn status_follows_set_status_from_the_plan() {
    let mut t = T::new();
    assert_eq!(run(&mut t, "false").0, 1);
    assert_eq!(t.s.status(), 1);
    assert_eq!(run(&mut t, "echo $?"), (0, "1\n".into(), "".into()));
    assert_eq!(run(&mut t, "! false").0, 0);
    assert_eq!(run(&mut t, "true && false").0, 1);
    assert_eq!(run(&mut t, "echo a | false").0, 1);
}

#[test]
fn exit_stops_the_plan_with_its_status() {
    let mut t = T::new();
    assert_eq!(
        run(&mut t, "echo before; exit 7; echo after"),
        (7, "before\n".into(), "".into())
    );
}

#[test]
fn subshell_scope_restores_variables_and_cwd() {
    let mut t = T::new();
    t.write("sub/f", "");
    let root = t.root();
    let (st, out, _) = run(&mut t, "X=1; echo $(X=2; cd sub); echo $X; pwd");
    assert_eq!(st, 0);
    assert_eq!(out, format!("\n1\n{}\n", root.display()));
    assert_eq!(t.s.var("X").as_deref(), Some("1"));
    assert_eq!(t.s.cwd(), root);
}

#[test]
fn capture_output_flows_into_the_command() {
    let mut t = T::new();
    assert_eq!(
        run(&mut t, "echo $(echo a b)"),
        (0, "a b\n".into(), "".into())
    );
    assert_eq!(
        run(&mut t, "echo \"$(echo x; echo y)\""),
        (0, "x\ny\n".into(), "".into())
    );
    assert_eq!(
        run(&mut t, "echo \"[$(printf 'a\\n\\n')]\""),
        (0, "[a]\n".into(), "".into())
    );
}

#[test]
fn for_loop_binds_items_and_frees_lists() {
    let mut t = T::new();
    t.write("a.txt", "");
    t.write("b.txt", "");
    assert_eq!(
        run(&mut t, "for f in *.txt; do echo $f; done"),
        (0, "a.txt\nb.txt\n".into(), "".into())
    );
    assert_eq!(
        run(&mut t, "for i in 1 2 3; do echo $i; done"),
        (0, "1\n2\n3\n".into(), "".into())
    );
    // a second run starts with a clean list table
    assert_eq!(run(&mut t, "for i in x y; do echo $i; done").1, "x\ny\n");
}

#[test]
fn while_until_loops() {
    let mut t = T::new();
    assert_eq!(
        run(
            &mut t,
            "i=; until [ \"$i\" = xx ]; do i=${i}x; echo $i; done"
        )
        .1,
        "x\nxx\n"
    );
}

#[test]
fn redirect_and_stderr_capture_through_the_chain() {
    let mut t = T::new();
    assert_eq!(
        run(&mut t, "echo one > f; echo two >> f; cat f").1,
        "one\ntwo\n"
    );
    assert_eq!(run(&mut t, "cat < f | wc -l").1, "2\n");
    let (st, out, err) = run(&mut t, "cat missing 2>&1 | cat");
    assert_eq!(
        (st, out.as_str(), err.as_str()),
        (0, "cat: missing: No such file or directory\n", "")
    );
}

#[test]
fn grants_hold_through_the_chain() {
    let mut t = T::new();
    let (st, out, err) = run(&mut t, "cat /etc/hosts");
    assert_eq!((st, out.as_str()), (1, ""));
    assert_eq!(err, "kapsel: /etc/hosts: outside the session's grants\n");
    assert_eq!(
        run(&mut t, "echo x > /tmp/kapsel-host-should-not-exist").0,
        1
    );
    assert!(!std::path::Path::new("/tmp/kapsel-host-should-not-exist").exists());
}

#[test]
fn cd_and_variables_persist_between_lines() {
    let mut t = T::new();
    t.write("d/f", "found\n");
    run(&mut t, "cd d; X=1");
    assert_eq!(run(&mut t, "cat f; echo $X").1, "found\n1\n");
}

#[test]
fn heredoc_through_the_chain() {
    let mut t = T::new();
    assert_eq!(run(&mut t, "X=7\ncat <<EOF\nv=$X\nEOF\n").1, "v=7\n");
}
