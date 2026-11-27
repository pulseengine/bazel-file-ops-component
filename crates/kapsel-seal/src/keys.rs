//! Signing and trusted keys.
//!
//! Local development: an Ed25519 key pair under `$KAPSEL_HOME/keys/` (default
//! `~/.config/kapsel/keys/`), created on first use with mode 0600 for the
//! secret key. `--key FILE` loads an explicit secret key. CI keyless signing
//! (sigil `--keyless`: ephemeral key, Fulcio certificate, Rekor entry) is a
//! seam only: `SigningKey::keyless` exists behind the `keyless` feature and
//! returns `SealError::KeylessUnavailable` until sigil's keyless path is
//! wired in (needs the `wsc` `async` feature and an OIDC token in CI).

use crate::SealError;
use std::path::{Path, PathBuf};
use wsc::{KeyPair, PublicKey, PublicKeySet, SecretKey};

const SECRET_NAME: &str = "kapsel.key";
const PUBLIC_NAME: &str = "kapsel.pub";

pub struct SigningKey {
    pub(crate) pair: KeyPair,
}

impl SigningKey {
    pub fn generate() -> Self {
        SigningKey { pair: KeyPair::generate() }
    }

    /// Loads a secret key file (as written by [`SigningKey::write`]).
    pub fn from_file(path: &Path) -> Result<Self, SealError> {
        let sk = SecretKey::from_file(path).map_err(|e| SealError::Key(e.to_string()))?;
        let pk = PublicKey { pk: sk.sk.public_key(), key_id: None };
        Ok(SigningKey { pair: KeyPair { pk, sk } })
    }

    /// Writes the secret key (mode 0600) to `secret` and the public key to
    /// `public` in sigil's raw format, which is what `wsc verify -K` reads.
    pub fn write(&self, secret: &Path, public: &Path) -> Result<(), SealError> {
        self.pair.sk.to_file(secret).map_err(|e| SealError::Key(e.to_string()))?;
        self.pair.pk.to_file(public).map_err(|e| SealError::Key(e.to_string()))?;
        Ok(())
    }

    pub fn public_pem(&self) -> String {
        self.pair.pk.to_pem()
    }

    /// The key pair kept under `dir` (usually [`keys_dir`]), created on first use.
    pub fn load_or_create(dir: &Path) -> Result<Self, SealError> {
        let secret = dir.join(SECRET_NAME);
        if secret.exists() {
            return Self::from_file(&secret);
        }
        std::fs::create_dir_all(dir)?;
        let key = Self::generate();
        key.write(&secret, &dir.join(PUBLIC_NAME))?;
        Ok(key)
    }

    /// The trust set containing only this key's public half.
    pub fn trusted(&self) -> TrustedKeys {
        let mut t = TrustedKeys::empty();
        t.add(self.pair.pk.clone());
        t
    }

    /// Seam for CI keyless signing. Not implemented.
    #[cfg(feature = "keyless")]
    pub fn keyless() -> Result<Self, SealError> {
        Err(SealError::KeylessUnavailable)
    }
}

/// Public keys whose signatures [`crate::verify`] accepts.
#[derive(Clone)]
pub struct TrustedKeys {
    pub(crate) set: PublicKeySet,
}

impl TrustedKeys {
    pub fn empty() -> Self {
        TrustedKeys { set: PublicKeySet::empty() }
    }

    pub(crate) fn add(&mut self, pk: PublicKey) {
        let _ = self.set.insert(pk);
    }

    /// Adds a PEM or DER public key file.
    pub fn add_file(&mut self, path: &Path) -> Result<(), SealError> {
        let bytes = std::fs::read(path)?;
        let pk = match std::str::from_utf8(&bytes) {
            Ok(s) if s.contains("BEGIN") => PublicKey::from_pem(s),
            // sigil's raw format (`wsc keygen`): 0x01 then the 32 key bytes.
            _ if bytes.first() == Some(&0x01) => PublicKey::from_bytes(&bytes),
            _ => PublicKey::from_der(&bytes),
        }
        .map_err(|e| SealError::Key(e.to_string()))?;
        self.add(pk);
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.set.len()
    }

    pub fn is_empty(&self) -> bool {
        self.set.is_empty()
    }
}

/// `$KAPSEL_HOME` if set and non-empty.
fn kapsel_home() -> Option<PathBuf> {
    std::env::var_os("KAPSEL_HOME").filter(|v| !v.is_empty()).map(PathBuf::from)
}

fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."))
}

/// `$KAPSEL_HOME/keys`, default `~/.config/kapsel/keys`.
pub fn keys_dir() -> PathBuf {
    match kapsel_home() {
        Some(h) => h.join("keys"),
        None => home().join(".config/kapsel/keys"),
    }
}

/// `$KAPSEL_HOME/cache`, default `~/.cache/kapsel`.
pub fn cache_dir() -> PathBuf {
    match kapsel_home() {
        Some(h) => h.join("cache"),
        None => home().join(".cache/kapsel"),
    }
}
