//! The module cache at `$KAPSEL_HOME/cache/` (REQ-KS-021).
//!
//! Key: sha256 of `normalized_sha256 \0 compiler version \0 WIT version`.
//! An entry is returned only after [`crate::verify`] passes against the
//! caller's trusted keys and the key's own plan hash; anything else is
//! deleted and reported as a miss with a reason.

use crate::{COMPILER_VERSION, SealError, TrustedKeys, WIT_VERSION, hex_sha256, verify};
use kapsel_plan::Plan;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MissReason {
    Absent,
    /// The entry failed verification and was deleted.
    Rejected(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CacheOutcome {
    Hit,
    Miss(MissReason),
}

pub enum Lookup {
    Hit(Vec<u8>),
    Miss(MissReason),
}

pub struct Cache {
    dir: PathBuf,
}

static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

impl Cache {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Cache { dir: dir.into() }
    }

    /// The cache under `$KAPSEL_HOME/cache` (default `~/.cache/kapsel`).
    pub fn default_location() -> Self {
        Self::new(crate::keys::cache_dir())
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The cache key for a plan: layout-only source changes share it.
    pub fn key(plan: &Plan) -> String {
        hex_sha256(format!("{}\0{}\0{}", plan.normalized_sha256(), COMPILER_VERSION, WIT_VERSION).as_bytes())
    }

    fn path(&self, plan: &Plan) -> PathBuf {
        self.dir.join(format!("{}.wasm", Self::key(plan)))
    }

    pub fn get(&self, plan: &Plan, trust: &TrustedKeys) -> Lookup {
        let path = self.path(plan);
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(_) => return Lookup::Miss(MissReason::Absent),
        };
        match verify(&bytes, &plan.normalized_sha256(), trust) {
            Ok(_) => Lookup::Hit(bytes),
            Err(e) => {
                let _ = std::fs::remove_file(&path);
                Lookup::Miss(MissReason::Rejected(e.to_string()))
            }
        }
    }

    /// Writes a sealed module atomically: temp file in the same directory, then rename.
    pub fn put(&self, plan: &Plan, sealed: &[u8]) -> Result<(), SealError> {
        std::fs::create_dir_all(&self.dir)?;
        let n = TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let tmp = self.dir.join(format!(".tmp-{}-{n}", std::process::id()));
        std::fs::write(&tmp, sealed)?;
        if let Err(e) = std::fs::rename(&tmp, self.path(plan)) {
            let _ = std::fs::remove_file(&tmp);
            return Err(e.into());
        }
        Ok(())
    }
}
