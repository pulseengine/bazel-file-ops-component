use super::*;
use kapsel_plan::{Call, Grants, PAndOr, PLAN_SCHEMA, PPipeline, PStage, PWord, Span};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

fn plan(source: &str, argv: &[&str], grants: Grants, span_shift: u32) -> Plan {
    let call = Call {
        tool: None,
        argv: argv.iter().map(|a| PWord::literal(*a)).collect(),
        assignments: vec![],
        redirects: vec![],
        span: Span::new(span_shift, span_shift + 7),
    };
    Plan {
        schema: PLAN_SCHEMA.into(),
        source_sha256: hex_sha256(source.as_bytes()),
        body: vec![PAndOr {
            first: PPipeline {
                negated: false,
                stages: vec![PStage::Call(call)],
            },
            rest: vec![],
        }],
        grants,
        warnings: vec![],
    }
}

fn tmp(name: &str) -> PathBuf {
    static N: AtomicU32 = AtomicU32::new(0);
    let d = std::env::temp_dir().join(format!(
        "kapsel-seal-{}-{}-{name}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// A valid empty component, the stand-in for the compiler's output.
fn stand_in(_: &Plan) -> Result<Vec<u8>, SealError> {
    Ok(wat::parse_str("(component)").unwrap())
}

const SRC: &str = "echo hi";

fn sealed(key: &SigningKey) -> (Plan, Vec<u8>) {
    let p = plan(SRC, &["echo", "hi"], Grants::default(), 0);
    let m = seal(&p, SRC, stand_in(&p).unwrap(), key).unwrap();
    (p, m)
}

#[test]
fn round_trip_returns_both_input_hashes() {
    let key = SigningKey::generate();
    let (p, m) = sealed(&key);
    let att = verify(&m, &p.normalized_sha256(), &key.trusted()).unwrap();
    let by = |n: &str| {
        att.inputs
            .iter()
            .find(|i| i.artifact.name == n)
            .unwrap()
            .artifact
            .clone()
    };
    assert_eq!(by(INPUT_SOURCE).hash, p.source_sha256);
    assert_eq!(by(INPUT_SOURCE).size, SRC.len() as u64);
    assert_eq!(by(INPUT_NORMALIZED_PLAN).hash, p.normalized_sha256());
    assert_eq!(
        by(INPUT_NORMALIZED_PLAN).size,
        serde_json::to_vec(&p.normalized()).unwrap().len() as u64
    );
    assert_eq!(att.tool.name, "kapsel-compile");
    assert_eq!(att.tool.version, COMPILER_VERSION);
    assert_eq!(att.tool.parameters["wit"], "kapsel:shell@0.1.0");
    assert_eq!(att.transformation_type.to_string(), "custom");
}

#[test]
fn source_must_match_the_plan() {
    let key = SigningKey::generate();
    let p = plan(SRC, &["echo", "hi"], Grants::default(), 0);
    let r = seal(&p, "something else", stand_in(&p).unwrap(), &key);
    assert!(matches!(r, Err(SealError::SourceMismatch)));
}

#[test]
fn flipped_byte_fails() {
    let key = SigningKey::generate();
    let (p, m) = sealed(&key);
    let trust = key.trusted();
    // Flip every byte in turn: no single-byte change may verify.
    for i in 0..m.len() {
        let mut bad = m.clone();
        bad[i] ^= 0x01;
        assert!(
            verify(&bad, &p.normalized_sha256(), &trust).is_err(),
            "flip at {i} verified"
        );
    }
}

#[test]
fn different_key_fails() {
    let key = SigningKey::generate();
    let (p, m) = sealed(&key);
    let other = SigningKey::generate();
    assert!(matches!(
        verify(&m, &p.normalized_sha256(), &other.trusted()),
        Err(SealError::BadSignature)
    ));
    assert!(verify(&m, &p.normalized_sha256(), &TrustedKeys::empty()).is_err());
}

#[test]
fn different_expected_hash_fails() {
    let key = SigningKey::generate();
    let (_, m) = sealed(&key);
    let other = plan(SRC, &["echo", "bye"], Grants::default(), 0);
    assert!(matches!(
        verify(&m, &other.normalized_sha256(), &key.trusted()),
        Err(SealError::Attestation(_))
    ));
}

#[test]
fn stripped_signature_fails() {
    let key = SigningKey::generate();
    let (p, m) = sealed(&key);
    let mut module = parse(&m).unwrap();
    module.sections.retain(|s| !matches!(s, Section::Custom(c) if c.is_signature_header() || c.is_signature_delimiter()));
    let mut stripped = Vec::new();
    module.serialize(&mut stripped).unwrap();
    assert!(matches!(
        verify(&stripped, &p.normalized_sha256(), &key.trusted()),
        Err(SealError::NotSigned)
    ));
}

#[test]
fn signed_without_attestation_fails() {
    let key = SigningKey::generate();
    let p = plan(SRC, &["echo", "hi"], Grants::default(), 0);
    let bare = parse(&stand_in(&p).unwrap()).unwrap();
    let signed = key.pair.sk.sign(bare, None).unwrap();
    let mut bytes = Vec::new();
    signed.serialize(&mut bytes).unwrap();
    assert!(matches!(
        verify(&bytes, &p.normalized_sha256(), &key.trusted()),
        Err(SealError::MissingAttestation)
    ));
}

#[test]
fn wsc_cli_accepts_a_library_sealed_module() {
    let dir = tmp("cli");
    let key = SigningKey::generate();
    let (_, m) = sealed(&key);
    let (module, pubkey) = (dir.join("m.wasm"), dir.join("k.pub"));
    std::fs::write(&module, &m).unwrap();
    key.pair.pk.to_file(&pubkey).unwrap();
    let run = |pk: &PathBuf| {
        std::process::Command::new("wsc")
            .args(["verify", "-K"])
            .arg(pk)
            .arg("-i")
            .arg(&module)
            .output()
            .ok()
    };
    let Some(ok) = run(&pubkey) else {
        eprintln!("wsc not on PATH: cross-check skipped");
        return;
    };
    assert!(
        ok.status.success(),
        "wsc verify failed: {}{}",
        String::from_utf8_lossy(&ok.stdout),
        String::from_utf8_lossy(&ok.stderr)
    );
    // Negative control: another key must be refused by the CLI too.
    let other = dir.join("other.pub");
    SigningKey::generate().pair.pk.to_file(&other).unwrap();
    assert!(!run(&other).unwrap().status.success());
}

#[test]
fn key_is_created_once_with_mode_0600_and_reloads() {
    let dir = tmp("keys");
    let a = SigningKey::load_or_create(&dir).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(dir.join("kapsel.key"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
    }
    let b = SigningKey::load_or_create(&dir).unwrap();
    assert_eq!(a.public_pem(), b.public_pem());
    // The explicit-key path (`--key FILE`) reads the same file.
    let c = SigningKey::from_file(&dir.join("kapsel.key")).unwrap();
    assert_eq!(a.public_pem(), c.public_pem());
    let mut t = TrustedKeys::empty();
    t.add_file(&dir.join("kapsel.pub")).unwrap();
    let (p, m) = sealed(&a);
    verify(&m, &p.normalized_sha256(), &t).unwrap();
}

// ---- cache ----

fn run(plan: &Plan, source: &str, cache: &Cache, key: &SigningKey) -> (Vec<u8>, CacheOutcome) {
    compile_sealed_with(plan, source, cache, key, stand_in).unwrap()
}

#[test]
fn whitespace_only_change_hits_and_argv_change_misses() {
    let key = SigningKey::generate();
    let cache = Cache::new(tmp("cache"));
    let a = plan("echo hi", &["echo", "hi"], Grants::default(), 0);
    let (m1, o1) = run(&a, "echo hi", &cache, &key);
    assert_eq!(o1, CacheOutcome::Miss(MissReason::Absent));

    let b = plan("echo    hi # note", &["echo", "hi"], Grants::default(), 4);
    assert_ne!(a.source_sha256, b.source_sha256);
    let (m2, o2) = run(&b, "echo    hi # note", &cache, &key);
    assert_eq!(o2, CacheOutcome::Hit);
    assert_eq!(m1, m2);

    let c = plan("echo bye", &["echo", "bye"], Grants::default(), 0);
    let (_, o3) = run(&c, "echo bye", &cache, &key);
    assert_eq!(o3, CacheOutcome::Miss(MissReason::Absent));
}

#[test]
fn a_fresh_module_returned_on_a_miss_verifies() {
    // The miss path verifies before returning; this pins the contract that
    // whatever reaches the host on a miss passes the same check a cache hit does.
    let key = SigningKey::generate();
    let cache = Cache::new(tmp("cache-miss-verify"));
    let p = plan("echo hi", &["echo", "hi"], Grants::default(), 0);
    let (m, o) = run(&p, "echo hi", &cache, &key);
    assert_eq!(o, CacheOutcome::Miss(MissReason::Absent));
    let att = verify(&m, &p.normalized_sha256(), &key.trusted()).unwrap();
    assert!(
        att.inputs
            .iter()
            .any(|i| i.artifact.hash == p.normalized_sha256())
    );
}

#[test]
fn a_miss_whose_fresh_module_does_not_verify_is_refused_and_not_cached() {
    // REQ-KS-021: the miss path calls `verify`. The module is signed by a key
    // the trust set lacks, so only that call can stop it.
    let key = SigningKey::generate();
    let other = SigningKey::generate();
    let cache = Cache::new(tmp("miss-untrusted"));
    let p = plan(SRC, &["echo", "hi"], Grants::default(), 0);

    // Control: with the signing key trusted the same call succeeds and caches.
    let ok = compile_sealed_trusting(&p, SRC, &cache, &key, &key.trusted(), stand_in);
    assert!(matches!(ok, Ok((_, CacheOutcome::Miss(_)))), "{ok:?}");
    let file = cache.dir().join(format!("{}.wasm", Cache::key(&p)));
    assert!(file.exists());
    std::fs::remove_file(&file).unwrap();

    for trust in [TrustedKeys::empty(), other.trusted()] {
        let r = compile_sealed_trusting(&p, SRC, &cache, &key, &trust, stand_in);
        assert!(matches!(r, Err(SealError::BadSignature)), "{r:?}");
        assert!(!file.exists(), "an unverified module must not be cached");
    }
}

#[test]
fn tampered_cache_file_is_rejected_and_removed() {
    let key = SigningKey::generate();
    let cache = Cache::new(tmp("tamper"));
    let p = plan(SRC, &["echo", "hi"], Grants::default(), 0);
    run(&p, SRC, &cache, &key);
    let file = cache.dir().join(format!("{}.wasm", Cache::key(&p)));
    let mut bytes = std::fs::read(&file).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 0xff;
    std::fs::write(&file, &bytes).unwrap();
    match cache.get(&p, &key.trusted()) {
        Lookup::Miss(MissReason::Rejected(why)) => assert!(!why.is_empty()),
        _ => panic!("tampered entry must be rejected"),
    }
    assert!(!file.exists(), "rejected entry must be deleted");
    // And the next run recompiles and re-caches.
    let (_, o) = run(&p, SRC, &cache, &key);
    assert_eq!(o, CacheOutcome::Miss(MissReason::Absent));
}

#[test]
fn wrongly_keyed_cache_entry_is_rejected_and_removed() {
    let cache = Cache::new(tmp("wrongkey"));
    let p = plan(SRC, &["echo", "hi"], Grants::default(), 0);
    run(&p, SRC, &cache, &SigningKey::generate());
    let file = cache.dir().join(format!("{}.wasm", Cache::key(&p)));
    let (_, o) = run(&p, SRC, &cache, &SigningKey::generate());
    assert!(matches!(o, CacheOutcome::Miss(MissReason::Rejected(_))));
    assert!(file.exists(), "the recompiled entry replaced it");

    // An entry sealed for another plan, moved under this plan's key: rejected.
    let q = plan(SRC, &["echo", "other"], Grants::default(), 0);
    let key = SigningKey::generate();
    let other = seal(&q, SRC, stand_in(&q).unwrap(), &key).unwrap();
    std::fs::write(&file, other).unwrap();
    assert!(matches!(
        cache.get(&p, &key.trusted()),
        Lookup::Miss(MissReason::Rejected(_))
    ));
    assert!(!file.exists());
}

#[test]
fn concurrent_put_of_one_key_does_not_corrupt() {
    let key = SigningKey::generate();
    let cache = std::sync::Arc::new(Cache::new(tmp("concurrent")));
    let p = plan(SRC, &["echo", "hi"], Grants::default(), 0);
    let (_, sealed_module) = sealed(&key);
    let trust = key.trusted();
    let mut handles = vec![];
    for _ in 0..8 {
        let (c, p, m, t) = (
            cache.clone(),
            p.clone(),
            sealed_module.clone(),
            trust.clone(),
        );
        handles.push(std::thread::spawn(move || {
            for _ in 0..25 {
                c.put(&p, &m).unwrap();
                // A reader never sees a torn entry.
                match c.get(&p, &t) {
                    Lookup::Hit(b) => assert_eq!(b, m),
                    Lookup::Miss(MissReason::Absent) => {}
                    Lookup::Miss(MissReason::Rejected(r)) => panic!("torn entry: {r}"),
                }
            }
        }));
    }
    for h in handles {
        h.join().unwrap();
    }
    match cache.get(&p, &trust) {
        Lookup::Hit(b) => assert_eq!(b, sealed_module),
        _ => panic!("entry missing after concurrent puts"),
    }
    let leftovers: Vec<_> = std::fs::read_dir(cache.dir())
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with(".tmp-"))
        .collect();
    assert!(leftovers.is_empty());
}

#[test]
fn grants_change_the_key_but_never_enter_a_module() {
    let g1 = Grants {
        read: vec!["/secret/alpha-input".into()],
        write: vec![],
        dynamic: false,
        native: vec![],
    };
    let g2 = Grants {
        read: vec![],
        write: vec!["/secret/beta-output".into()],
        dynamic: false,
        native: vec![],
    };
    let a = plan(SRC, &["echo", "hi"], g1, 0);
    let b = plan(SRC, &["echo", "hi"], g2, 0);
    assert_ne!(Cache::key(&a), Cache::key(&b));
    assert_ne!(a.normalized_sha256(), b.normalized_sha256());

    let key = SigningKey::generate();
    let cache = Cache::new(tmp("grants"));
    for (p, path) in [(&a, "/secret/alpha-input"), (&b, "/secret/beta-output")] {
        let (m, o) = run(p, SRC, &cache, &key);
        assert_eq!(o, CacheOutcome::Miss(MissReason::Absent));
        assert!(
            !m.windows(path.len()).any(|w| w == path.as_bytes()),
            "{path} appears in the module"
        );
    }
}
