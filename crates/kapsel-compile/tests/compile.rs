//! Oracles for kapsel-compile (REQ-KS-020, DD-KS-005):
//!
//! - determinism: every plan compiles to identical bytes twice;
//! - validity: the component validates, imports only `kapsel:shell/host`,
//!   exports only `run`;
//! - behaviour: a recording mock host (wasmtime) shows the call sequence;
//! - honesty: every decline path has a test, with its code and span;
//! - rule coverage: every rule in `RULES` is exercised by a named plan;
//! - frozen fixtures: `testdata/kapsel/compile/*.plan.json` hash to their
//!   committed `*.sha256` (re-bless with `KAPSEL_BLESS=1`);
//! - known-wrong table: `KNOWN.md` lists exactly the pinned counts.

use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use kapsel_compile::{CompileError, DECLINED, KNOWN_WRONG, RULES, code, compile, compile_report};
use kapsel_plan::{
    AndOrOp, Call, Grants, PAndOr, PLAN_SCHEMA, PPart, PPipeline, PRedirect, PRedirectTarget,
    PStage, PWord, Plan, RedirectOp, Span, Tool,
};

mod bindings {
    wasmtime::component::bindgen!({
        path: "../../wit/kapsel-shell.wit",
        world: "plan",
    });
}
use bindings::kapsel::shell::host::{self as wit, Part, RedirectOp as WOp};

// ---- plan builders ----------------------------------------------------------

fn lit(s: &str) -> PWord {
    PWord::literal(s)
}

fn var(name: &str) -> PWord {
    PWord {
        parts: vec![PPart::Var {
            name: name.into(),
            quoted: false,
        }],
    }
}

fn cap(body: Vec<PAndOr>) -> PWord {
    PWord {
        parts: vec![PPart::Capture {
            body,
            quoted: false,
        }],
    }
}

fn callw(argv: Vec<PWord>) -> Call {
    let name = argv[0].as_literal().unwrap_or_default();
    Call {
        tool: Some(Tool::Builtin(name)),
        argv,
        assignments: vec![],
        redirects: vec![],
        span: Span::new(0, 1),
    }
}

fn c(words: &str) -> PStage {
    PStage::Call(callw(words.split(' ').map(lit).collect()))
}

fn pl(stages: Vec<PStage>) -> PPipeline {
    PPipeline {
        negated: false,
        stages,
    }
}

fn one(s: PStage) -> PAndOr {
    PAndOr {
        first: pl(vec![s]),
        rest: vec![],
    }
}

fn seq(stages: Vec<PStage>) -> Vec<PAndOr> {
    stages.into_iter().map(one).collect()
}

fn plan(body: Vec<PAndOr>) -> Plan {
    Plan {
        schema: PLAN_SCHEMA.into(),
        source_sha256: String::new(),
        body,
        grants: Grants::default(),
        warnings: vec![],
    }
}

fn link(first: PStage, op: AndOrOp, second: PStage) -> Plan {
    plan(vec![PAndOr {
        first: pl(vec![first]),
        rest: vec![(op, pl(vec![second]))],
    }])
}

// ---- named plans (shared by behaviour, determinism, coverage) ---------------

