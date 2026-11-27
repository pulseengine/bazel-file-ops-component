//! Fail-closed behaviour of the expression evaluator, on inline workflows.

use kapsel_scan::{Report, fork_guard_file};

fn scan(runs_on: &str) -> Report {
    let src = format!("on: pull_request\njobs:\n  a:\n    runs-on: {runs_on}\n    steps: []\n");
    let mut rep = Report::default();
    fork_guard_file("w.yml", &src, &mut rep);
    rep
}

const GD: &str = "(github.event_name != 'pull_request' || github.event.pull_request.head.repo.full_name == github.repository)";

#[test]
fn correct_guard_passes() {
    let r = scan(&format!(
        "${{{{ {GD} && fromJSON('[\"self-hosted\"]') || 'macos-latest' }}}}"
    ));
    assert_eq!(r.sites.len(), 1);
    assert!(r.sites[0].ok, "{:?}", r.sites[0].reason);
}

#[test]
fn unsupported_syntax_fails_closed() {
    for e in [
        "${{ github.run_number > 3 && 'ubuntu-24.04' || 'self-hosted' }}",
        "${{ format('{0}', 'self-hosted') }}",
        "${{ 'x' }}-self-hosted",
        "${{ matrix['runner'] }} self-hosted",
        "${{ ( 'self-hosted' }}",
    ] {
        let r = scan(&format!("\"{}\"", e.replace('"', "\\\"")));
        assert!(!r.sites.is_empty() && !r.sites[0].ok, "{e}");
    }
}

#[test]
fn guard_with_hosted_fallback_only_is_not_enough_if_selected_by_unknown() {
    let r = scan("${{ inputs.fast && 'self-hosted' || 'ubuntu-24.04' }}");
    assert!(!r.sites[0].ok);
}

#[test]
fn hosted_labels() {
    use kapsel_scan::expr::hosted_label;
    for l in [
        "ubuntu-24.04",
        "ubuntu-latest",
        "macos-14",
        "windows-latest",
    ] {
        assert!(hosted_label(l), "{l}");
    }
    for l in [
        "self-hosted",
        "linux",
        "ubuntu-self-hosted",
        "my runner",
        "",
    ] {
        assert!(!hosted_label(l), "{l}");
    }
}
