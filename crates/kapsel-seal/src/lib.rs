//! Sealing, verifying and caching compiled modules (REQ-KS-021, DD-KS-005).
//!
//! Borrowed methods (DD-KS-005): batch records, meaning every change to the
//! master record makes a new batch (the cache key covers the normalized plan,
//! compiler version and WIT version), and forensic seals, meaning signed when
//! made and checked before use ([`seal`] and [`verify`]).
//!
//! Built on sigil, in-process through the `wsc` and `wsc-attestation` crates:
//! * `TransformationAttestationBuilder` builds the attestation (type custom,
//!   tool `kapsel-compile`, parameter `wit`), signed with Ed25519 through
//!   `build_and_sign_ed25519`.
//! * `wsc::composition::embed_transformation_attestation` puts it in the
//!   `wsc.transformation.attestation` custom section.
//! * `SecretKey::sign` signs the whole module; `PublicKeySet::verify` checks it.
//!
//! Grants are host state and never reach a module or its attestation: they are
//! part of the normalized plan, so they change the cache key, but only the
//! plan's hash is attested.

mod cache;
mod keys;

pub use cache::{Cache, CacheOutcome, Lookup, MissReason};
pub use keys::{SigningKey, TrustedKeys, cache_dir, keys_dir};
pub use wsc_attestation::TransformationAttestation as Attestation;

use kapsel_plan::Plan;
use sha2::{Digest, Sha256};
use wsc::composition::{embed_transformation_attestation, extract_transformation_attestation};
use wsc::wasm_module::{Module, Section};
use wsc_attestation::{TransformationAttestationBuilder, TransformationType};

/// The WIT package the compiled component targets.
pub const WIT_VERSION: &str = "kapsel:shell@0.1.0";
/// The compiler version recorded in attestations and in the cache key.
pub const COMPILER_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const TOOL_NAME: &str = "kapsel-compile";
/// Names of the two attested inputs.
pub const INPUT_SOURCE: &str = "source";
pub const INPUT_NORMALIZED_PLAN: &str = "normalized-plan";

#[derive(Debug)]
pub enum SealError {
    /// The module is not parseable wasm.
    Malformed(String),
    /// No signature section, or no trusted key made a valid signature.
    NotSigned,
    BadSignature,
    MissingAttestation,
    /// The attestation is present but does not describe this module or plan.
    Attestation(String),
    /// `source` does not hash to `plan.source_sha256`.
    SourceMismatch,
    Key(String),
    Io(std::io::Error),
    Compile(String),
    /// CI keyless signing is a seam only (see `keys.rs`).
    #[cfg(feature = "keyless")]
    KeylessUnavailable,
}

impl std::fmt::Display for SealError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SealError::Malformed(m) => write!(f, "malformed module: {m}"),
            SealError::NotSigned => f.write_str("module is not signed"),
            SealError::BadSignature => {
                f.write_str("signature does not verify against a trusted key")
            }
            SealError::MissingAttestation => {
                f.write_str("module has no transformation attestation")
            }
            SealError::Attestation(m) => write!(f, "attestation rejected: {m}"),
            SealError::SourceMismatch => {
                f.write_str("source text does not match the plan's source hash")
            }
            SealError::Key(m) => write!(f, "key error: {m}"),
            SealError::Io(e) => write!(f, "io error: {e}"),
            SealError::Compile(m) => write!(f, "compile error: {m}"),
            #[cfg(feature = "keyless")]
            SealError::KeylessUnavailable => f.write_str("keyless signing is not implemented yet"),
        }
    }
}

impl std::error::Error for SealError {}

impl From<std::io::Error> for SealError {
    fn from(e: std::io::Error) -> Self {
        SealError::Io(e)
    }
}

pub(crate) fn hex_sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn normalized_json(plan: &Plan) -> Vec<u8> {
    serde_json::to_vec(&plan.normalized()).expect("plan serializes")
}

fn parse(module: &[u8]) -> Result<Module, SealError> {
    Module::deserialize(&mut &module[..]).map_err(|e| SealError::Malformed(e.to_string()))
}

/// Embeds a sigil transformation attestation in `module` and signs it.
///
/// Inputs are `source` (hash `plan.source_sha256`) and the normalized plan
/// (hash `plan.normalized_sha256()`); the output is `module` before signing.
pub fn seal(
    plan: &Plan,
    source: &str,
    module: Vec<u8>,
    key: &SigningKey,
) -> Result<Vec<u8>, SealError> {
    if hex_sha256(source.as_bytes()) != plan.source_sha256 {
        return Err(SealError::SourceMismatch);
    }
    let parsed = parse(&module)?;
    let norm = normalized_json(plan);
    debug_assert_eq!(hex_sha256(&norm), plan.normalized_sha256());

    let attestation = TransformationAttestationBuilder::new(
        TransformationType::Custom,
        TOOL_NAME,
        COMPILER_VERSION,
    )
    .add_parameter("wit", serde_json::Value::String(WIT_VERSION.into()))
    .add_input_unsigned(source.as_bytes(), INPUT_SOURCE)
    .add_input_unsigned(&norm, INPUT_NORMALIZED_PLAN)
    .build_and_sign_ed25519(&module, "module.wasm", key.pair.sk.sk.as_ref(), None)
    .map_err(SealError::Key)?;

    let embedded = embed_transformation_attestation(parsed, &attestation)
        .map_err(|e| SealError::Malformed(e.to_string()))?;
    let signed = key
        .pair
        .sk
        .sign(embedded, None)
        .map_err(|e| SealError::Key(e.to_string()))?;
    let mut out = Vec::with_capacity(module.len() + 1024);
    signed
        .serialize(&mut out)
        .map_err(|e| SealError::Malformed(e.to_string()))?;
    Ok(out)
}

