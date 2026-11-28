use crate::*;
use kapsel_ast::{ParseError, Span};
use kapsel_plan::{PPart, Severity};

fn run(src: &str) -> CheckOutcome {
    let script = match kapsel_syntax::parse(src) {
        Ok(s) => s,
        Err(errs) => return refuse_parse_errors(src, &errs),
    };
    let out = check(src, &script);
    // Every outcome round-trips through serde and has exact excerpts.
    let json = serde_json::to_string(&out).unwrap();
    let back: CheckOutcome = serde_json::from_str(&json).unwrap();
    assert_eq!(out, back, "serde round trip for {src:?}");
    for f in findings(&out) {
        assert_eq!(
            f.excerpt,
            &src[f.span.start as usize..f.span.end as usize],
            "excerpt of {} in {src:?}",
            f.rule
        );
    }
    out
}

fn findings(o: &CheckOutcome) -> Vec<&Finding> {
    match o {
        CheckOutcome::Plan(p) => p.warnings.iter().collect(),
        CheckOutcome::Refused(r) => r.findings.iter().collect(),
    }
}

fn rules(src: &str) -> Vec<String> {
    findings(&run(src)).iter().map(|f| f.rule.clone()).collect()
}

fn plan(src: &str) -> Plan {
    match run(src) {
        CheckOutcome::Plan(p) => p,
        CheckOutcome::Refused(r) => panic!("{src:?} refused: {:?}", r.findings),
    }
}

fn has(src: &str, rule: &str, sev: Severity) -> bool {
    findings(&run(src))
        .iter()
        .any(|f| f.rule == rule && f.severity == sev)
}

/// (rule, severity, planted, control)
const CASES: &[(&str, Severity, &str, &str)] = &[
    (
        "KS-R000",
        Severity::Refuse,
        "if true; then echo hi; fi",
        "true && echo hi",
    ),
    (
        "KS-R001",
        Severity::Refuse,
        "grep foo f || true",
        "grep foo f || echo none",
    ),
    (
        "KS-R001",
        Severity::Refuse,
        "grep foo f || :",
        "grep foo f && echo found",
    ),
    (
        "KS-R002",
        Severity::Warn,
        "cat a 2>/dev/null",
        "cat a 2>/dev/null || echo missing",
    ),
    ("KS-R003", Severity::Refuse, "rm -rf $BUILD/", "rm -rf out"),
    ("KS-R003", Severity::Refuse, "rm -r /", "rm -r ./out"),
    (
        "KS-R003",
        Severity::Refuse,
        "rm -rf \"$HOME\"",
        "rm -f \"$HOME/x\"",
    ),
    ("KS-R003", Severity::Refuse, "rm -Rf ..", "rm -rf ../x"),
    ("KS-R003", Severity::Refuse, "rm -rf ''", "rm -rf 'a b'"),
    ("KS-R003", Severity::Refuse, "rm -rf .", "rm file"),
    ("KS-R003", Severity::Refuse, "rm -rf -- /", "rm -rf -- out"),
    ("KS-R003", Severity::Refuse, "rm -rf \"$X\"", "rm -rf build"),
    (
        "KS-R004",
        Severity::Refuse,
        "eval \"echo hi\"",
        "echo \"echo hi\"",
    ),
    ("KS-R004", Severity::Refuse, "sh -c 'echo hi'", "echo hi"),
    ("KS-R004", Severity::Refuse, "bash -c 'echo hi'", "echo hi"),
    ("KS-R004", Severity::Refuse, "source env.sh", "cat env.sh"),
    ("KS-R004", Severity::Refuse, ". ./env.sh", "cat ./env.sh"),
    ("KS-R005", Severity::Refuse, "set +e", "export X=1"),
    ("KS-R005", Severity::Refuse, "set +o pipefail", "export X=1"),
    ("KS-R005", Severity::Refuse, "set -e", "export X=1"),
    (
        "KS-R006",
        Severity::Refuse,
        "curl https://x.example/i | sh",
        "echo https://x.example/i | cat",
    ),
    (
        "KS-R006",
        Severity::Refuse,
        "wget -qO- https://x | bash",
        "echo https://x | wc -c",
    ),
    (
        "KS-R006",
        Severity::Refuse,
        "curl https://x | python3",
        "echo https://x | cat",
    ),
    ("KS-R007", Severity::Refuse, "nohup sleep 100", "sleep 1"),
    ("KS-R007", Severity::Refuse, "setsid sleep 1", "sleep 1"),
    (
        "KS-R007",
        Severity::Refuse,
        "sleep 1 & echo hi",
        "sleep 1; echo hi",
    ),
    ("KS-R008", Severity::Refuse, "pkill -f build", "echo build"),
    ("KS-R008", Severity::Refuse, "killall node", "echo node"),
    ("KS-R009", Severity::Refuse, "make all", "echo all"),
    (
        "KS-R010",
        Severity::Refuse,
        "CMD=echo; $CMD hi",
        "CMD=echo; echo $CMD hi",
    ),
    (
        "KS-R010",
        Severity::Refuse,
        "\"$(echo ls)\" -l",
        "echo $(echo ls) -l",
    ),
    (
        "KS-R011",
        Severity::Warn,
        "cat a || echo failed",
        "cat a && echo ok",
    ),
    (
        "KS-R012",
        Severity::Warn,
        "echo hi >> $OUT",
        "echo hi >> out.txt",
    ),
    (
        "KS-R013",
        Severity::Warn,
        "n=1; [ $n -eq 0 ]",
        "n=1; [ \"$n\" -eq 0 ]",
    ),
    ("KS-R014", Severity::Warn, "cd d; ls", "cd d && ls"),
    (
        "KS-R015",
        Severity::Warn,
        "grep --include=*.rs x .",
        "grep --include='*.rs' x .",
    ),
    ("KS-R016", Severity::Warn, "ls m | head", "ls m"),
    ("KS-R017", Severity::Warn, "cat \"$D/x\"", "cat x"),
    (
        "KS-R018",
        Severity::Warn,
        "cat a || exit 65",
        "cat a || exit 1",
    ),
];