fn p_single() -> Plan {
    plan(seq(vec![c("echo hello")]))
}
fn p_seq() -> Plan {
    plan(seq(vec![c("echo a"), c("echo b")]))
}
fn p_and() -> Plan {
    link(c("test a"), AndOrOp::And, c("echo b"))
}
fn p_or() -> Plan {
    link(c("test a"), AndOrOp::Or, c("echo b"))
}
fn p_not() -> Plan {
    plan(vec![PAndOr {
        first: PPipeline {
            negated: true,
            stages: vec![c("test a")],
        },
        rest: vec![],
    }])
}
fn p_pipe3() -> Plan {
    plan(vec![PAndOr {
        first: pl(vec![c("cat f"), c("sort"), c("wc -l")]),
        rest: vec![],
    }])
}
fn p_exit() -> Plan {
    plan(seq(vec![c("echo a"), c("exit 3"), c("echo b")]))
}
fn p_assign() -> Plan {
    plan(seq(vec![PStage::Call(Call {
        tool: None,
        argv: vec![],
        assignments: vec![("X".into(), lit("1")), ("Y".into(), var("X"))],
        redirects: vec![],
        span: Span::new(0, 7),
    })]))
}
fn p_for() -> Plan {
    plan(seq(vec![PStage::For {
        var: "i".into(),
        items: vec![lit("a"), lit("b"), lit("c")],
        body: seq(vec![PStage::Call(callw(vec![lit("echo"), var("i")]))]),
    }]))
}
fn p_while() -> Plan {
    plan(seq(vec![PStage::While {
        until: false,
        condition: seq(vec![c("test x")]),
        body: seq(vec![c("echo loop")]),
    }]))
}
fn p_until() -> Plan {
    plan(seq(vec![PStage::While {
        until: true,
        condition: seq(vec![c("test x")]),
        body: seq(vec![c("echo loop")]),
    }]))
}
fn p_capture() -> Plan {
    plan(seq(vec![PStage::Call(callw(vec![
        lit("echo"),
        cap(seq(vec![c("cat f")])),
    ]))]))
}
fn p_redirects() -> Plan {
    let mut call = callw(vec![lit("cat")]);
    call.redirects = vec![
        PRedirect {
            fd: 0,
            op: RedirectOp::Read,
            target: PRedirectTarget::Word { word: lit("in") },
        },
        PRedirect {
            fd: 1,
            op: RedirectOp::Write,
            target: PRedirectTarget::Word { word: lit("out") },
        },
        PRedirect {
            fd: 1,
            op: RedirectOp::Append,
            target: PRedirectTarget::Word { word: var("LOG") },
        },
        PRedirect {
            fd: 2,
            op: RedirectOp::DupOut,
            target: PRedirectTarget::Fd { fd: 1 },
        },
        PRedirect {
            fd: 0,
            op: RedirectOp::DupIn,
            target: PRedirectTarget::Fd { fd: 3 },
        },
        PRedirect {
            fd: 0,
            op: RedirectOp::Heredoc,
            target: PRedirectTarget::Heredoc {
                body: PWord {
                    parts: vec![PPart::Lit {
                        text: "body\n".into(),
                        quoted: true,
                    }],
                },
            },
        },
    ];
    plan(seq(vec![PStage::Call(call)]))
}
fn p_nested_capture() -> Plan {
    // echo x | grep $(cat p | head -n1)
    let inner = vec![PAndOr {
        first: pl(vec![c("cat p"), c("head -n1")]),
        rest: vec![],
    }];
    let grep = PStage::Call(callw(vec![lit("grep"), cap(inner)]));
    plan(vec![PAndOr {
        first: pl(vec![c("echo x"), grep]),
        rest: vec![],
    }])
}
fn p_glob_parts() -> Plan {
    let word = PWord {
        parts: vec![
            PPart::Lit {
                text: "a".into(),
                quoted: true,
            },
            PPart::Var {
                name: "X".into(),
                quoted: true,
            },
            PPart::Glob {
                pattern: "*.txt".into(),
            },
        ],
    };
    plan(seq(vec![PStage::Call(callw(vec![lit("ls"), word]))]))
}
fn p_capture_assign_redirect() -> Plan {
    // X=$(echo v) cat > $(echo out) — captures run in order: argv, redirects, assignments
    let mut call = callw(vec![lit("cat")]);
    call.redirects = vec![PRedirect {
        fd: 1,
        op: RedirectOp::Write,
        target: PRedirectTarget::Word {
            word: cap(seq(vec![c("echo out")])),
        },
    }];
    call.assignments = vec![("X".into(), cap(seq(vec![c("echo v")])))];
    plan(seq(vec![PStage::Call(call)]))
}

fn all_plans() -> Vec<(&'static str, Plan)> {
    vec![
        ("single", p_single()),
        ("seq", p_seq()),
        ("and", p_and()),
        ("or", p_or()),
        ("not", p_not()),
        ("pipe3", p_pipe3()),
        ("exit", p_exit()),
        ("assign", p_assign()),
        ("for", p_for()),
        ("while", p_while()),
        ("until", p_until()),
        ("capture", p_capture()),
        ("redirects", p_redirects()),
        ("nested_capture", p_nested_capture()),
        ("glob_parts", p_glob_parts()),
        ("capture_assign_redirect", p_capture_assign_redirect()),
        ("kw1", p_kw1()),
        ("kw2", p_kw2()),
        ("kw4", p_kw4()),
        ("empty", plan(vec![])),
    ]
}

// ---- recording mock host ----------------------------------------------------

#[derive(Default)]
struct Mock {
    log: Vec<String>,
    /// Statuses per argv[0], consumed in order; 0 when exhausted.
    script: BTreeMap<String, VecDeque<u32>>,
    pipes: u32,
    lists: u32,
}

fn render(w: &[Part]) -> String {
    w.iter()
        .map(|p| match p {
            Part::Lit(l) if l.quoted => format!("\"{}\"", l.text),
            Part::Lit(l) => l.text.clone(),
            Part::Var(v) if v.quoted => format!("\"${}\"", v.name),
            Part::Var(v) => format!("${}", v.name),
            Part::Glob(g) => g.clone(),
            Part::Captured(c) => format!("<cap{}>", c.pipe),
        })
        .collect()
}

impl wit::Host for Mock {
    fn pipe(&mut self) -> u32 {
        self.pipes += 1;
        self.log.push(format!("pipe -> {}", self.pipes));
        self.pipes
    }

