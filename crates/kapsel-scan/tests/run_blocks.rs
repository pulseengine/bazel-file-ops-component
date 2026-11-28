//! Rule `run-blocks` (REQ-KS-015, FEAT-KS-011): every `run:` block of a
//! fixture repository is classified, `${{ }}` stays opaque, and findings point
//! at the original YAML line and column.

use std::path::PathBuf;

use kapsel_scan::runs::{
    Block, Class, RunsReport, check_text, is_shell, mask_expressions, run_blocks, scan_file,
};

fn fixture(case: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../testdata/kapsel/scan/run-blocks")
        .join(case)
}

fn scan(case: &str) -> RunsReport {
    let rep = run_blocks(&fixture(case));
    assert!(rep.errors.is_empty(), "{case}: {:?}", rep.errors);
    rep
}

fn classes(rep: &RunsReport) -> Vec<(usize, Class)> {
    rep.blocks.iter().map(|b| (b.line, b.class)).collect()
}

/// `(rule, line, col)` of every finding of a block.
fn at(b: &Block) -> Vec<(&str, usize, usize)> {
    b.findings
        .iter()
        .map(|f| (f.rule.as_str(), f.line, f.col))
        .collect()
}

#[test]
fn shell_block_plans() {
    let rep = scan("shell");
    assert_eq!(classes(&rep), vec![(7, Class::Plan)]);
    assert!(rep.blocks[0].findings.is_empty());
}

#[test]
fn expression_stays_opaque_and_maps_to_the_yaml_column() {
    let rep = scan("expr");
    assert_eq!(
        classes(&rep),
        vec![(7, Class::Plan), (8, Class::Refused)],
        "{:?}",
        rep.blocks
    );
    // `      - run: ${{ matrix.tool }} --version || true`: the `|| true` is
    // after an 18-byte expression, and the column counts the original text.
    let line = "      - run: ${{ matrix.tool }} --version || true";
    let col = line.find("||").unwrap() + 1;
    let r001 = rep.blocks[1]
        .findings
        .iter()
        .find(|f| f.rule == "KS-R001")
        .unwrap_or_else(|| panic!("no KS-R001: {:?}", rep.blocks[1].findings));
    assert_eq!((r001.line, r001.col), (8, col));
}

#[test]
fn pwsh_is_not_shell_and_the_next_block_still_scans() {
    let rep = scan("pwsh");
    assert_eq!(classes(&rep), vec![(8, Class::NotShell), (9, Class::Plan)]);
    assert_eq!(rep.blocks[0].shell.as_deref(), Some("pwsh"));
    // `|| true` in the pwsh block is not reported: it is not shell.
    assert!(rep.blocks[0].findings.is_empty());
}

#[test]
fn python_step_shell_is_not_shell() {
    let rep = scan("python");
    assert_eq!(classes(&rep), vec![(9, Class::NotShell)]);
    assert_eq!(rep.counts().crash, 0);
}

#[test]
fn defaults_shell_applies_by_scope() {
    let rep = scan("defaults-shell");
    assert_eq!(
        classes(&rep),
        vec![
            (10, Class::NotShell), // workflow default pwsh
            (17, Class::Plan),     // job default bash overrides
            (19, Class::Plan),     // `bash -e {0}`
            (21, Class::Plan),     // kapsel
        ]
    );
    assert_eq!(rep.blocks[0].shell.as_deref(), Some("pwsh"));
    assert_eq!(rep.blocks[1].shell.as_deref(), Some("bash"));
}

#[test]
fn multiline_block_maps_to_its_yaml_line() {
    let rep = scan("multiline");
    let b = &rep.blocks[0];
    assert_eq!((b.line, b.class), (8, Class::Refused));
    // `echo two || true` is the fourth script line: the second is blank.
    let r001: Vec<_> = at(b).into_iter().filter(|f| f.0 == "KS-R001").collect();
    assert_eq!(r001.len(), 1, "{:?}", b.findings);
    assert_eq!(r001[0].1, 11);
    assert_eq!(
        r001[0].2,
        "          echo two || true".find("||").unwrap() + 1
    );
}

#[test]
fn folded_block_refuses_at_the_right_line() {
    let rep = scan("folded");
    let b = &rep.blocks[0];
    assert_eq!(b.class, Class::Refused, "{:?}", b.findings);
    let r001 = b.findings.iter().find(|f| f.rule == "KS-R001").unwrap();
    // `>` folds `echo one\ntwo` onto one script line; `echo three || true`
    // follows a blank line, on YAML line 12.
    assert_eq!(r001.line, 12, "{:?}", b.findings);

    // A finding on a joined line (`echo one two || true`, folded from YAML
    // lines 15 and 16) points at the first YAML line of the joined text.
    let j = &rep.blocks[1];
    assert_eq!((j.line, j.class), (14, Class::Refused), "{:?}", j.findings);
    let r001 = j.findings.iter().find(|f| f.rule == "KS-R001").unwrap();
    assert_eq!(r001.line, 15, "{:?}", j.findings);
}

