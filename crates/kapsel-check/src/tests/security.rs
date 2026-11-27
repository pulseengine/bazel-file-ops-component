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
        "rm -r ~/",
        "rm -rf ./*",
        "rm -rf a/../..",
        "rm -rf ~/..",
        "rm -rf ../..",
        "rm -rf ..",
        "rm -rf .*",
        "rm -rf *",
        "rm -rf ~/*",
        "rm -rf ../*",
        "rm -rf /../",
        "rm -rf a/b/../../..",
        "rm -rf /.",
        "rm -rf $X/*",
        "rm -rf -- /*",
    ] {
        assert!(has(planted, "KS-R003", Severity::Refuse), "{planted:?}: {:?}", rules(planted));
    }
    for control in [
        "rm -rf ./build",
        "rm -rf out/*.o",
        "rm -rf ../x",
        "rm -rf a/../b",
        "rm -rf /tmp/*",
        "rm -rf ~/x",
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
    ("sed -i \"\" s/a/b/ f", &["s/a/b/", "f"], &["s/a/b/", "f"], true),
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
        assert!(has(src, rule, Severity::Warn), "{src:?} should warn {rule}: {:?}", rules(src));
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
        assert!(!has_rule(src, rule), "{src:?} should not hit {rule}: {:?}", rules(src));
    }
}

#[test]
fn r017_does_not_double_report_recursive_rm() {
    let r = rules("rm -rf \"$D/x\"");
    assert!(r.contains(&"KS-R003".to_string()) && !r.contains(&"KS-R017".to_string()), "{r:?}");
    assert!(rules("rm -f \"$D/x\"").contains(&"KS-R017".to_string()));
}

#[test]
fn error_corpus_matches_expectations() {
    // Cases the test-only parser cannot read: they need `kapsel_syntax`
    // (003 backquotes, 006 `${name[1]}`). The count is pinned.
    const SKIP: &[&str] = &["003-backquote-in-double-quotes.sh", "006-zsh-array-subscript.sh"];
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/kapsel/errors/ci");
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".sh"))
        .collect();
    names.sort();
    assert_eq!(names.len(), 25, "corpus size changed: {names:?}");
    let (mut skipped, mut run_errors, mut tested) = (0, 0, 0);
    for name in &names {
        let src = std::fs::read_to_string(dir.join(name)).unwrap();
        let expect = src.lines().next().unwrap().strip_prefix("# expect: ").unwrap_or_else(|| panic!("{name}: no expect line"));
        let mut words = expect.split_whitespace();
        let kind = words.next().unwrap().trim_end_matches(';');
        if SKIP.contains(&name.as_str()) {
            skipped += 1;
            continue;
        }
        if kind == "run-error" {
            run_errors += 1;
            continue;
        }
        tested += 1;
        let out = run(&src);
        match kind {
            "refuse" => {
                let rule = words.next().unwrap();
                assert!(matches!(out, CheckOutcome::Refused(_)), "{name}: should refuse");
                assert!(findings(&out).iter().any(|f| f.rule == rule && f.severity == Severity::Refuse), "{name}: want refuse {rule}, got {:?}", findings(&out));
            }
            "warn" => {
                let rule = words.next().unwrap();
                assert!(findings(&out).iter().any(|f| f.rule == rule && f.severity == Severity::Warn), "{name}: want warn {rule}, got {:?}", findings(&out));
            }
            "plan" => assert!(matches!(out, CheckOutcome::Plan(_)), "{name}: should plan, got {:?}", findings(&out)),
            other => panic!("{name}: unknown expectation {other}"),
        }
    }
    assert_eq!(skipped, SKIP.len());
    assert_eq!(skipped, 2, "skip list changed");
    assert_eq!(run_errors, 3, "run-error cases changed");
    assert_eq!(tested, 20);
}