    fn call(
        &mut self,
        argv: Vec<wit::Word>,
        assignments: Vec<(String, wit::Word)>,
        redirects: Vec<wit::Redirect>,
        stdin: u32,
        stdout: u32,
    ) -> u32 {
        let words: Vec<String> = argv.iter().map(|w| render(w)).collect();
        let mut line = format!("call {} <{stdin} >{stdout}", words.join(" "));
        for (n, v) in &assignments {
            line.push_str(&format!(" {n}={}", render(v)));
        }
        for r in &redirects {
            let op = match r.op {
                WOp::Read => "<",
                WOp::Write => ">",
                WOp::Append => ">>",
                WOp::DupOut => ">&",
                WOp::DupIn => "<&",
                WOp::Heredoc => "<<",
            };
            line.push_str(&format!(
                " {}{op}[{}]{}",
                r.fd,
                render(&r.target),
                r.target_fd
            ));
        }
        self.log.push(line);
        self.script
            .get_mut(&words[0])
            .and_then(|q| q.pop_front())
            .unwrap_or(0)
    }

    fn set_var(&mut self, name: String, value: wit::Word) {
        self.log.push(format!("set-var {name}={}", render(&value)));
    }

    fn set_status(&mut self, status: u32) {
        self.log.push(format!("set-status {status}"));
    }

    fn expand_list(&mut self, words: Vec<wit::Word>) -> (u32, u32) {
        self.lists += 1;
        let ws: Vec<String> = words.iter().map(|w| render(w)).collect();
        self.log
            .push(format!("expand-list {} -> {}", ws.join(" "), self.lists));
        (self.lists, words.len() as u32)
    }

    fn bind_item(&mut self, name: String, list: u32, index: u32) {
        self.log.push(format!("bind-item {name} {list} {index}"));
    }
}

fn engine() -> &'static wasmtime::Engine {
    static E: OnceLock<wasmtime::Engine> = OnceLock::new();
    E.get_or_init(|| {
        let mut cfg = wasmtime::Config::new();
        cfg.wasm_component_model(true);
        wasmtime::Engine::new(&cfg).unwrap()
    })
}

/// Compiles and runs a plan under the mock; returns the log and `run`'s result.
fn run(p: &Plan, script: &[(&str, &[u32])]) -> (Vec<String>, u32) {
    let bytes = compile(p).unwrap_or_else(|e| panic!("compile: {e}"));
    let component = wasmtime::component::Component::new(engine(), &bytes).unwrap();
    let mut linker = wasmtime::component::Linker::<Mock>::new(engine());
    bindings::Plan::add_to_linker::<Mock, wasmtime::component::HasSelf<Mock>>(&mut linker, |s| s)
        .unwrap();
    let mut mock = Mock::default();
    for (name, sts) in script {
        mock.script
            .insert((*name).into(), sts.iter().copied().collect());
    }
    let mut store = wasmtime::Store::new(engine(), mock);
    let inst = bindings::Plan::instantiate(&mut store, &component, &linker).unwrap();
    let status = inst.call_run(&mut store).unwrap();
    (std::mem::take(&mut store.data_mut().log), status)
}

fn check(p: &Plan, script: &[(&str, &[u32])], want_log: &[&str], want_status: u32) {
    let (log, status) = run(p, script);
    assert_eq!(log, want_log, "call sequence");
    assert_eq!(status, want_status, "run() result");
}

// ---- behaviour --------------------------------------------------------------

#[test]
fn single_call() {
    check(
        &p_single(),
        &[],
        &["call echo hello <0 >0", "set-status 0"],
        0,
    );
}

#[test]
fn sequence() {
    check(
        &p_seq(),
        &[("echo", &[4, 0])],
        &[
            "call echo a <0 >0",
            "set-status 4",
            "call echo b <0 >0",
            "set-status 0",
        ],
        0,
    );
}

#[test]
fn and_ok_runs_second() {
    check(
        &p_and(),
        &[("test", &[0]), ("echo", &[5])],
        &[
            "call test a <0 >0",
            "set-status 0",
            "call echo b <0 >0",
            "set-status 5",
        ],
        5,
    );
}

#[test]
fn and_fail_skips_second() {
    check(
        &p_and(),
        &[("test", &[1])],
        &["call test a <0 >0", "set-status 1"],
        1,
    );
}

#[test]
fn or_ok_skips_second() {
    check(
        &p_or(),
        &[("test", &[0])],
        &["call test a <0 >0", "set-status 0"],
        0,
    );
}

#[test]
fn or_fail_runs_second() {
    check(
        &p_or(),
        &[("test", &[2])],
        &[
            "call test a <0 >0",
            "set-status 2",
            "call echo b <0 >0",
            "set-status 0",
        ],
        0,
    );
}

