//! Targeted checks of decisions the goldens do not spell out.

use kapsel_syntax::ast::*;

fn simple(src: &str) -> SimpleCommand {
    let script = kapsel_syntax::parse(src).unwrap_or_else(|e| panic!("{src:?}: {e:?}"));
    match &script.items[0].first.commands[0] {
        Command::Simple(s) => s.clone(),
        c => panic!("not simple: {c:?}"),
    }
}

fn constructs(src: &str) -> Vec<String> {
    kapsel_syntax::parse_green(src)
        .errors()
        .iter()
        .map(|e| e.construct.clone())
        .collect()
}

#[test]
fn keywords_only_in_command_position() {
    let s = simple("X=1 for x");
    assert_eq!(s.assignments.len(), 1);
    assert_eq!(s.words.len(), 2);
    assert!(kapsel_syntax::parse("echo if then fi done }").is_ok());
}

#[test]
fn dup_redirect_targets_are_fds() {
    let s = simple("cmd 2>&1");
    assert_eq!(s.redirects[0].fd, Some(2));
    assert_eq!(s.redirects[0].op, RedirectOp::DupOut);
    assert_eq!(s.redirects[0].target, RedirectTarget::Fd { fd: 1 });
}

#[test]
fn nested_quotes_inside_command_substitution() {
    assert!(kapsel_syntax::parse(r#"echo "$(echo ")")""#).is_ok());
}

#[test]
fn assignment_value_keeps_hash_and_equals() {
    let s = simple("X=a#b=c env");
    assert_eq!(
        s.assignments[0].value.parts,
        vec![WordPart::Literal {
            text: "a#b=c".into()
        }]
    );
}

#[test]
fn quoted_heredoc_is_one_literal() {
    let s = simple("cat <<'E'\n$x `y`\nE\n");
    match &s.redirects[0].target {
        RedirectTarget::Heredoc {
            body,
            quoted,
            strip_tabs,
        } => {
            assert!(*quoted);
            assert!(!*strip_tabs);
            assert_eq!(
                body.parts,
                vec![WordPart::Literal {
                    text: "$x `y`\n".into()
                }]
            );
        }
        t => panic!("{t:?}"),
    }
}

#[test]
fn every_error_is_collected() {
    assert_eq!(
        constructs("echo `a`; sleep 1 & echo ${x:-y} |& cat"),
        ["backquote", "background", "param-operator", "pipe-stderr"]
    );
}

#[test]
fn errors_carry_expected_and_help() {
    let p = kapsel_syntax::parse_green("for x in a; do echo");
    let e = &p.errors()[0];
    assert_eq!(e.expected, ["done"]);
    let p = kapsel_syntax::parse_green("echo >");
    assert_eq!(p.errors()[0].expected, ["word"]);
    let p = kapsel_syntax::parse_green("echo `x`");
    assert_eq!(p.errors()[0].help.as_deref(), Some("use $(...)"));
}

#[test]
fn unterminated_quote_points_at_opening_quote() {
    let src = "echo a \"b c";
    let p = kapsel_syntax::parse_green(src);
    let e = &p.errors()[0];
    assert_eq!(&src[e.span.start as usize..e.span.end as usize], "\"");
}

#[test]
fn parse_is_comparable_for_salsa() {
    let a = kapsel_syntax::parse_green("echo a | wc");
    assert_eq!(a, kapsel_syntax::parse_green("echo a | wc"));
    assert_ne!(a, kapsel_syntax::parse_green("echo b | wc"));
    assert!(a.ok());
    assert_eq!(a.syntax_node().kind(), kapsel_syntax::SyntaxKind::SCRIPT);
}

#[test]
fn incomplete_nested() {
    assert!(kapsel_syntax::is_incomplete("echo \"$(ls"));
    assert!(kapsel_syntax::is_incomplete(
        "for x in a; do for y in b; do echo; done"
    ));
    assert!(!kapsel_syntax::is_incomplete("echo done"));
}

mod common;

/// Regression (proptest `token_concatenation`, `["Y=", "\\\n"]`): an
/// assignment value that is only a line continuation had a word span
/// covering the continuation, outside the assignment's trimmed span.
#[test]
fn assignment_value_line_continuation_spans() {
    for (src, value) in [
        ("Y=\\\n", vec![]),
        ("Y=\\\n echo", vec![]),
        ("for=\\\n", vec![]),
        (
            "X=\\\nvalue",
            vec![WordPart::Literal {
                text: "value".into(),
            }],
        ),
        (
            "X=\\\nvalue env",
            vec![WordPart::Literal {
                text: "value".into(),
            }],
        ),
        (
            "X=\"\\\nv\"",
            vec![WordPart::DoubleQuoted {
                parts: vec![WordPart::Literal { text: "v".into() }],
            }],
        ),
        ("X=\"\\\n\"", vec![WordPart::DoubleQuoted { parts: vec![] }]),
    ] {
        let script = kapsel_syntax::parse(src).unwrap_or_else(|e| panic!("{src:?}: {e:?}"));
        common::check_spans(src, &script).unwrap_or_else(|e| panic!("{src:?}: {e}"));
        let Command::Simple(s) = &script.items[0].first.commands[0] else {
            panic!("{src:?}")
        };
        assert_eq!(s.assignments[0].value.parts, value, "{src:?}");
    }
}
