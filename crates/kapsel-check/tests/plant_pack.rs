//! The plant pack through the real parser (`kapsel_syntax::parse`): every
//! `testdata/kapsel/rules/<id>/planted.sh` must be caught, every `control.sh`
//! must pass clean, and the rendered findings must match `expected.json`
//! (KS-R000 is compared by rule only: its wording comes from the parser).

use kapsel_plan::CheckOutcome;
use std::path::Path;

fn run(src: &str) -> CheckOutcome {
    match kapsel_syntax::parse(src) {
        Ok(script) => kapsel_check::check(src, &script),
        Err(errs) => kapsel_check::refuse_parse_errors(src, &errs),
    }
}

#[test]
#[ignore = "needs parser (ws/parser)"]
fn plant_pack_through_the_real_parser() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/kapsel/rules");
    for n in 0..=10 {
        let id = format!("KS-R{n:03}");
        let dir = root.join(&id);
        let planted = std::fs::read_to_string(dir.join("planted.sh")).unwrap();
        let control = std::fs::read_to_string(dir.join("control.sh")).unwrap();
        let found: Vec<kapsel_plan::Finding> = match run(&planted) {
            CheckOutcome::Plan(p) => p.warnings,
            CheckOutcome::Refused(r) => r.findings,
        };
        assert!(found.iter().any(|f| f.rule == id), "{id} planted not caught: {found:?}");
        for f in &found {
            assert_eq!(f.excerpt, &planted[f.span.start as usize..f.span.end as usize]);
        }
        if id != "KS-R000" {
            let want: Vec<kapsel_plan::Finding> =
                serde_json::from_str(&std::fs::read_to_string(dir.join("expected.json")).unwrap()).unwrap();
            assert_eq!(found, want, "golden for {id}");
        }
        match run(&control) {
            CheckOutcome::Plan(p) => assert!(p.warnings.is_empty(), "{id} control warns: {:?}", p.warnings),
            CheckOutcome::Refused(r) => panic!("{id} control refused: {:?}", r.findings),
        }
    }
}