#[test]
fn not_inverts_zero_and_nonzero() {
    check(
        &p_not(),
        &[("test", &[0])],
        &["call test a <0 >0", "set-status 1"],
        1,
    );
    check(
        &p_not(),
        &[("test", &[7])],
        &["call test a <0 >0", "set-status 0"],
        0,
    );
}

#[test]
fn pipeline_three_stages_wiring() {
    check(
        &p_pipe3(),
        &[("wc", &[9])],
        &[
            "pipe -> 1",
            "pipe -> 2",
            "call cat f <0 >1",
            "call sort <1 >2",
            "call wc -l <2 >0",
            "set-status 9",
        ],
        9,
    );
}

#[test]
fn exit_mid_script_stops_and_returns_n() {
    check(
        &p_exit(),
        &[("exit", &[256 + 3])],
        &["call echo a <0 >0", "set-status 0", "call exit 3 <0 >0"],
        3,
    );
}

#[test]
fn assignment_only() {
    check(
        &p_assign(),
        &[],
        &["set-var X=1", "set-var Y=$X", "set-status 0"],
        0,
    );
}

#[test]
fn for_loop_three_items() {
    check(
        &p_for(),
        &[("echo", &[0, 0, 6])],
        &[
            "expand-list a b c -> 1",
            "bind-item i 1 0",
            "call echo $i <0 >0",
            "set-status 0",
            "bind-item i 1 1",
            "call echo $i <0 >0",
            "set-status 0",
            "bind-item i 1 2",
            "call echo $i <0 >0",
            "set-status 6",
            "set-status 6",
        ],
        6,
    );
}

#[test]
fn while_loop_stops_after_two_iterations() {
    check(
        &p_while(),
        &[("test", &[0, 0, 1]), ("echo", &[0, 4])],
        &[
            "call test x <0 >0",
            "set-status 0",
            "call echo loop <0 >0",
            "set-status 0",
            "call test x <0 >0",
            "set-status 0",
            "call echo loop <0 >0",
            "set-status 4",
            "call test x <0 >0",
            "set-status 1",
            "set-status 4",
        ],
        4,
    );
}

#[test]
fn while_loop_never_runs_status_zero() {
    check(
        &p_while(),
        &[("test", &[1])],
        &["call test x <0 >0", "set-status 1", "set-status 0"],
        0,
    );
}

#[test]
fn until_loop_runs_until_success() {
    check(
        &p_until(),
        &[("test", &[1, 0])],
        &[
            "call test x <0 >0",
            "set-status 1",
            "call echo loop <0 >0",
            "set-status 0",
            "call test x <0 >0",
            "set-status 0",
            "set-status 0",
        ],
        0,
    );
}

#[test]
fn capture_echo_cat() {
    check(
        &p_capture(),
        &[],
        &[
            "pipe -> 1",
            "call cat f <0 >1",
            "set-status 0",
            "call echo <cap1> <0 >0",
            "set-status 0",
        ],
        0,
    );
}

#[test]
fn redirect_list() {
    check(
        &p_redirects(),
        &[],
        &[
            "call cat <0 >0 0<[in]0 1>[out]0 1>>[$LOG]0 2>&[]1 0<&[]3 0<<[\"body\n\"]0",
            "set-status 0",
        ],
        0,
    );
}

#[test]
fn nested_capture_inside_pipeline_stage() {
    check(
        &p_nested_capture(),
        &[],
        &[
            "pipe -> 1",
            "call echo x <0 >1",
            "pipe -> 2",
            "pipe -> 3",
            "call cat p <1 >3",
            "call head -n1 <3 >2",
            "set-status 0",
            "call grep <cap2> <1 >0",
            "set-status 0",
        ],
        0,
    );
}

#[test]
fn word_parts_quoted_var_glob() {
    check(
        &p_glob_parts(),
        &[],
        &["call ls \"a\"\"$X\"*.txt <0 >0", "set-status 0"],
        0,
    );
}

#[test]
fn captures_run_in_posix_order() {
    check(
        &p_capture_assign_redirect(),
        &[],
        &[
            "pipe -> 1",
            "call echo out <0 >1",
            "set-status 0",
            "pipe -> 2",
            "call echo v <0 >2",
            "set-status 0",
            "call cat <0 >0 X=<cap2> 1>[<cap1>]0",
            "set-status 0",
        ],
        0,
    );
}

#[test]
fn assignment_status_is_last_capture_status() {
    let p = plan(seq(vec![PStage::Call(Call {
        tool: None,
        argv: vec![],
        assignments: vec![("X".into(), cap(seq(vec![c("false")])))],
        redirects: vec![],
        span: Span::new(0, 1),
    })]));
    check(
        &p,
        &[("false", &[1])],
        &[
            "pipe -> 1",
            "call false <0 >1",
            "set-status 1",
            "set-var X=<cap1>",
            "set-status 1",
        ],
        1,
    );
}

