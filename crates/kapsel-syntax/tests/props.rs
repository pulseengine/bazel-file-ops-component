//! No-panic and lossless properties over random input (REQ-KS-010 oracle).
//!
//! For every input: parsing does not panic or hang, the tree text equals the
//! input, error spans are valid, and an accepted script's AST spans are exact
//! and nested.

mod common;

use proptest::prelude::*;

fn check(src: &str) -> Result<(), TestCaseError> {
    let parse = kapsel_syntax::parse_green(src);
    prop_assert_eq!(parse.syntax_node().text().to_string(), src);
    if let Err(e) = common::check_errors(src, parse.errors()) {
        return Err(TestCaseError::fail(e));
    }
    let _ = parse.is_incomplete();
    match parse.lower() {
        Ok(script) => {
            prop_assert!(parse.ok());
            if let Err(e) = common::check_spans(src, &script) {
                return Err(TestCaseError::fail(format!("{src:?}: {e}")));
            }
        }
        Err(errors) => {
            // Lowering an error-free tree must succeed: its errors are the
            // parser's, never the lowering's.
            prop_assert!(
                !parse.ok(),
                "{:?}: lowering failed on an error-free tree: {:?}",
                src,
                errors
            );
            prop_assert_eq!(errors.as_slice(), parse.errors());
        }
    }
    Ok(())
}

const SHELL_CHARS: &[char] = &[
    ' ', '\t', '\n', ';', '&', '|', '<', '>', '(', ')', '{', '}', '[', ']', '$', '`', '\'', '"',
    '\\', '#', '*', '?', '!', '=', '~', '-', ',', '.', ':', '/', '%', '@', '0', '1', '2', '9', 'a',
    'b', 'x', 'E', 'O', 'F', '_', 'é', 'π', '\r', '\0',
];

const TOKENS: &[&str] = &[
    "echo", "cat", "x", "X=1", "Y=", "a b", " ", "  ", "\t", "\n", ";", ";;", "&", "&&", "||", "|",
    "|&", "!", "(", ")", "((", "))", "{", "}", "[", "]", "[[", "]]", "<", ">", ">>", "<<", "<<-",
    "<<<", ">&", "<&", ">|", "<>", "&>", "2>", "2>&1", ">&-", "<(", ">(", "$", "$x", "${x}",
    "${x:-y}", "${#x}", "${", "$(", "$((", "$?", "$#", "$@", "$*", "$1", "$$", "$'", "$\"", "`",
    "'", "\"", "\\", "\\\n", "#", "# c\n", "*", "?", "[a-z]", "{a,b}", "~", "~/x", "for", "in",
    "do", "done", "while", "until", "if", "then", "else", "elif", "fi", "case", "esac", "function",
    "select", "coproc", "EOF", "'EOF'", "\nEOF\n", "f()", "=", "é",
];

proptest! {
    #![proptest_config(ProptestConfig { cases: 10_000, ..ProptestConfig::default() })]

    #[test]
    fn shell_char_soup(chars in prop::collection::vec(prop::sample::select(SHELL_CHARS), 0..80)) {
        let src: String = chars.into_iter().collect();
        check(&src)?;
    }

    #[test]
    fn token_concatenation(toks in prop::collection::vec(prop::sample::select(TOKENS), 0..40)) {
        let src: String = toks.concat();
        check(&src)?;
    }

    #[test]
    fn token_concatenation_spaced(toks in prop::collection::vec(prop::sample::select(TOKENS), 0..40)) {
        let src = toks.join(" ");
        check(&src)?;
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 4_000, ..ProptestConfig::default() })]

    #[test]
    fn arbitrary_unicode(src in "\\PC{0,120}") {
        check(&src)?;
    }

    #[test]
    fn arbitrary_bytes_as_lossy_utf8(bytes in prop::collection::vec(any::<u8>(), 0..120)) {
        let src = String::from_utf8_lossy(&bytes).into_owned();
        check(&src)?;
    }
}

/// Deep nesting is capped instead of overflowing the stack.
#[test]
fn deep_nesting_does_not_overflow() {
    for src in [
        "$(".repeat(20_000),
        "echo \"$(".repeat(5_000),
        "for x in a; do ".repeat(5_000),
        "while ".repeat(5_000),
        "coproc ".repeat(20_000),
        "f() ".repeat(10_000),
        "( ".repeat(20_000),
        "{ ".repeat(20_000),
        "<(".repeat(20_000),
        "! ".repeat(20_000),
    ] {
        let parse = kapsel_syntax::parse_green(&src);
        assert_eq!(parse.syntax_node().text().to_string(), src);
        assert!(!parse.ok());
    }
}

/// Every golden input truncated at every char boundary still parses
/// losslessly, without panicking.
#[test]
fn truncated_goldens() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/kapsel/parse");
    let mut n = 0;
    for kind in ["accept", "refuse"] {
        for e in std::fs::read_dir(root.join(kind)).expect("dir") {
            let p = e.expect("entry").path();
            if p.extension().is_none_or(|x| x != "sh") {
                continue;
            }
            let src = std::fs::read_to_string(&p).expect("read");
            for (i, _) in src.char_indices() {
                check(&src[..i]).unwrap_or_else(|e| panic!("{}[..{i}]: {e}", p.display()));
                n += 1;
            }
        }
    }
    assert!(n > 1000);
}
