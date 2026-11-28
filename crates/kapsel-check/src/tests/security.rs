//! Review fixes (R003 normalization, option-aware grants, `cd`) and the rules
//! R011..R017, plus the error corpus under `testdata/kapsel/errors/ci/`.

use super::*;

fn has_rule(src: &str, rule: &str) -> bool {
    rules(src).iter().any(|r| r == rule)
}

#[test]
fn r003_normalizes_paths_and_globs() {
    for planted in [
        "rm -rf //",
        "rm -rf /./",
        "rm -rf /*",
        "rm -rf ./*",
        "rm -rf a/../..",
        "rm -rf ../..",
        "rm -rf ..",
        "rm -rf .*",
        "rm -rf *",
        "rm -rf ../*",
        "rm -rf /../",
        "rm -rf a/b/../../..",
        "rm -rf /.",
        "rm -rf $X/*",
        "rm -rf -- /*",
    ] {
        assert!(
            has(planted, "KS-R003", Severity::Refuse),
            "{planted:?}: {:?}",
            rules(planted)
        );
    }
    for control in [
        "rm -rf ./build",
        "rm -rf out/*.o",
        "rm -rf ../x",
        "rm -rf a/../b",
        "rm -rf /tmp/*",
        "rm -f /",
        "rm -rf out/",
    ] {
        assert!(!has_rule(control, "KS-R003"), "{control:?}");
    }
}

/// (source, read, write, dynamic)
const GRANTS2: &[(&str, &[&str], &[&str], bool)] = &[
    ("sort -oout.txt in", &["in"], &["out.txt"], false),
    ("sort --output=o in", &["in"], &["o"], false),
    ("sort -o out in", &["in"], &["out"], false),
    ("grep -ffile a", &["file", "a"], &[], false),
    ("grep --file=pats a", &["pats", "a"], &[], false),
    ("grep --regexp=x a", &["a"], &[], false),
    ("head -n5 f", &["f"], &[], false),
    ("head -5 f", &["f"], &[], false),
    ("head --lines=5 f", &["f"], &[], false),
    ("cp --target-directory=d a b", &["a", "b"], &["d"], false),
    ("cp -t d a", &["a"], &["d"], false),
    ("mv -t d a", &["a"], &["a", "d"], false),
    ("cp -t $D a", &["a"], &[], true),
    ("sort --output=$F in", &["in"], &[], true),
    ("grep -rn foo --include=*.rs .", &["."], &[], false),
    ("sed -e s/a/b/ -ibak f", &["f"], &["f"], false),
    // Options the analysis does not know: assume anything.
    ("cat --weird f", &["f"], &[], true),
    ("ls -@ d", &["d"], &[], true),
    ("sort -Z in", &["in"], &[], true),
    (
        "sed -i \"\" s/a/b/ f",
        &["s/a/b/", "f"],
        &["s/a/b/", "f"],
        true,
    ),
    // cd changes what relative paths mean.
    ("cd sub && cat a", &["sub", "a"], &[], true),
    ("cd . && cat a", &[".", "a"], &[], false),
    ("cat a && cd sub", &["a", "sub"], &[], false),
    ("cd /abs && cat /x", &["/abs", "/x"], &[], false),
    ("cd sub && cat /x", &["sub", "/x"], &[], false),
    ("cd $D && echo hi", &[], &[], true),
    ("cd && cat a", &["a"], &[], true),
    ("for f in a; do cat x; cd d; done", &["x", "d"], &[], true),
];

#[test]
fn grants_review_table() {
    for (src, read, write, dynamic) in GRANTS2 {
        let g = plan(src).grants;
        assert_eq!(g.read, *read, "read of {src:?}");
        assert_eq!(g.write, *write, "write of {src:?}");
        assert_eq!(g.dynamic, *dynamic, "dynamic of {src:?}");
        run(src);
    }
}

