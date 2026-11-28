//! Error-quality tests: help, notes, suggestions and the rendered goldens.

use super::*;

#[test]
fn every_finding_has_help_and_a_note() {
    for (rule, _, planted, _) in CASES {
        let o = run(planted);
        for f in findings(&o) {
            assert!(
                f.help.as_deref().is_some_and(|h| !h.is_empty()),
                "{} help in {planted:?}",
                f.rule
            );
            assert!(!f.notes.is_empty(), "{} notes in {planted:?}", f.rule);
        }
        assert!(
            findings(&o)
                .iter()
                .any(|f| f.rule == *rule || f.rule == "KS-R000")
        );
    }
}

#[test]
fn r009_suggests_close_builtins() {
    let CheckOutcome::Refused(r) = run("grpe -n foo a") else {
        panic!()
    };
    assert!(
        r.findings[0]
            .help
            .as_deref()
            .unwrap()
            .contains("did you mean `grep`?")
    );
    let CheckOutcome::Refused(r) = run("make all") else {
        panic!()
    };
    assert!(
        !r.findings[0]
            .help
            .as_deref()
            .unwrap()
            .contains("did you mean")
    );
    assert_eq!(edit_distance("grpe", "grep"), 1);
    assert_eq!(closest_builtin("cta"), Some("cat"));
    assert_eq!(closest_builtin("xyz"), None);
}

#[test]
fn r000_passes_parser_expected_and_help_through() {
    let src = "if x";
    let e = ParseError {
        construct: "if".into(),
        message: "not in the subset".into(),
        span: Span::new(0, 2),
        expected: vec!["a command".into()],
        help: Some("use `&&` and `||`".into()),
    };
    let CheckOutcome::Refused(r) = refuse_parse_errors(src, &[e]) else {
        panic!()
    };
    assert_eq!(r.findings[0].help.as_deref(), Some("use `&&` and `||`"));
    assert!(r.findings[0].notes[0].contains("expected a command"));
    // Without parser help there is still a fix.
    let e = ParseError {
        construct: "x".into(),
        message: "m".into(),
        span: Span::new(0, 1),
        expected: vec![],
        help: None,
    };
    let CheckOutcome::Refused(r) = refuse_parse_errors(src, &[e]) else {
        panic!()
    };
    assert!(r.findings[0].help.is_some());
}

/// Rendered finding list for each planted script, reviewed like code.
/// Regenerate with `KAPSEL_BLESS=1 cargo test -p kapsel-check goldens`.
#[test]
fn goldens_match_expected_json() {
    for id in [
        "R000", "R001", "R002", "R003", "R004", "R005", "R006", "R007", "R008", "R009", "R010",
        "R011", "R012", "R013", "R014", "R015", "R016", "R017",
    ] {
        let dir = pack_dir().join(format!("KS-{id}"));
        let planted = std::fs::read_to_string(dir.join("planted.sh")).unwrap();
        let o = run(&planted);
        let mut text = serde_json::to_string_pretty(&findings(&o)).unwrap();
        text.push('\n');
        let path = dir.join("expected.json");
        if std::env::var_os("KAPSEL_BLESS").is_some() {
            std::fs::write(&path, &text).unwrap();
        }
        let want = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path:?}: {e}"));
        assert_eq!(text, want, "golden for KS-{id}");
    }
}