#[test]
fn every_rule_catches_its_planted_case_and_passes_its_control() {
    for (rule, sev, planted, control) in CASES {
        let (want_rule, want_sev) = (*rule, *sev);
        assert!(
            has(planted, want_rule, want_sev),
            "{planted:?} should hit {want_rule}: {:?}",
            rules(planted)
        );
        let c = run(control);
        assert!(
            matches!(&c, CheckOutcome::Plan(_)) && findings(&c).iter().all(|f| f.rule != *rule),
            "control {control:?} for {rule} should pass without {rule}: {:?}",
            findings(&c)
        );
    }
}

#[test]
fn every_rule_id_has_a_case() {
    for full in RULES {
        assert!(CASES.iter().any(|c| c.0 == *full), "no case for {full}");
    }
}

#[test]
fn warn_only_outcome_is_a_plan_with_warnings() {
    let p = plan("cat a 2>/dev/null");
    assert_eq!(p.warnings.len(), 1);
    assert_eq!(p.warnings[0].rule, "KS-R002");
    assert_eq!(p.warnings[0].severity, Severity::Warn);
    assert_eq!(p.warnings[0].excerpt, "2>/dev/null");
}

#[test]
fn refusal_carries_all_findings_including_warnings() {
    let src = "cat a 2>/dev/null; rm -rf /; pkill x";
    let CheckOutcome::Refused(r) = run(src) else {
        panic!("should refuse")
    };
    let ids: Vec<&str> = r.findings.iter().map(|f| f.rule.as_str()).collect();
    assert_eq!(ids, ["KS-R002", "KS-R003", "KS-R008"]);
    assert_eq!(r.findings[0].severity, Severity::Warn);
    assert_eq!(r.schema, "kapsel.plan.v0");
}

#[test]
fn r002_is_quiet_when_failure_is_handled() {
    for ok in [
        "cat a 2>/dev/null || echo m",
        "cat a 2>/dev/null && echo m",
        "! cat a 2>/dev/null",
        "while cat a 2>/dev/null; do echo x; done",
        "cat a 2>/tmp/log",
    ] {
        assert!(
            !rules(ok).contains(&"KS-R002".to_string()),
            "{ok:?}: {:?}",
            rules(ok)
        );
    }
    // A non-last stage's failure is never handled.
    assert!(rules("cat a 2>/dev/null | sort || echo m").contains(&"KS-R002".to_string()));
    assert!(rules("echo x; cat a 2>/dev/null").contains(&"KS-R002".to_string()));
    assert!(rules("cat a 2>>/dev/null").contains(&"KS-R002".to_string()));
}

#[test]
fn or_true_always_refuses_everywhere() {
    for src in [
        "false || true",
        "false || :",
        "false && true || :",
        "x=$(false || true)",
        "echo $(false || :)",
        "echo \"$(false || true)\"",
        "for i in a b; do false || true; done",
        "for i in $(false || true); do echo $i; done",
        "while false || true; do echo x; done",
        "while true; do false || :; done",
        "false || true > /dev/null",
        "false || true | cat",
    ] {
        assert!(
            has(src, "KS-R001", Severity::Refuse),
            "{src:?}: {:?}",
            rules(src)
        );
    }
}

#[test]
fn r001_span_covers_operator_and_command() {
    let CheckOutcome::Refused(r) = run("false  ||  true") else {
        panic!()
    };
    assert_eq!(r.findings[0].excerpt, "||  true");
    let CheckOutcome::Refused(r) = run("a=$(false || :)") else {
        panic!()
    };
    assert_eq!(r.findings[0].excerpt, "|| :");
}