#[test]
fn exit_inside_loop_stops() {
    let p = plan(seq(vec![
        PStage::For {
            var: "i".into(),
            items: vec![lit("a"), lit("b")],
            body: seq(vec![c("exit 4")]),
        },
        c("echo after"),
    ]));
    check(
        &p,
        &[("exit", &[260])],
        &[
            "expand-list a b -> 1",
            "bind-item i 1 0",
            "call exit 4 <0 >0",
        ],
        4,
    );
}

// ---- known-wrong pins (KNOWN.md) --------------------------------------------

/// KW-1: `echo $(X=2)` sets X in the session (POSIX: only in the subshell).
fn p_kw1() -> Plan {
    let body = seq(vec![PStage::Call(Call {
        tool: None,
        argv: vec![],
        assignments: vec![("X".into(), lit("2"))],
        redirects: vec![],
        span: Span::new(7, 10),
    })]);
    plan(seq(vec![PStage::Call(callw(vec![lit("echo"), cap(body)]))]))
}

#[test]
fn known_wrong_kw1_capture_assignment_reaches_session() {
    check(
        &p_kw1(),
        &[],
        &[
            "pipe -> 1",
            "set-var X=2",
            "set-status 0",
            "call echo <cap1> <0 >0",
            "set-status 0",
        ],
        0,
    );
}

/// KW-2: `echo $(exit 5); echo after` stops the plan with 5.
fn p_kw2() -> Plan {
    plan(seq(vec![
        PStage::Call(callw(vec![lit("echo"), cap(seq(vec![c("exit 5")]))])),
        c("echo after"),
    ]))
}

#[test]
fn known_wrong_kw2_exit_in_capture_stops_plan() {
    check(
        &p_kw2(),
        &[("exit", &[261])],
        &["pipe -> 1", "call exit 5 <0 >1"],
        5,
    );
}

#[test]
fn known_wrong_kw3_status_in_same_command_sees_capture() {
    // false; echo $? $(true): the capture's set-status 0 lands before echo.
    let p = plan(seq(vec![
        c("false"),
        PStage::Call(callw(vec![
            lit("echo"),
            var("?"),
            cap(seq(vec![c("true")])),
        ])),
    ]));
    check(
        &p,
        &[("false", &[1])],
        &[
            "call false <0 >0",
            "set-status 1",
            "pipe -> 1",
            "call true <0 >1",
            "set-status 0",
            "call echo $? <cap1> <0 >0",
            "set-status 0",
        ],
        0,
    );
}

/// KW-4: `exit 3 | cat; echo after` stops the plan with 3.
fn p_kw4() -> Plan {
    plan(vec![
        PAndOr {
            first: pl(vec![c("exit 3"), c("cat")]),
            rest: vec![],
        },
        one(c("echo after")),
    ])
}

#[test]
fn known_wrong_kw4_exit_in_pipeline_stage_stops_plan() {
    check(
        &p_kw4(),
        &[("exit", &[259])],
        &["pipe -> 1", "call exit 3 <0 >1"],
        3,
    );
}

#[test]
fn known_md_counts_are_pinned() {
    let text =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("KNOWN.md")).unwrap();
    let count = |prefix: &str| text.lines().filter(|l| l.starts_with(prefix)).count();
    let header = |key: &str| -> usize {
        text.lines()
            .find_map(|l| l.strip_prefix(key))
            .unwrap_or_else(|| panic!("KNOWN.md has no `{key}` line"))
            .trim()
            .parse()
            .unwrap()
    };
    // Pinned: a change in either direction must update this test, KNOWN.md
    // and the tables in lib.rs together.
    assert_eq!(KNOWN_WRONG.len(), 4);
    assert_eq!(DECLINED.len(), 8);
    assert_eq!(header("Known-wrong count:"), KNOWN_WRONG.len());
    assert_eq!(header("Declined count:"), DECLINED.len());
    assert_eq!(count("### KW-"), KNOWN_WRONG.len());
    assert_eq!(count("### KD-"), DECLINED.len());
    for (id, _) in KNOWN_WRONG.iter().chain(DECLINED) {
        assert!(text.contains(&format!("### {id}:")), "KNOWN.md lacks {id}");
    }
}

// ---- honesty: declines ------------------------------------------------------

fn declined(p: &Plan) -> CompileError {
    match compile(p) {
        Ok(_) => panic!("expected a decline"),
        Err(e) => e,
    }
}

fn assert_kc001(p: &Plan, construct: &str, span: Option<Span>) {
    let e = declined(p);
    assert_eq!(e.code, code::UNSUPPORTED, "{e}");
    assert_eq!(e.construct, construct, "{e}");
    assert_eq!(e.span, span, "{e}");
}