#[test]
fn windows_runner_defaults_to_pwsh_unless_undecided() {
    let rep = scan("windows-default");
    assert_eq!(
        classes(&rep),
        vec![
            (7, Class::NotShell), // windows-2025, no shell: pwsh
            (9, Class::Plan),     // explicit bash wins
            (16, Class::Plan),    // `${{ matrix.os }}` is undecided: bash
        ]
    );
    assert_eq!(rep.blocks[0].shell.as_deref(), Some("pwsh"));
}

#[test]
fn a_panic_is_caught_and_reported_as_a_crash_message() {
    use kapsel_scan::runs::guarded;
    assert_eq!(guarded(|| 7), Ok(7));
    assert_eq!(guarded(|| -> u8 { panic!("boom") }), Err("boom".into()));
    let n = 3;
    assert_eq!(
        guarded(|| -> u8 { panic!("boom {n}") }),
        Err("boom 3".into())
    );
}

#[test]
fn heredoc_with_expression_is_checked_without_crash() {
    let rep = scan("heredoc");
    let b = &rep.blocks[0];
    assert_ne!(b.class, Class::Crash);
    assert_eq!(b.line, 8);
    // the expression never reaches the parser as text
    assert!(b.findings.iter().all(|f| !f.message.contains("${{")));
}

#[test]
fn or_true_refuses_with_rule_and_line() {
    let rep = scan("refuses");
    let b = &rep.blocks[0];
    assert_eq!((b.line, b.class), (8, Class::Refused));
    let f = b.findings.iter().find(|f| f.rule == "KS-R001").unwrap();
    assert!(f.refuse);
    assert_eq!(f.line, 10);
    assert_eq!(
        f.col,
        "          rm -f cache.tmp || true".find("||").unwrap() + 1
    );
    assert_eq!(rep.counts().refused, 1);
}

#[test]
fn shell_names() {
    for s in [
        "bash",
        "sh",
        "kapsel",
        "kapsel {0}",
        "bash -e {0}",
        "/bin/bash",
    ] {
        assert!(is_shell(s), "{s}");
    }
    for s in [
        "pwsh",
        "python",
        "cmd",
        "powershell",
        "perl {0}",
        "node {0}",
    ] {
        assert!(!is_shell(s), "{s}");
    }
}

#[test]
fn masking_keeps_every_offset() {
    let src = "echo ${{ a }} and ${{\n b }}\nz";
    let m = mask_expressions(src);
    assert_eq!(m.len(), src.len());
    assert_eq!(m.matches('\n').count(), src.matches('\n').count());
    assert!(!m.contains("${{") && !m.contains("}}"));
    // a multi-line expression stays one word
    assert!(check_text(&m).is_ok());
}

#[test]
fn unterminated_expression_and_odd_text_do_not_crash() {
    for text in [
        "echo ${{ never closed",
        "${{",
        "}}",
        "echo '${{ x }}",
        "\u{feff}echo \u{1f600} ${{ \u{e9} }}",
        "",
    ] {
        let m = mask_expressions(text);
        assert_eq!(m.len(), text.len(), "{text:?}");
        assert!(check_text(&m).is_ok(), "{text:?}");
    }
}

/// The no-crash property over a corpus of shapes that real workflows use.
#[test]
fn crash_free_over_the_fixtures_and_a_synthetic_corpus() {
    let mut rep = RunsReport::default();
    let wf = "jobs:\n  a:\n    steps:\n      - run: |\n";
    let bodies = [
        "case $x in a) echo 1;; esac",
        "if [ -f x ]; then echo y; fi",
        "echo $((1+2)) ${x:-d} <(ls)",
        "f() { echo hi; }; f",
        "cat <<'EOF'\n${{ a }}\nEOF",
        "echo \"unterminated",
        "echo $(",
        "a && b || c &",
        "for i in 1 2; do echo $i; done",
        "x=(1 2 3)",
        "echo {a,b}",
        "[[ -n $x ]]",
    ];
    for (i, body) in bodies.iter().enumerate() {
        let mut y = String::from(wf);
        for l in body.lines() {
            y.push_str("          ");
            y.push_str(l);
            y.push('\n');
        }
        scan_file(&format!("synthetic{i}.yml"), &y, &mut rep);
    }
    assert!(rep.errors.is_empty(), "{:?}", rep.errors);
    assert_eq!(rep.blocks.len(), bodies.len());
    assert_eq!(rep.counts().crash, 0, "{:?}", rep.blocks);
    for case in [
        "shell",
        "expr",
        "pwsh",
        "python",
        "defaults-shell",
        "multiline",
        "folded",
        "heredoc",
        "refuses",
        "windows-default",
    ] {
        assert_eq!(scan(case).counts().crash, 0, "{case}");
    }
}
