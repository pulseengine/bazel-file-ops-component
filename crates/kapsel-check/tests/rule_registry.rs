//! REQ-KS-016: every rule in the registry has a planted script and a control,
//! and no directory in the plant pack names a rule the registry does not know.

use std::path::Path;

fn rules_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/kapsel/rules")
}

#[test]
fn every_registered_rule_has_a_planted_script_and_a_control() {
    let root = rules_dir();
    for id in kapsel_check::RULES {
        for file in ["planted.sh", "control.sh"] {
            let p = root.join(id).join(file);
            let text = std::fs::read_to_string(&p)
                .unwrap_or_else(|e| panic!("{id}: {} is missing: {e}", p.display()));
            assert!(!text.trim().is_empty(), "{id}: {} is empty", p.display());
        }
    }
}

#[test]
fn plant_pack_directories_are_exactly_the_registry() {
    let mut dirs: Vec<String> = std::fs::read_dir(rules_dir())
        .unwrap()
        .map(|e| e.unwrap())
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    dirs.sort();
    let mut want: Vec<String> = kapsel_check::RULES.iter().map(|s| s.to_string()).collect();
    want.sort();
    assert_eq!(
        dirs, want,
        "plant-pack directories and kapsel_check::RULES differ"
    );
}