#[test]
fn new_rules_fire_and_stay_quiet() {
    let yes = [
        ("a || echo hi", "KS-R011"),
        ("a && b || echo hi", "KS-R011"),
        ("a || printf 'x\\n'", "KS-R011"),
        ("echo x >> $F", "KS-R012"),
        ("cat < $(echo f)", "KS-R012"),
        ("n=1; [ $n -eq 0 ]", "KS-R013"),
        ("test -n $X", "KS-R013"),
        ("cd d; ls", "KS-R014"),
        ("cd d\nls", "KS-R014"),
        ("a && cd d; ls", "KS-R014"),
        ("grep --include=*.rs x .", "KS-R015"),
        ("echo https://x/issues?since=1", "KS-R015"),
        ("echo https://x/*", "KS-R015"),
        ("ls m | head", "KS-R016"),
        ("cat m | sort", "KS-R016"),
        ("grep x m | wc -l", "KS-R016"),
        ("cat \"$D/x\"", "KS-R017"),
        ("cat $D/x", "KS-R017"),
        ("echo hi > \"$D/x\"", "KS-R017"),
        ("cp a \"$D/b\"", "KS-R017"),
    ];
    for (src, rule) in yes {
        assert!(
            has(src, rule, Severity::Warn),
            "{src:?} should warn {rule}: {:?}",
            rules(src)
        );
    }
    let no = [
        ("a && echo hi", "KS-R011"),
        ("a || echo hi && b", "KS-R011"),
        ("a || exit 1", "KS-R011"),
        ("echo x >> \"$F\"", "KS-R012"),
        ("echo x >> out", "KS-R012"),
        ("[ \"$n\" -eq 0 ]", "KS-R013"),
        ("[ $? -eq 0 ]", "KS-R013"),
        ("cd d && ls", "KS-R014"),
        ("cd d", "KS-R014"),
        ("cd d || exit 1; ls", "KS-R014"),
        ("grep --include='*.rs' x .", "KS-R015"),
        ("ls *.c", "KS-R015"),
        ("echo https://x/y", "KS-R015"),
        ("ls m", "KS-R016"),
        ("echo m | head", "KS-R016"),
        ("cat settings", "KS-R017"),
        ("cat \"x/$D\"", "KS-R017"),
        ("echo $X", "KS-R017"),
    ];
    for (src, rule) in no {
        assert!(
            !has_rule(src, rule),
            "{src:?} should not hit {rule}: {:?}",
            rules(src)
        );
    }
}

#[test]
fn r017_does_not_double_report_recursive_rm() {
    let r = rules("rm -rf \"$D/x\"");
    assert!(
        r.contains(&"KS-R003".to_string()) && !r.contains(&"KS-R017".to_string()),
        "{r:?}"
    );
    assert!(rules("rm -f \"$D/x\"").contains(&"KS-R017".to_string()));
}

#[test]
fn error_corpus_matches_expectations() {
    // Cases the test-only parser cannot read: they need `kapsel_syntax`
    // (003 backquotes, 006 `${name[1]}`). The count is pinned.
    const SKIP: &[&str] = &[];
    let dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/kapsel/errors/ci");
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".sh"))
        .collect();
    names.sort();
    assert_eq!(names.len(), 25, "corpus size changed: {names:?}");
    let (mut skipped, mut run_errors, mut tested) = (0, 0, 0);
    let mut wrong = Vec::new();
    for name in &names {
        let src = std::fs::read_to_string(dir.join(name)).unwrap();
        let expect = src
            .lines()
            .next()
            .unwrap()
            .strip_prefix("# expect: ")
            .unwrap_or_else(|| panic!("{name}: no expect line"));
        if SKIP.contains(&name.as_str()) {
            skipped += 1;
            continue;
        }
        let want = parse_expect(expect).unwrap_or_else(|e| panic!("{name}: {e}"));
        if want.run_error {
            run_errors += 1;
        }
        tested += 1;
        let out = run(&src);
        if let Err(e) = want.matches(&out) {
            wrong.push(format!("{name}: {e}"));
        }
    }
    assert!(
        wrong.is_empty(),
        "expect lines that differ:\n{}",
        wrong.join("\n")
    );
    assert_eq!(skipped, SKIP.len());
    assert_eq!(skipped, 0, "skip list changed");
    assert_eq!(run_errors, 3, "run-error cases changed");
    assert_eq!(tested, 25);
}