/// Checks a sealed module: the signature verifies against a trusted key, the
/// attestation is present, it describes this module, and its normalized-plan
/// input hash equals `expected_normalized`.
pub fn verify(
    module: &[u8],
    expected_normalized: &str,
    trust: &TrustedKeys,
) -> Result<Attestation, SealError> {
    let parsed = parse(module)?;
    match parsed.sections.first() {
        Some(Section::Custom(c)) if c.is_signature_header() => {}
        _ => return Err(SealError::NotSigned),
    }
    match trust.set.verify(&mut &module[..], None) {
        Ok(good) if !good.is_empty() => {}
        Ok(_) => return Err(SealError::BadSignature),
        Err(_) => return Err(SealError::BadSignature),
    }

    let att = extract_transformation_attestation(&parsed)
        .map_err(|e| SealError::Attestation(e.to_string()))?
        .ok_or(SealError::MissingAttestation)?;
    if att.tool.name != TOOL_NAME {
        return Err(SealError::Attestation(format!(
            "tool is {}, not {TOOL_NAME}",
            att.tool.name
        )));
    }
    let attested = att
        .inputs
        .iter()
        .find(|i| i.artifact.name == INPUT_NORMALIZED_PLAN)
        .ok_or_else(|| SealError::Attestation("no normalized-plan input".into()))?;
    if attested.artifact.hash != expected_normalized {
        return Err(SealError::Attestation(format!(
            "attested plan hash {} differs from expected {expected_normalized}",
            attested.artifact.hash
        )));
    }
    // The attested output must be this module without the seal.
    let mut bare = parsed.clone();
    bare.sections.retain(|s| match s {
        Section::Custom(c) => {
            !c.is_signature_header()
                && !c.is_signature_delimiter()
                && c.name() != wsc_attestation::TRANSFORMATION_ATTESTATION_SECTION
        }
        _ => true,
    });
    let mut bytes = Vec::new();
    bare.serialize(&mut bytes)
        .map_err(|e| SealError::Malformed(e.to_string()))?;
    if hex_sha256(&bytes) != att.output.hash {
        return Err(SealError::Attestation(
            "attested output hash does not match the module".into(),
        ));
    }
    Ok(att)
}

/// What the CLI and kapsel-db call: compile (or fetch from the cache) and
/// seal. Compiles the normalized plan with `kapsel_compile::compile`.
pub fn compile_sealed(
    plan: &Plan,
    source: &str,
    cache: &Cache,
    key: &SigningKey,
) -> Result<(Vec<u8>, CacheOutcome), SealError> {
    compile_sealed_with(plan, source, cache, key, |p| {
        kapsel_compile::compile(p).map_err(|e| SealError::Compile(e.to_string()))
    })
}

/// [`compile_sealed`] with an injectable compiler (tests, and callers that
/// already hold compiled bytes).
pub fn compile_sealed_with(
    plan: &Plan,
    source: &str,
    cache: &Cache,
    key: &SigningKey,
    compiler: impl FnOnce(&Plan) -> Result<Vec<u8>, SealError>,
) -> Result<(Vec<u8>, CacheOutcome), SealError> {
    compile_sealed_trusting(plan, source, cache, key, &key.trusted(), compiler)
}

/// [`compile_sealed_with`] with the trust set a module is checked against
/// given by the caller instead of derived from `key`. Exists so a test can
/// sign with a key the verifier does not trust; production callers use
/// [`compile_sealed`].
#[doc(hidden)]
pub fn compile_sealed_trusting(
    plan: &Plan,
    source: &str,
    cache: &Cache,
    key: &SigningKey,
    trust: &TrustedKeys,
    compiler: impl FnOnce(&Plan) -> Result<Vec<u8>, SealError>,
) -> Result<(Vec<u8>, CacheOutcome), SealError> {
    let reason = match cache.get(plan, trust) {
        Lookup::Hit(m) => return Ok((m, CacheOutcome::Hit)),
        Lookup::Miss(r) => r,
    };
    let module = compiler(&plan.normalized())?;
    let sealed = seal(plan, source, module, key)?;
    // A fresh module is verified exactly as a cached one is, so no module
    // reaches the host without a signature and attestation that check out.
    verify(&sealed, &plan.normalized_sha256(), trust)?;
    cache.put(plan, &sealed)?;
    Ok((sealed, CacheOutcome::Miss(reason)))
}

#[cfg(test)]
mod tests;