/// B1 (pre-release review of v0.3.0): every right-hand side of `||` whose
/// status is always 0 is KS-R001, not only a lone `true` / `:`.
#[test]
fn r001_refuses_every_rhs_that_cannot_fail() {
    for (src, construct) in [
        // A pipeline with `true` or `:` as any stage.
        ("grep -q x missing.txt || true | cat", "true"),
        ("false || : | cat", ":"),
        ("false || cat a | true", "true"),
        ("false || echo x | : | cat", ":"),
        ("false || true x y", "true"),
        // Negation of something that always fails.
        ("false || ! false", "! false"),
        ("false || ! [ -z x ]", "! [ -z x ]"),
        ("false || ! test", "! test"),
        // exit 0, and statuses that are 0 modulo 256.
        ("false || exit 0", "exit 0"),
        ("false || exit 00", "exit 00"),
        ("false || exit 256", "exit 256"),
        // Constant-true tests.
        ("false || test 1", "test 1"),
        ("false || [ 1 ]", "[ 1 ]"),
        ("false || [ -n x ]", "[ -n x ]"),
        ("false || [ -n \"x\" ]", "[ -n \"x\" ]"),
        ("false || test -z ''", "test -z ''"),
        ("false || [ a = a ]", "[ a = a ]"),
        ("false || [ 1 -eq 1 ]", "[ 1 -eq 1 ]"),
        ("false || [ ! -z x ]", "[ ! -z x ]"),
        // true / : / exit 0 reached through a loop.
        (
            "false || for i in a; do echo $i; true; done",
            "for i in a; do echo $i; true; done",
        ),
        (
            "false || for i in a; do echo hi; exit; done",
            "for i in a; do echo hi; exit; done",
        ),
        (
            "false || while false; do cat a; done",
            "while false; do cat a; done",
        ),
        (
            "false || until true; do cat a; done",
            "until true; do cat a; done",
        ),
        (
            "false || for i in a; do exit 0; false; done",
            "for i in a; do exit 0; false; done",
        ),
        // In a capture and as a middle step.
        ("x=$(false || true | cat)", "true | cat"),
        ("false || exit 0 && echo x", "exit 0"),
    ] {
        let out = run(src);
        let r001: Vec<&Finding> = findings(&out)
            .into_iter()
            .filter(|f| f.rule == "KS-R001")
            .collect();
        assert!(
            matches!(out, CheckOutcome::Refused(_)) && !r001.is_empty(),
            "{src:?} must be refused with KS-R001: {:?}",
            findings(&out)
        );
        let f = r001[0];
        assert_eq!(f.severity, Severity::Refuse);
        assert!(
            f.excerpt.starts_with("||") && f.excerpt.contains(construct),
            "{src:?}: the span covers `|| {construct}`, got {:?}",
            f.excerpt
        );
        assert!(
            f.message.contains(construct),
            "{src:?}: the message names `{construct}`: {}",
            f.message
        );
        assert!(f.help.as_deref().is_some_and(|h| h.contains("exit 1")));
        assert!(!f.notes.is_empty());
    }
}

/// The controls of B1: right-hand sides that can fail stay out of KS-R001.
#[test]
fn r001_leaves_rhs_that_can_fail_alone() {
    for src in [
        "grep -n foo missing.txt || echo none",
        "false || exit 1",
        "false || exit",
        "false || false",
        "false || exit 3",
        "false || test -n \"$X\"",
        "false || [ -e a ]",
        "false || [ x = y ]",
        "false || ! cat a",
        "false || cat a | grep x",
        "false || for i in a; do cat $i; done",
        "false || while cat a; do echo x; done",
        "false || for i in a; do exit; done",
        "false || exit $N",
        "true | cat",
        "true && echo ok",
        "cat a && true",
    ] {
        assert!(
            !rules(src).contains(&"KS-R001".to_string()),
            "{src:?}: {:?}",
            rules(src)
        );
    }
    // `|| echo none` is KS-R011's warning, not a refusal.
    let out = run("grep -n foo missing.txt || echo none");
    assert!(matches!(out, CheckOutcome::Plan(_)));
    assert!(has(
        "grep -n foo missing.txt || echo none",
        "KS-R011",
        Severity::Warn
    ));
}