/// A parsed `# expect:` line: the full check outcome of an error-corpus case
/// (format in testdata/kapsel/errors/README.md).
#[derive(Debug)]
struct Expect {
    refused: bool,
    run_error: bool,
    findings: std::collections::BTreeSet<(String, String)>,
}

fn parse_expect(line: &str) -> Result<Expect, String> {
    let mut e = Expect {
        refused: false,
        run_error: false,
        findings: Default::default(),
    };
    // A run error is a plan with no findings; its message may hold anything.
    if line.starts_with("run-error exit ") && line.contains(" | ") {
        e.run_error = true;
        return Ok(e);
    }
    let mut plan = false;
    for clause in line.split(';').map(str::trim) {
        let words: Vec<&str> = clause.split_whitespace().collect();
        match words.as_slice() {
            ["plan"] => plan = true,
            ["note", ..] => {}
            [sev @ ("refuse" | "warn"), rule] if rule.starts_with("KS-") => {
                e.refused |= *sev == "refuse";
                e.findings.insert((sev.to_string(), rule.to_string()));
            }
            _ => {
                return Err(format!(
                    "bad clause {clause:?}: use `plan`, `refuse KS-Rnnn`, `warn KS-Rnnn`, \
                     `note TEXT` or `run-error exit N | MESSAGE`, separated by `;`"
                ));
            }
        }
    }
    if plan && e.refused {
        return Err("`plan` and `refuse` contradict each other".into());
    }
    if !plan && e.findings.is_empty() {
        return Err("the line states no outcome".into());
    }
    Ok(e)
}

impl Expect {
    /// Ok when the outcome is exactly the stated one: refused or a plan, and
    /// the set of (severity, rule) pairs equal to the stated set.
    fn matches(&self, out: &CheckOutcome) -> Result<(), String> {
        let got: std::collections::BTreeSet<(String, String)> = findings(out)
            .iter()
            .map(|f| {
                let sev = match f.severity {
                    Severity::Refuse => "refuse",
                    Severity::Warn => "warn",
                };
                (sev.to_string(), f.rule.clone())
            })
            .collect();
        let refused = matches!(out, CheckOutcome::Refused(_));
        if refused != self.refused || got != self.findings {
            return Err(format!(
                "want refused={} {:?}, got refused={refused} {got:?}",
                self.refused, self.findings
            ));
        }
        Ok(())
    }
}

#[test]
fn the_expect_matcher_requires_the_full_outcome() {
    // Case 001: bazel is not a built-in, so it is refused (KS-R009) as well as
    // warned about (KS-R011). The pre-review line `warn KS-R011` passed.
    let src = "bazel build //x || echo \"failed\"";
    let out = run(src);
    for loose in ["warn KS-R011", "refuse KS-R009", "plan"] {
        let e = parse_expect(loose).unwrap();
        assert!(e.matches(&out).is_err(), "{loose:?} must not match");
    }
    assert!(
        parse_expect("refuse KS-R009; warn KS-R011")
            .unwrap()
            .matches(&out)
            .is_ok()
    );
    for bad in [
        "refuse KS-R000 param-operator",
        "plan; refuse KS-R001",
        "note only",
        "",
    ] {
        assert!(parse_expect(bad).is_err(), "{bad:?} must be rejected");
    }
    // A run error must check clean.
    let e = parse_expect("run-error exit 2 | x").unwrap();
    assert!(e.matches(&run("cat a")).is_ok());
    assert!(e.matches(&run("cat a 2>/dev/null")).is_err());
}

#[test]
fn tilde_is_refused_by_the_parser_not_r003() {
    let r = rules("rm -rf ~");
    assert_eq!(r, ["KS-R000"], "{r:?}");
}

#[test]
fn rm_danger_home_forms_on_hand_built_words() {
    // The parser refuses `~`, but the normalization stays correct for any
    // word that reaches it (defence in depth).
    for p in ["~", "~/", "~/..", "~/../x", "~//.."] {
        assert!(crate::args::rm_danger(&PWord::literal(p)).is_some(), "{p}");
    }
    for p in ["~/x", "~/a/..", "~x"] {
        let d = crate::args::rm_danger(&PWord::literal(p));
        // `~/a/..` is `~` again.
        assert_eq!(d.is_some(), p == "~/a/..", "{p}: {d:?}");
    }
}