fn raw_call(
    tool: bool,
    argv: Vec<PWord>,
    asg: Vec<(String, PWord)>,
    redirects: Vec<PRedirect>,
) -> Plan {
    plan(seq(vec![PStage::Call(Call {
        tool: tool.then(|| Tool::Builtin("x".into())),
        argv,
        assignments: asg,
        redirects,
        span: Span::new(3, 9),
    })]))
}

const SPAN: Option<Span> = Some(Span { start: 3, end: 9 });

#[test]
fn kd1_empty_pipeline() {
    assert_kc001(
        &plan(vec![PAndOr {
            first: pl(vec![]),
            rest: vec![],
        }]),
        "pipeline",
        None,
    );
}

#[test]
fn kd2_empty_argv() {
    assert_kc001(&raw_call(true, vec![], vec![], vec![]), "call", SPAN);
}

#[test]
fn kd3_dynamic_command_name() {
    assert_kc001(
        &raw_call(true, vec![var("CMD")], vec![], vec![]),
        "call",
        SPAN,
    );
}

#[test]
fn kd4_no_tool_with_argv() {
    assert_kc001(
        &raw_call(false, vec![lit("echo")], vec![], vec![]),
        "call",
        SPAN,
    );
}

#[test]
fn kd5_no_tool_no_assignments() {
    assert_kc001(&raw_call(false, vec![], vec![], vec![]), "call", SPAN);
}

#[test]
fn kd6_redirect_only_command() {
    let r = PRedirect {
        fd: 1,
        op: RedirectOp::Write,
        target: PRedirectTarget::Word { word: lit("f") },
    };
    assert_kc001(
        &raw_call(false, vec![], vec![("X".into(), lit("1"))], vec![r]),
        "call",
        SPAN,
    );
}

#[test]
fn kd7_redirect_target_kind_mismatch() {
    let bad = [
        (RedirectOp::Write, PRedirectTarget::Fd { fd: 1 }),
        (RedirectOp::DupOut, PRedirectTarget::Word { word: lit("f") }),
        (
            RedirectOp::Heredoc,
            PRedirectTarget::Word { word: lit("f") },
        ),
        (
            RedirectOp::Read,
            PRedirectTarget::Heredoc { body: lit("b") },
        ),
    ];
    for (op, target) in bad {
        let r = PRedirect { fd: 1, op, target };
        assert_kc001(
            &raw_call(true, vec![lit("cat")], vec![], vec![r]),
            "redirect",
            SPAN,
        );
    }
}

#[test]
fn kd8_while_empty_condition() {
    let p = plan(seq(vec![PStage::While {
        until: false,
        condition: vec![],
        body: seq(vec![c("true")]),
    }]));
    assert_kc001(&p, "while", None);
}

#[test]
fn decline_inside_capture_carries_enclosing_span() {
    let bad_inner = raw_call(true, vec![var("CMD")], vec![], vec![]).body;
    let mut outer = callw(vec![lit("echo"), cap(bad_inner)]);
    outer.span = Span::new(20, 30);
    // The inner call has its own span; the decline names it.
    assert_kc001(&plan(seq(vec![PStage::Call(outer)])), "call", SPAN);
}

#[test]
fn kc002_wrong_schema() {
    let mut p = p_single();
    p.schema = "kapsel.plan.v9".into();
    let e = declined(&p);
    assert_eq!(e.code, code::SCHEMA);
}

#[test]
fn kc003_too_many_locals() {
    // 50_002 stages need 50_001 pipe locals plus the status local.
    let stages = (0..50_002).map(|_| c("true")).collect();
    let e = declined(&plan(vec![PAndOr {
        first: pl(stages),
        rest: vec![],
    }]));
    assert_eq!(e.code, code::LIMIT, "{e}");
}

#[test]
fn kc900_validator_rejects_component_of_another_plan() {
    let other = compile(&p_seq()).unwrap();
    let e = kapsel_compile::validate(&p_single(), &other).unwrap_err();
    assert_eq!(e.code, code::VALIDATOR, "{e}");
    // Same control shape, different argv: the data-segment check catches it.
    let other = compile(&plan(seq(vec![c("echo goodbye")]))).unwrap();
    let e = kapsel_compile::validate(&p_single(), &other).unwrap_err();
    assert_eq!(e.code, code::VALIDATOR, "{e}");
    assert!(e.message.contains("goodbye"), "{e}");
    // Same calls, different pipe wiring.
    let swapped = plan(vec![PAndOr {
        first: pl(vec![c("cat f"), c("sort"), c("wc -l")]),
        rest: vec![],
    }]);
    let mut wrong = swapped.clone();
    wrong.body[0].first.stages.swap(1, 2);
    let e = kapsel_compile::validate(&swapped, &compile(&wrong).unwrap()).unwrap_err();
    assert_eq!(e.code, code::VALIDATOR, "{e}");
    // Garbage bytes.
    let e = kapsel_compile::validate(&p_single(), b"\0asm garbage").unwrap_err();
    assert_eq!(e.code, code::VALIDATOR, "{e}");
}

