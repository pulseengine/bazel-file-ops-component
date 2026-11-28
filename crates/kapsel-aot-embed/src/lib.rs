//! Embed wasmtime-precompiled code (`.cwasm`) into a WASM component as
//! custom sections named `aot-<name>`.
//!
//! The byte layout is the one rules_wasm_component's `wasm_embed_aot` rule
//! writes (tools/wasm_embed_aot at 27eefae, the rule that built kapsel's
//! `file_ops_component_aot.wasm` up to v0.2.2): the 8-byte header, then one
//! custom section per artifact in the order given, then the rest of the
//! input unchanged. A reader that does not know the sections skips them, so
//! the result is still the same runnable component; a runtime that wants
//! native code extracts the section for its platform and loads it with
//! the same wasmtime version that compiled it.

use std::ops::Range;

/// `\0asm`, the magic both core modules and components start with.
const MAGIC: &[u8; 4] = b"\0asm";

/// Prefix of every embedded section name.
pub const PREFIX: &str = "aot-";

fn write_leb128(out: &mut Vec<u8>, mut v: usize) {
    loop {
        let byte = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

fn read_leb128(bytes: &[u8], pos: &mut usize) -> Result<usize, String> {
    let mut v: u64 = 0;
    let mut shift = 0;
    loop {
        let b = *bytes
            .get(*pos)
            .ok_or_else(|| format!("truncated LEB128 at offset {pos}"))?;
        *pos += 1;
        if shift >= 35 {
            return Err(format!("LEB128 longer than 5 bytes at offset {pos}"));
        }
        v |= u64::from(b & 0x7f) << shift;
        if b & 0x80 == 0 {
            return usize::try_from(v).map_err(|_| "LEB128 value too large".to_string());
        }
        shift += 7;
    }
}

fn check_header(wasm: &[u8]) -> Result<(), String> {
    if wasm.len() < 8 || &wasm[..4] != MAGIC {
        return Err("not a WebAssembly binary (missing \\0asm header)".into());
    }
    Ok(())
}

/// One custom section: `0x00`, size, name length, name, data.
pub fn custom_section(name: &str, data: &[u8]) -> Vec<u8> {
    let mut name_len = Vec::new();
    write_leb128(&mut name_len, name.len());
    let mut s = vec![0u8];
    write_leb128(&mut s, name_len.len() + name.len() + data.len());
    s.extend_from_slice(&name_len);
    s.extend_from_slice(name.as_bytes());
    s.extend_from_slice(data);
    s
}

/// The input with `aot-<name>` sections inserted after the header, in order.
pub fn embed(wasm: &[u8], artifacts: &[(String, Vec<u8>)]) -> Result<Vec<u8>, String> {
    check_header(wasm)?;
    let mut seen = std::collections::BTreeSet::new();
    for (name, _) in artifacts {
        if name.is_empty() || name.contains(':') {
            return Err(format!("invalid artifact name {name:?}"));
        }
        if !seen.insert(name) {
            return Err(format!("artifact name {name:?} given twice"));
        }
    }
    let extra: usize = artifacts.iter().map(|(n, d)| n.len() + d.len() + 16).sum();
    let mut out = Vec::with_capacity(wasm.len() + extra);
    out.extend_from_slice(&wasm[..8]);
    for (name, data) in artifacts {
        out.extend(custom_section(&format!("{PREFIX}{name}"), data));
    }
    out.extend_from_slice(&wasm[8..]);
    Ok(out)
}

/// A top-level section: its id, the byte range of the whole section and,
/// for a custom section, its name and the range of its data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    pub id: u8,
    pub range: Range<usize>,
    pub custom: Option<(String, Range<usize>)>,
}

/// Every top-level section of a module or component, in file order.
pub fn sections(wasm: &[u8]) -> Result<Vec<Section>, String> {
    check_header(wasm)?;
    let mut pos = 8;
    let mut out = Vec::new();
    while pos < wasm.len() {
        let start = pos;
        let id = wasm[pos];
        pos += 1;
        let size = read_leb128(wasm, &mut pos)?;
        let end = pos
            .checked_add(size)
            .filter(|e| *e <= wasm.len())
            .ok_or_else(|| format!("section at offset {start} runs past the end"))?;
        let custom = if id == 0 {
            let mut p = pos;
            let n = read_leb128(wasm, &mut p)?;
            let name_end = p
                .checked_add(n)
                .filter(|e| *e <= end)
                .ok_or_else(|| format!("custom section name at offset {start} runs past it"))?;
            let name = std::str::from_utf8(&wasm[p..name_end])
                .map_err(|_| format!("custom section name at offset {start} is not UTF-8"))?;
            Some((name.to_string(), name_end..end))
        } else {
            None
        };
        out.push(Section {
            id,
            range: start..end,
            custom,
        });
        pos = end;
    }
    Ok(out)
}

/// `(name without the aot- prefix, data range)` of each embedded artifact.
pub fn list(wasm: &[u8]) -> Result<Vec<(String, Range<usize>)>, String> {
    Ok(sections(wasm)?
        .into_iter()
        .filter_map(|s| s.custom)
        .filter_map(|(n, r)| n.strip_prefix(PREFIX).map(|n| (n.to_string(), r)))
        .collect())
}

/// The data of `aot-<name>`.
pub fn extract<'a>(wasm: &'a [u8], name: &str) -> Result<&'a [u8], String> {
    list(wasm)?
        .into_iter()
        .find(|(n, _)| n == name)
        .map(|(_, r)| &wasm[r])
        .ok_or_else(|| format!("no section {PREFIX}{name}"))
}

/// The input with every `aot-*` section removed: the component it was
/// embedded into.
pub fn strip(wasm: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = wasm[..8].to_vec();
    for s in sections(wasm)? {
        let embedded = matches!(&s.custom, Some((n, _)) if n.starts_with(PREFIX));
        if !embedded {
            out.extend_from_slice(&wasm[s.range]);
        }
    }
    Ok(out)
}