#[test]
fn mixed_option_words_are_options() {
    // rm: an option written with an expansion may be -r.
    for planted in [
        "rm -$X /tmp/x",
        "rm -rf -$X out",
        "rm \"-\"rf /",
        "rm -r\"$X\" out",
        "rm -$(echo r) out",
    ] {
        assert!(
            has(planted, "KS-R003", Severity::Refuse),
            "{planted:?}: {:?}",
            rules(planted)
        );
    }
    for control in ["rm -f out", "rm -- -$X", "rm \"-f\" out", "rm out/$X"] {
        assert!(!has_rule(control, "KS-R003"), "{control:?}");
    }
}

/// (source, read, write, dynamic)
const GRANTS3: &[(&str, &[&str], &[&str], bool)] = &[
    // Mixed option words set dynamic; plain ones do not.
    ("cat -$X f", &["f"], &[], true),
    ("cat -n$X f", &["f"], &[], true),
    ("cat \"-\"n f", &["f"], &[], false),
    ("cat -i$X f", &["f"], &[], true),
    ("head -n$X f", &["f"], &[], false),
    ("sort -T$D in", &["in"], &[], true),
    ("sed -i$X s/a/b/ f", &["f"], &["f"], true),
    ("ls -$(echo l) d", &["d"], &[], true),
    // A long option with an expansion glued on never swallows the next word.
    ("sort --output$X in", &["in"], &[], true),
    ("grep --file$X pats a", &["a"], &[], true),
    ("cp --target-directory$X a b", &["a"], &["b"], true),
    // Every option that names a path records its access.
    ("uniq in out", &["in"], &["out"], false),
    ("uniq in", &["in"], &[], false),
    ("uniq a b c", &["a"], &["b"], true),
    ("sort -T tmp in", &["in"], &["tmp"], false),
    ("sort -Ttmp in", &["in"], &["tmp"], false),
    ("sort --temporary-directory=t in", &["in"], &["t"], false),
    ("sort -o o -T t in", &["in"], &["o", "t"], false),
    ("sed -n s/a/b/p f", &["f"], &[], false),
    ("sed s/a/b/w\\ out f", &[], &[], true),
    ("sed s/a/b/e f", &[], &[], true),
    ("sed 'w out' f", &[], &[], true),
    ("sed -e s/a/b/ -e p f", &["f"], &[], false),
    ("sed -f script.sed f", &["script.sed", "f"], &[], false),
    ("sed -e $X f", &[], &[], true),
];

#[test]
fn grants_round3_table() {
    for (src, read, write, dynamic) in GRANTS3 {
        let g = plan(src).grants;
        assert_eq!(g.read, *read, "read of {src:?}");
        assert_eq!(g.write, *write, "write of {src:?}");
        assert_eq!(g.dynamic, *dynamic, "dynamic of {src:?}");
    }
}

#[test]
fn redirect_only_command_is_colon_with_redirects() {
    let p = plan("> f");
    let kapsel_plan::PStage::Call(c) = &p.body[0].first.stages[0] else {
        panic!()
    };
    assert_eq!(c.tool, Some(Tool::Builtin(":".into())));
    assert_eq!(c.argv, [PWord::literal(":")]);
    assert_eq!(c.redirects.len(), 1);
    assert_eq!(c.redirects[0].fd, 1);
    assert_eq!(p.grants.write, ["f"]);
    // An assignment-only command stays `tool: None`.
    let p = plan("X=1");
    let kapsel_plan::PStage::Call(c) = &p.body[0].first.stages[0] else {
        panic!()
    };
    assert_eq!(c.tool, None);
    assert!(c.argv.is_empty());
}

#[test]
fn background_maps_to_r007() {
    let src = "sleep 1 &";
    let CheckOutcome::Refused(r) = run(src) else {
        panic!()
    };
    assert_eq!(r.findings.len(), 1);
    assert_eq!(r.findings[0].rule, "KS-R007");
    assert_eq!(r.findings[0].severity, Severity::Refuse);
    assert!(r.findings[0].help.is_some() && !r.findings[0].notes.is_empty());
    assert_eq!(r.findings[0].excerpt, "&");
}