#[test]
fn kc900_validator_rejects_wrong_and_or_shape() {
    // `a && b` compiled, checked against `a; b`: the If/End structure differs.
    let e = kapsel_compile::validate(
        &plan(seq(vec![c("test a"), c("echo b")])),
        &compile(&p_and()).unwrap(),
    )
    .unwrap_err();
    assert_eq!(e.code, code::VALIDATOR, "{e}");
}

// ---- determinism and validity -----------------------------------------------

#[test]
fn determinism_same_plan_same_bytes() {
    for (name, p) in all_plans() {
        let a = compile(&p).unwrap_or_else(|e| panic!("{name}: {e}"));
        let b = compile(&p.clone()).unwrap();
        assert_eq!(a, b, "{name}: two compiles differ");
    }
}

#[test]
fn validity_imports_only_host_exports_only_run() {
    for (name, p) in all_plans() {
        let bytes = compile(&p).unwrap();
        let mut features = wasmparser::WasmFeatures::default();
        features.insert(wasmparser::WasmFeatures::COMPONENT_MODEL);
        wasmparser::Validator::new_with_features(features)
            .validate_all(&bytes)
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        let wit_component::DecodedWasm::Component(resolve, world) =
            wit_component::decode(&bytes).unwrap()
        else {
            panic!("{name}: not a component")
        };
        let w = &resolve.worlds[world];
        let imports: Vec<String> = w
            .imports
            .keys()
            .map(|k| resolve.name_world_key(k))
            .collect();
        let exports: Vec<String> = w
            .exports
            .keys()
            .map(|k| resolve.name_world_key(k))
            .collect();
        assert_eq!(imports, ["kapsel:shell/host@0.1.0"], "{name}");
        assert_eq!(exports, ["run"], "{name}");
    }
}

#[test]
fn no_cabi_realloc_exported() {
    let core = kapsel_compile::compile_core(&p_pipe3()).unwrap();
    for p in wasmparser::Parser::new(0).parse_all(&core) {
        if let wasmparser::Payload::ExportSection(r) = p.unwrap() {
            let names: Vec<_> = r.into_iter().map(|e| e.unwrap().name.to_owned()).collect();
            assert_eq!(names, ["memory", "run"]);
        }
    }
}

#[test]
fn component_size_three_stage_pipeline() {
    let n = compile(&p_pipe3()).unwrap().len();
    eprintln!("3-stage pipeline component: {n} bytes");
    assert!(n < 64 * 1024, "{n} bytes");
}

// ---- rule coverage ----------------------------------------------------------

/// Each rule id → the named plan that exercises it (and the behaviour test
/// that runs that plan).
const RULE_TESTS: &[(&str, &str)] = &[
    ("KC-R01", "seq"),
    ("KC-R02", "and"),
    ("KC-R03", "or"),
    ("KC-R04", "single"),
    ("KC-R05", "pipe3"),
    ("KC-R06", "not"),
    ("KC-R07", "single"),
    ("KC-R08", "assign"),
    ("KC-R09", "for"),
    ("KC-R10", "while"),
    ("KC-R11", "until"),
    ("KC-R12", "capture"),
    ("KC-R13", "redirects"),
    ("KC-R14", "redirects"),
    ("KC-R15", "redirects"),
    ("KC-R16", "single"),
    ("KC-R17", "glob_parts"),
    ("KC-R18", "glob_parts"),
];

#[test]
fn every_rule_has_a_test_that_fires_it() {
    let plans: BTreeMap<_, _> = all_plans().into_iter().collect();
    for r in RULES {
        let Some((_, name)) = RULE_TESTS.iter().find(|(id, _)| *id == r.id) else {
            panic!("rule {} ({}) has no test", r.id, r.name)
        };
        let fired = compile_report(&plans[name]).unwrap().rules_fired;
        assert!(
            fired.contains(&r.id),
            "plan `{name}` does not fire {} ({}): {fired:?}",
            r.id,
            r.name
        );
    }
    assert_eq!(
        RULE_TESTS.len(),
        RULES.len(),
        "RULE_TESTS names a rule that is not in RULES"
    );
}

// ---- frozen byte fixtures ---------------------------------------------------

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/kapsel/compile")
}

