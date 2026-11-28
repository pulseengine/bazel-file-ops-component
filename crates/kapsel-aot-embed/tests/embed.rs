//! The embedding layout, and a byte-for-byte rebuild of the v0.2.2
//! `file_ops_component_aot.wasm` (built by rules_wasm_component's
//! wasm_embed_aot) from its own parts.

use std::fs;
use std::path::PathBuf;

use kapsel_aot_embed::{custom_section, embed, extract, list, sections, strip};
use sha2::{Digest, Sha256};

/// A component header (layer 1) followed by one non-custom section.
fn component() -> Vec<u8> {
    let mut w = b"\0asm\x0d\x00\x01\x00".to_vec();
    w.extend_from_slice(&[0x07, 0x03, 0x01, 0x40, 0x00]);
    w
}

fn arts(spec: &[(&str, usize)]) -> Vec<(String, Vec<u8>)> {
    spec.iter()
        .enumerate()
        .map(|(i, (n, len))| (n.to_string(), vec![i as u8 + 1; *len]))
        .collect()
}

#[test]
fn sections_follow_the_header_in_order() {
    let a = arts(&[("linux-x64", 3), ("portable", 200), ("big", 20_000)]);
    let out = embed(&component(), &a).unwrap();
    assert_eq!(&out[..8], &component()[..8]);
    let names: Vec<String> = list(&out).unwrap().into_iter().map(|(n, _)| n).collect();
    assert_eq!(names, ["linux-x64", "portable", "big"]);
    let s = sections(&out).unwrap();
    assert_eq!(s.len(), 4);
    assert_eq!(s[0].range.start, 8);
    assert_eq!(s[3].id, 7);
    for (name, data) in &a {
        assert_eq!(extract(&out, name).unwrap(), data.as_slice());
    }
    // Each section is exactly what custom_section writes.
    let mut expect = component()[..8].to_vec();
    for (name, data) in &a {
        expect.extend(custom_section(&format!("aot-{name}"), data));
    }
    expect.extend_from_slice(&component()[8..]);
    assert_eq!(out, expect);
}

#[test]
fn strip_returns_the_input() {
    let out = embed(&component(), &arts(&[("a", 1), ("b", 130)])).unwrap();
    assert_eq!(strip(&out).unwrap(), component());
}

#[test]
fn refuses_bad_input() {
    assert!(embed(b"notwasm!", &arts(&[("a", 1)])).is_err());
    assert!(embed(&component(), &arts(&[("a", 1), ("a", 2)])).is_err());
    assert!(embed(&component(), &arts(&[("", 1)])).is_err());
    let mut truncated = embed(&component(), &arts(&[("a", 300)])).unwrap();
    truncated.truncate(100);
    assert!(sections(&truncated).is_err());
    assert!(extract(&component(), "a").is_err());
}

const V022_AOT_SHA256: &str = "19e180368077c55d4565bd63cd7063cfc3f242f6f3d3e5dfff0ada2f8fc748e5";
const V022_SHA256: &str = "fbf869e7075a42c804814965603c50231c93c86a65d3c35b529a942a07474a9f";

fn sha(b: &[u8]) -> String {
    Sha256::digest(b).iter().map(|x| format!("{x:02x}")).collect()
}

/// Needs `KAPSEL_FILEOPS_ORACLE_DIR` holding the v0.2.2 assets
/// `file_ops_component.wasm` and `file_ops_component_aot.wasm`.
#[test]
#[ignore]
fn rebuilds_the_v0_2_2_aot_asset_byte_for_byte() {
    let dir = PathBuf::from(
        std::env::var_os("KAPSEL_FILEOPS_ORACLE_DIR").expect("set KAPSEL_FILEOPS_ORACLE_DIR"),
    );
    let base = fs::read(dir.join("file_ops_component.wasm")).unwrap();
    let aot = fs::read(dir.join("file_ops_component_aot.wasm")).unwrap();
    assert_eq!(sha(&base), V022_SHA256, "not the v0.2.2 component");
    assert_eq!(sha(&aot), V022_AOT_SHA256, "not the v0.2.2 AOT component");

    let parts: Vec<(String, Vec<u8>)> = list(&aot)
        .unwrap()
        .into_iter()
        .map(|(n, r)| (n, aot[r].to_vec()))
        .collect();
    let names: Vec<&str> = parts.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(
        names,
        [
            "linux-x64",
            "linux-arm64",
            "darwin-x64",
            "darwin-arm64",
            "windows-x64",
            "portable"
        ]
    );
    assert_eq!(strip(&aot).unwrap(), base);
    assert_eq!(sha(&embed(&base, &parts).unwrap()), V022_AOT_SHA256);
}