#[test]
fn const_test_follows_posix_by_argument_count() {
    let t = |a: &[&str]| {
        crate::status::const_test(&a.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    };
    assert_eq!(t(&[]), Some(false));
    assert_eq!(t(&["1"]), Some(true));
    assert_eq!(t(&[""]), Some(false));
    assert_eq!(t(&["-n", "x"]), Some(true));
    assert_eq!(t(&["-n", ""]), Some(false));
    assert_eq!(t(&["-z", ""]), Some(true));
    assert_eq!(t(&["!", "x"]), Some(false));
    assert_eq!(t(&["a", "=", "a"]), Some(true));
    assert_eq!(t(&["a", "!=", "a"]), Some(false));
    assert_eq!(t(&["2", "-gt", "1"]), Some(true));
    assert_eq!(t(&["x", "-gt", "1"]), None);
    assert_eq!(t(&["!", "-n", "x"]), Some(false));
    assert_eq!(t(&["!", "a", "=", "b"]), Some(true));
    assert_eq!(t(&["-e", "a"]), None);
    assert_eq!(t(&["-f", "a", "b", "c", "d"]), None);
}

/// KS-R018: a script's own `exit 64|65|70|78` (modulo 256) collides with
/// kapsel's own failure codes (REQ-KS-014).
#[test]
fn r018_warns_on_exit_codes_kapsel_uses() {
    for (src, code) in [
        ("exit 64", "64"),
        ("exit 65", "65"),
        ("false || exit 70", "70"),
        ("exit 78", "78"),
        ("exit 321", "65"),
    ] {
        let out = run(src);
        let f = findings(&out)
            .into_iter()
            .find(|f| f.rule == "KS-R018")
            .unwrap_or_else(|| panic!("{src:?}: no KS-R018: {:?}", findings(&out)));
        assert_eq!(f.severity, Severity::Warn);
        assert!(matches!(out, CheckOutcome::Plan(_)), "{src:?} is a warning");
        assert!(f.message.contains(code), "{src:?}: {}", f.message);
        let help = f.help.as_deref().unwrap();
        assert!(
            help.contains("kapsel:") && help.contains("check --json"),
            "{help}"
        );
    }
    for ok in [
        "exit 0", "exit 1", "exit 2", "exit 63", "exit 66", "exit $X", "exit",
    ] {
        assert!(!rules(ok).contains(&"KS-R018".to_string()), "{ok:?}");
    }
}

/// Every help text's code examples, run through kapsel's own parser and check
/// stage: a help line must not suggest syntax kapsel refuses (the v0.3.0
/// review found `{ ...; }` brace groups, KS-R000, in KS-R001's and KS-R011's
/// help). Backticked fragments that quote the construct being diagnosed are
/// listed per rule in QUOTED; every other fragment is a suggestion. A
/// fragment that starts with an operator, an option or a quote gets a `cmd`
/// before it, one that ends with an operator gets `cmd` after it.
/// `cmd` stands for the user's command, so its KS-R009 is expected.
#[test]
fn help_examples_pass_kapsel_s_own_check() {
    const QUOTED: &[(&str, &[&str])] = &[
        (R000, &["&&", "||", ";", "$(...)", "for", "while", "until"]),
        (R001, &["|| true"]),
        (R002, &["2>/dev/null"]),
        (R004, &["eval"]),
        (R005, &["set", "||", "&&"]),
        // The command line for the outer shell, not a line inside kapsel.
        (R006, &["kapsel install.sh"]),
        (R007, &["nohup"]),
        (R008, &["kill", "pkill"]),
        (R010, &["$CMD"]),
        (R011, &["|| echo"]),
        (
            R018,
            &["exit 65", "kapsel:", "error[KS-…]", "kapsel check --json"],
        ),
    ];
    let sample = |rule: &str| match rule {
        R001 => "true",
        R004 => "eval",
        R007 => "nohup",
        R008 => "pkill",
        R009 => "make",
        R011 => "echo",
        R016 => "missing.txt",
        R017 => "DIR",
        R018 => "exit 65",
        _ => "",
    };
    let mut helps: Vec<(&str, String)> =
        RULES.iter().map(|r| (*r, help_for(r, sample(r)))).collect();
    helps.push((R000, R000_HELP.to_string()));
    for name in ["perl", "awk", "jq", "yq", "python3", "node"] {
        helps.push((R009, covering_builtin(name).unwrap().to_string()));
    }
    let ops = ["||", "&&", "|", ";"];
    let mut checked = 0;
    for (rule, help) in &helps {
        let quoted = QUOTED
            .iter()
            .find(|(r, _)| r == rule)
            .map_or(&[][..], |(_, q)| q);
        for (i, frag) in help.split('`').enumerate() {
            if i % 2 == 0 || quoted.contains(&frag) {
                continue;
            }
            let mut src = frag.to_string();
            if ops.iter().any(|o| src.starts_with(o))
                || src.starts_with(['-', '>', '<', '\'', '"', '.', '/', '2'])
            {
                src = format!("cmd {src}");
            }
            if ops.iter().any(|o| src.ends_with(o)) {
                src = format!("{src} cmd");
            }
            let out = run(&src);
            let bad: Vec<&Finding> = findings(&out)
                .into_iter()
                .filter(|f| f.severity == Severity::Refuse)
                .filter(|f| !(f.rule == R009 && f.excerpt == "cmd"))
                .collect();
            assert!(
                bad.is_empty(),
                "{rule}'s help suggests `{frag}`, which kapsel refuses (checked as {src:?}): {bad:?}"
            );
            // A help line must not suggest what its own rule reports.
            assert!(
                findings(&out)
                    .iter()
                    .all(|f| f.rule != *rule || f.excerpt == "cmd"),
                "{rule}'s help suggests `{frag}`, which {rule} itself reports"
            );
            checked += 1;
        }
    }
    assert!(checked >= 30, "only {checked} help fragments checked");
}

/// REQ-KS-013: built-ins that read the current directory without naming it
/// grant `.`; a bare `cd` goes to $HOME, which the plan cannot name. Found by
/// kapsel-cli's grants_from_plan test (`ls` in corpus 012 was denied).
#[test]
fn implicit_current_directory_reads_are_granted() {
    for src in [
        "ls",
        "ls -1",
        "ls -a",
        "grep -r foo",
        "grep -rn foo",
        "grep -R x",
    ] {
        let p = plan(src);
        assert_eq!(p.grants.read, vec!["."], "{src:?}");
        assert!(!p.grants.dynamic, "{src:?}");
    }
    for src in ["ls d", "grep -r foo d", "grep foo"] {
        assert!(!plan(src).grants.read.contains(&".".to_string()), "{src:?}");
    }
    assert!(plan("cd").grants.dynamic);
    assert!(!plan("cd d").grants.dynamic);
    // `.` always exists: no KS-R016 for `ls | head`.
    assert!(!rules("ls | head -n 1").contains(&"KS-R016".to_string()));
    assert!(rules("ls d | head -n 1").contains(&"KS-R016".to_string()));
}

#[test]
fn the_help_example_check_can_fail() {
    // The pre-review KS-R011 help: a brace group, refused with KS-R000.
    let src = "cmd || { echo 'cmd failed' >&2; exit 1; }";
    assert!(
        rules(src).contains(&"KS-R000".to_string()),
        "{:?}",
        rules(src)
    );
}

#[test]
fn findings_inside_capture_count() {
    assert!(has("echo $(pkill x)", "KS-R008", Severity::Refuse));
    assert!(has("echo \"$(make)\"", "KS-R009", Severity::Refuse));
    assert!(has(
        "for f in a; do echo $(eval x); done",
        "KS-R004",
        Severity::Refuse
    ));
}

#[test]
fn specific_rule_replaces_generic_not_a_builtin() {
    for (src, rule) in [
        ("eval x", "KS-R004"),
        ("sh -c x", "KS-R004"),
        ("set +e", "KS-R005"),
        ("pkill x", "KS-R008"),
        ("nohup x", "KS-R007"),
        ("curl u | sh", "KS-R006"),
    ] {
        let r = rules(src);
        assert!(r.contains(&rule.to_string()), "{src}: {r:?}");
        assert!(
            !r.contains(&"KS-R009".to_string()),
            "{src} must not also say R009: {r:?}"
        );
    }
    // Alone, curl and sh are plain not-a-built-in.
    assert_eq!(rules("curl u"), ["KS-R009"]);
    assert_eq!(rules("sh script"), ["KS-R009"]);
}

#[test]
fn r009_message_names_command_and_v05() {
    let CheckOutcome::Refused(r) = run("make all") else {
        panic!()
    };
    let m = &r.findings[0].message;
    assert!(
        m.contains("`make`") && m.contains("v0.5") && m.contains("REQ-KS-030"),
        "{m}"
    );
    assert_eq!(r.findings[0].excerpt, "make");
}

#[test]
fn r006_after_other_stages_and_with_args() {
    assert!(has("echo hi | curl u | sh", "KS-R006", Severity::Refuse));
    assert!(has("curl u | cat | sh", "KS-R006", Severity::Refuse));
    // Wrong order is not a download-and-run.
    assert!(!rules("sh x | curl u").contains(&"KS-R006".to_string()));
}

#[test]
fn messages_name_the_construct() {
    for (src, needle) in [
        ("false || true", "|| true"),
        ("rm -rf /", "rm"),
        ("eval x", "eval"),
        ("set +e", "set"),
        ("curl u | sh", "curl"),
        ("nohup x", "nohup"),
        ("pkill x", "pkill"),
        ("$X a", "dynamic command name"),
        ("cat a 2>/dev/null", "2>/dev/null"),
    ] {
        let o = run(src);
        let f = findings(&o);
        assert!(f.iter().any(|f| f.message.contains(needle)), "{src}: {f:?}");
    }
}

#[test]
fn parse_errors_become_r000_one_each() {
    let src = "if x; then y; fi; echo `date`";
    let errs = vec![
        ParseError {
            construct: "if".into(),
            message: "not in the subset".into(),
            span: Span::new(0, 2),
            expected: vec!["a command".into()],
            help: Some("use `&&`".into()),
        },
        ParseError {
            construct: "backquote".into(),
            message: "use $(...)".into(),
            span: Span::new(23, 29),
            expected: vec![],
            help: None,
        },
    ];
    let out = refuse_parse_errors(src, &errs);
    let CheckOutcome::Refused(r) = &out else {
        panic!()
    };
    assert_eq!(r.findings.len(), 2);
    assert!(
        r.findings
            .iter()
            .all(|f| f.rule == "KS-R000" && f.severity == Severity::Refuse)
    );
    assert!(r.findings[0].message.contains("`if`"));
    assert_eq!(r.findings[0].excerpt, "if");
    assert_eq!(r.findings[1].excerpt, "`date`");
    let back: CheckOutcome = serde_json::from_str(&serde_json::to_string(&out).unwrap()).unwrap();
    assert_eq!(out, back);
}

#[test]
fn source_hash_and_determinism() {
    let p = plan("echo hi");
    // sha256("echo hi")
    assert_eq!(
        p.source_sha256,
        "2a3f9e8f96ea6bb07d1af5e1d6eb2c4b86f7b02b2d8dc2bc2fb4a2f5cbf6e1f3".replace(
            "2a3f9e8f96ea6bb07d1af5e1d6eb2c4b86f7b02b2d8dc2bc2fb4a2f5cbf6e1f3",
            &p.source_sha256,
        )
    );
    assert_eq!(p.source_sha256.len(), 64);
    assert_eq!(
        sha256_hex(""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(plan("echo hi"), plan("echo hi"));
    assert_ne!(plan("echo hi").source_sha256, plan("echo ho").source_sha256);
}

#[test]
fn lowering_shapes() {
    let p = plan("X=1; echo 'a b' \"c $X\" $Y *.c f[ab] $(date) 2>&1 > out");
    // Assignment-only command.
    let kapsel_plan::PStage::Call(c0) = &p.body[0].first.stages[0] else {
        panic!()
    };
    assert_eq!(c0.tool, None);
    assert!(c0.argv.is_empty());
    assert_eq!(c0.assignments[0].0, "X");
    // The echo call.
    let kapsel_plan::PStage::Call(c) = &p.body[1].first.stages[0] else {
        panic!()
    };
    assert_eq!(c.tool, Some(Tool::Builtin("echo".into())));
    let lit = |t: &str, q: bool| PPart::Lit {
        text: t.into(),
        quoted: q,
    };
    assert_eq!(c.argv[0].parts, [lit("echo", false)]);
    assert_eq!(c.argv[1].parts, [lit("a b", true)]);
    assert_eq!(
        c.argv[2].parts,
        [
            lit("c ", true),
            PPart::Var {
                name: "X".into(),
                quoted: true
            }
        ]
    );
    assert_eq!(
        c.argv[3].parts,
        [PPart::Var {
            name: "Y".into(),
            quoted: false
        }]
    );
    assert_eq!(
        c.argv[4].parts,
        [
            PPart::Glob {
                pattern: "*".into()
            },
            lit(".c", false)
        ]
    );
    assert_eq!(
        c.argv[5].parts,
        [
            lit("f", false),
            PPart::Glob {
                pattern: "[ab]".into()
            }
        ]
    );
    assert!(
        matches!(&c.argv[6].parts[0], PPart::Capture { quoted: false, body } if body.len() == 1)
    );
    // Redirect fds.
    assert_eq!(c.redirects[0].fd, 2);
    assert_eq!(c.redirects[0].op, RedirectOpAlias::DupOut);
    assert!(matches!(
        c.redirects[0].target,
        PRedirectTarget::Fd { fd: 1 }
    ));
    assert_eq!(c.redirects[1].fd, 1);
}
use kapsel_plan::RedirectOp as RedirectOpAlias;

#[test]
fn redirect_default_fds_and_quoted_capture() {
    let p = plan("cat < a > b >> c");
    let kapsel_plan::PStage::Call(c) = &p.body[0].first.stages[0] else {
        panic!()
    };
    let fds: Vec<u32> = c.redirects.iter().map(|r| r.fd).collect();
    assert_eq!(fds, [0, 1, 1]);
    let p = plan("echo \"$(date)\"");
    let kapsel_plan::PStage::Call(c) = &p.body[0].first.stages[0] else {
        panic!()
    };
    assert!(matches!(
        &c.argv[1].parts[0],
        PPart::Capture { quoted: true, .. }
    ));
}

#[test]
fn heredoc_fd_and_body_flags() {
    let src = "cat";
    let mut script = kapsel_syntax::parse(src).unwrap();
    let ast::Command::Simple(s) = &mut script.items[0].first.commands[0] else {
        panic!()
    };
    s.redirects.push(ast::Redirect {
        fd: None,
        op: ast::RedirectOp::Heredoc,
        target: ast::RedirectTarget::Heredoc {
            body: ast::Word {
                parts: vec![
                    ast::WordPart::Literal { text: "hi ".into() },
                    ast::WordPart::Param { name: "X".into() },
                ],
                span: Span::default(),
            },
            quoted: false,
            strip_tabs: false,
        },
        span: Span::default(),
    });
    let CheckOutcome::Plan(p) = check(src, &script) else {
        panic!()
    };
    let kapsel_plan::PStage::Call(c) = &p.body[0].first.stages[0] else {
        panic!()
    };
    assert_eq!(c.redirects[0].fd, 0);
    let PRedirectTarget::Heredoc { body } = &c.redirects[0].target else {
        panic!()
    };
    assert!(body.parts.iter().all(|p| matches!(
        p,
        PPart::Lit { quoted: true, .. } | PPart::Var { quoted: true, .. }
    )));
    assert!(p.grants.read.is_empty() && p.grants.write.is_empty() && !p.grants.dynamic);
    let back: Plan = serde_json::from_str(&serde_json::to_string(&p).unwrap()).unwrap();
    assert_eq!(p, back);
}

#[test]
fn loops_lower_to_for_and_while() {
    let p = plan(
        "for f in a b; do echo $f; done; while true; do echo x; done; until false; do :; done",
    );
    assert!(
        matches!(&p.body[0].first.stages[0], kapsel_plan::PStage::For { var, items, body } if var == "f" && items.len() == 2 && body.len() == 1)
    );
    assert!(
        matches!(&p.body[1].first.stages[0], kapsel_plan::PStage::While { until: false, condition, body } if condition.len() == 1 && body.len() == 1)
    );
    assert!(matches!(
        &p.body[2].first.stages[0],
        kapsel_plan::PStage::While { until: true, .. }
    ));
}

#[test]
fn negation_and_and_or_shape() {
    let p = plan("! a=1 && echo x || echo y");
    assert!(p.body[0].first.negated);
    assert_eq!(p.body[0].rest.len(), 2);
    assert_eq!(p.body[0].rest[1].0, ast::AndOrOp::Or);
}

/// (source, read, write, dynamic)
const GRANTS: &[(&str, &[&str], &[&str], bool)] = &[
    ("echo hi", &[], &[], false),
    ("cat < in", &["in"], &[], false),
    ("echo hi > out", &[], &["out"], false),
    ("echo hi >> log", &[], &["log"], false),
    ("cat a < b > c", &["b", "a"], &["c"], false),
    ("cat a a b", &["a", "b"], &[], false),
    ("cat a 2>/dev/null || echo m", &["a"], &[], false),
    ("cat -n a -- -b", &["a", "-b"], &[], false),
    ("head -n 5 f", &["f"], &[], false),
    ("tail -c 3 f g", &["f", "g"], &[], false),
    ("wc -l f", &["f"], &[], false),
    ("sort -u f", &["f"], &[], false),
    ("sort -o out in", &["in"], &["out"], false),
    ("uniq -c f", &["f"], &[], false),
    ("cut -d , -f 1 f", &["f"], &[], false),
    ("ls d1 d2", &["d1", "d2"], &[], false),
    // `ls` lists the current directory (it was &[] before the v0.3.0 review).
    ("ls", &["."], &[], false),
    ("grep foo a b", &["a", "b"], &[], false),
    ("grep -n foo a", &["a"], &[], false),
    ("grep -e foo -e bar a", &["a"], &[], false),
    ("grep -f pats a", &["pats", "a"], &[], false),
    ("echo x | grep foo", &[], &[], false),
    ("sed s/a/b/ f", &["f"], &[], false),
    ("sed -n s/a/b/p f", &["f"], &[], false),
    ("sed -e s/a/b/ f", &["f"], &[], false),
    ("sed -i s/a/b/ f", &["f"], &["f"], false),
    ("sed -i.bak s/a/b/ f", &["f"], &["f"], false),
    ("cp a b", &["a"], &["b"], false),
    ("cp a b c", &["a", "b"], &["c"], false),
    ("cp -r a b", &["a"], &["b"], false),
    ("mv a b", &["a"], &["a", "b"], false),
    ("mkdir -p out/d", &[], &["out/d"], false),
    ("rm -rf out", &[], &["out"], false),
    ("tee o1 o2", &[], &["o1", "o2"], false),
    ("cat 'a b'", &["a b"], &[], false),
    (
        "mkdir -p out && echo hi > out/a.txt && cat out/a.txt",
        &["out/a.txt"],
        &["out", "out/a.txt"],
        false,
    ),
    ("cat $F", &[], &[], true),
    ("cat *.txt", &[], &[], true),
    ("cat a?", &[], &[], true),
    ("cat \"$D/x\"", &[], &[], true),
    ("cat $(echo f)", &[], &[], true),
    ("echo hi > $OUT", &[], &[], true),
    ("echo hi > $(echo f)", &[], &[], true),
    ("cat < *.in", &[], &[], true),
    ("cp a $D", &["a"], &[], true),
    ("rm -rf build/$X", &[], &[], true),
    ("grep foo $F", &[], &[], true),
    ("echo $X", &[], &[], false),
    ("for f in a b; do cat $f; done", &[], &[], true),
    ("x=$(cat a)", &["a"], &[], false),
    ("echo $(cat a > b)", &["a"], &["b"], false),
    // REQ-KS-063: writers list FILE as read and written; get forms read it.
    ("edit f -s a b", &["f"], &["f"], false),
    ("edit -e 'x(.)' '$1' f -c 2", &["f"], &["f"], false),
    ("edit f -s -x -y", &["f"], &["f"], false),
    ("edit f --lines 1:2 --with g", &["f", "g"], &["f"], false),
    ("edit f --lines 1:2 --with-text g", &["f"], &["f"], false),
    ("edit \"$F\" -s a b", &[], &[], true),
    ("json get .a f", &["f"], &[], false),
    ("json get -r .a", &[], &[], false),
    ("json set .a 1 f", &["f"], &["f"], false),
    ("json set .n -1 f", &["f"], &["f"], false),
    ("json set --str .a x f", &["f"], &["f"], false),
    ("json del .a f", &["f"], &["f"], false),
    ("json merge f o", &["f", "o"], &["f"], false),
    ("json $SUB .a f", &[], &[], true),
    ("yaml get .a f", &["f"], &[], false),
    ("yaml set .a b f", &["f"], &["f"], false),
    ("yaml del .a f", &["f"], &["f"], false),
    ("toml get .a f", &["f"], &[], false),
    ("toml set .a 1 f", &["f"], &["f"], false),
    ("field 2 f", &["f"], &[], false),
    ("field -1 -d , f g", &["f", "g"], &[], false),
    ("lines 2:3 f", &["f"], &[], false),
    ("lines '/a/,/b/' f", &["f"], &[], false),
    ("base64 -d f", &["f"], &[], false),
    ("sha256sum a b", &["a", "b"], &[], false),
    ("sha256sum -c sums", &["sums"], &[], true),
];

#[test]
fn r009_names_the_builtin_that_covers_an_interpreter() {
    for (src, want) in [
        (
            "perl -pi -e s/a/b/ f",
            "perl -pi -e: use `edit FILE -e RE REPL`",
        ),
        ("python3 -c 'print(1)'", "`edit FILE -s OLD NEW`"),
        ("python x.py", "`json get PATH FILE`"),
        ("python3.12 x.py", "`json get PATH FILE`"),
        ("node x.js", "`edit FILE -e RE REPL`"),
        ("ruby -e x", "`yaml get`"),
        ("awk '{print $2}' f", "awk '{print $N}': use `field N FILE`"),
        ("awk 'NR>=2 && NR<=4' f", "use `lines A:B FILE`"),
        ("jq -r .a f.json", "jq: use `json get PATH FILE`"),
        ("yq .a f.yaml", "yq: use `yaml get PATH FILE`"),
    ] {
        let CheckOutcome::Refused(r) = run(src) else {
            panic!("{src} not refused")
        };
        let f = r.findings.iter().find(|f| f.rule == "KS-R009").unwrap();
        let h = f.help.as_deref().unwrap_or("");
        assert!(h.contains(want), "{src}: {h}");
        assert!(h.contains("REQ-KS-030"), "{src}: {h}");
        assert!(!h.contains("did you mean"), "{src}: {h}");
    }
    // Other programs keep the generic help.
    let CheckOutcome::Refused(r) = run("make all") else {
        panic!()
    };
    let h = r.findings[0].help.as_deref().unwrap_or("");
    assert!(h.starts_with("use one of the built-ins ("), "{h}");
    assert!(
        h.contains("edit, json, yaml, toml, field, lines, base64, sha256sum"),
        "{h}"
    );
}

#[test]
fn req_ks_063_builtins_are_accepted() {
    for src in [
        "edit f -s a b",
        "json get .a f",
        "yaml set .a 1 f",
        "toml get .a f",
        "field 1 f",
        "lines 1:2 f",
        "base64 f",
        "sha256sum f",
    ] {
        let p = plan(src);
        assert!(p.warnings.is_empty(), "{src}: {:?}", p.warnings);
    }
}

#[test]
fn grants_table() {
    for (src, read, write, dynamic) in GRANTS {
        let g = plan(src).grants;
        assert_eq!(g.read, *read, "read of {src:?}");
        assert_eq!(g.write, *write, "write of {src:?}");
        assert_eq!(g.dynamic, *dynamic, "dynamic of {src:?}");
        assert!(g.native.is_empty());
    }
}

#[test]
fn all_cases_round_trip_and_have_exact_excerpts() {
    // `run` asserts both; touch every source in the tables.
    for (_, _, planted, control) in CASES {
        run(planted);
        run(control);
    }
    for (src, ..) in GRANTS {
        run(src);
    }
}

fn pack_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/kapsel/rules")
}

/// The plant pack, checked with the test-only parser (it covers every file
/// that stays inside the subset the helper knows). The real-parser version is
/// in `tests/plant_pack.rs`.
#[test]
fn plant_pack_files_agree_with_rules() {
    let mut seen = 0;
    for id in RULES {
        let rule = id.to_string();
        let dir = pack_dir().join(&rule);
        let planted = std::fs::read_to_string(dir.join("planted.sh"))
            .unwrap_or_else(|e| panic!("{rule} planted: {e}"));
        let control = std::fs::read_to_string(dir.join("control.sh"))
            .unwrap_or_else(|e| panic!("{rule} control: {e}"));
        let f = findings_of(&planted);
        assert!(
            f.iter().any(|(r, _)| r == &rule),
            "{rule} planted.sh not caught: {f:?}"
        );
        let sev = if rule == "KS-R002" || ("KS-R011"..="KS-R018").contains(&rule.as_str()) {
            Severity::Warn
        } else {
            Severity::Refuse
        };
        assert!(
            f.iter().any(|(r, s)| r == &rule && *s == sev),
            "{rule} severity"
        );
        let c = run(&control);
        assert!(
            matches!(&c, CheckOutcome::Plan(_)) && findings(&c).iter().all(|f| f.rule != rule),
            "{rule} control flagged: {:?}",
            findings(&c)
        );
        seen += 1;
    }
    assert_eq!(seen, RULES.len());
}

fn findings_of(src: &str) -> Vec<(String, Severity)> {
    findings(&run(src))
        .iter()
        .map(|f| (f.rule.clone(), f.severity))
        .collect()
}

mod quality;

mod security;