#[test]
fn frozen_fixtures_match_committed_sha256() {
    use sha2::Digest;
    let bless = std::env::var("KAPSEL_BLESS").is_ok_and(|v| v == "1");
    let mut paths: Vec<_> = std::fs::read_dir(fixture_dir())
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.to_string_lossy().ends_with(".plan.json"))
        .collect();
    paths.sort();
    assert!(
        paths.len() >= 8,
        "expected at least 8 fixtures, found {}",
        paths.len()
    );
    let mut failures = Vec::new();
    for path in paths {
        let p: Plan = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let bytes = compile(&p).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let got = format!("{:x}\n", sha2::Sha256::digest(&bytes));
        let sha_path = PathBuf::from(path.to_string_lossy().replace(".plan.json", ".sha256"));
        if bless {
            std::fs::write(&sha_path, &got).unwrap();
            continue;
        }
        let want = std::fs::read_to_string(&sha_path).unwrap_or_default();
        if want != got {
            failures.push(format!(
                "{}: committed {}, compiled {}",
                path.display(),
                want.trim(),
                got.trim()
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "byte change (bless with KAPSEL_BLESS=1):\n{}",
        failures.join("\n")
    );
}

/// Writes the fixture plans; run once with `--ignored` when adding fixtures.
#[test]
#[ignore]
fn write_fixture_plans() {
    for (name, p) in all_plans() {
        if name == "empty" || name.starts_with("kw") {
            continue;
        }
        let path = fixture_dir().join(format!("{name}.plan.json"));
        std::fs::write(path, serde_json::to_string_pretty(&p).unwrap() + "\n").unwrap();
    }
}

// ---- cache key: bytes depend only on the normalized plan --------------------

/// Gives every call a distinct span, sets a source hash and adds a warning.
fn decorate(mut p: Plan) -> Plan {
    fn word(w: &mut PWord, n: &mut u32) {
        for part in &mut w.parts {
            if let PPart::Capture { body, .. } = part {
                body.iter_mut().for_each(|a| and_or(a, n));
            }
        }
    }
    fn and_or(a: &mut PAndOr, n: &mut u32) {
        pipeline(&mut a.first, n);
        for (_, p) in &mut a.rest {
            pipeline(p, n);
        }
    }
    fn pipeline(p: &mut PPipeline, n: &mut u32) {
        for s in &mut p.stages {
            match s {
                PStage::Call(c) => {
                    *n += 7;
                    c.span = Span::new(*n, *n + 5);
                    c.argv.iter_mut().for_each(|w| word(w, n));
                    c.assignments.iter_mut().for_each(|(_, w)| word(w, n));
                    for r in &mut c.redirects {
                        if let PRedirectTarget::Word { word: w } = &mut r.target {
                            word(w, n);
                        }
                    }
                }
                PStage::For { items, body, .. } => {
                    items.iter_mut().for_each(|w| word(w, n));
                    body.iter_mut().for_each(|a| and_or(a, n));
                }
                PStage::While {
                    condition, body, ..
                } => condition
                    .iter_mut()
                    .chain(body.iter_mut())
                    .for_each(|a| and_or(a, n)),
            }
        }
    }
    let mut n = 100;
    p.body.iter_mut().for_each(|a| and_or(a, &mut n));
    p.source_sha256 = "ab".repeat(32);
    p.warnings.push(kapsel_plan::Finding {
        rule: "KS-R002".into(),
        severity: kapsel_plan::Severity::Warn,
        message: "a warning".into(),
        span: Span::new(1, 2),
        excerpt: "2>/dev/null".into(),
        help: Some("handle the failure".into()),
        notes: vec![],
        related: vec![],
    });
    p
}

#[test]
fn bytes_equal_compile_of_normalized_plan() {
    for (name, p) in all_plans() {
        let a = compile(&p).unwrap();
        assert_eq!(
            a,
            compile(&p.normalized()).unwrap(),
            "{name}: differs from normalized"
        );
        let d = decorate(p.clone());
        assert_ne!(d, p, "{name}: decorate changed nothing");
        assert_eq!(
            d.normalized(),
            p.normalized(),
            "{name}: decorate changed more than spans"
        );
        assert_eq!(
            compile(&d).unwrap(),
            a,
            "{name}: spans/source/warnings reached the module"
        );
    }
}

#[test]
fn grants_are_never_compiled_in() {
    let mut p = p_redirects();
    p.grants.write = vec!["secret-path".into()];
    p.grants.read = vec!["secret-read".into()];
    p.grants.dynamic = true;
    let bytes = compile(&p).unwrap();
    for needle in [&b"secret-path"[..], b"secret-read"] {
        assert!(
            !bytes.windows(needle.len()).any(|w| w == needle),
            "grant text in module"
        );
    }
    let mut no_grants = p.clone();
    no_grants.grants = Grants::default();
    assert_eq!(
        bytes,
        compile(&no_grants).unwrap(),
        "grants changed the module"
    );
}
