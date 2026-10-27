//! Cryptographic primitives: Argon2id derivation, XChaCha20-Poly1305 AEAD and AAD context binding.
//! Rules: a random master key encrypts all data; the passphrase only derives a KEK that wraps the master key;
//! ciphertexts for different purposes use different AAD contexts and cannot be swapped.

use crate::error::Error;
use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::{
    aead::{Aead, KeyInit, Payload},
    XChaCha20Poly1305, XNonce,
};
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, ZeroizeOnDrop};

pub const KEY_LEN: usize = 32;
pub const NONCE_LEN: usize = 24;
pub const TAG_LEN: usize = 16;
pub const MIN_SALT_LEN: usize = 16;

/// A 32-byte key: zeroed on drop, `Debug` does not leak contents, comparison is constant time.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct Key([u8; KEY_LEN]);

impl Key {
    pub fn from_bytes(b: [u8; KEY_LEN]) -> Self {
        Key(b)
    }
    pub fn as_bytes(&self) -> &[u8; KEY_LEN] {
        &self.0
    }
    pub fn random() -> Result<Self, Error> {
        let mut k = [0u8; KEY_LEN];
        OsRng.try_fill_bytes(&mut k).map_err(|_| Error::Rng)?;
        Ok(Key(k))
    }
}

impl core::fmt::Debug for Key {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("Key(<redacted>)")
    }
}

impl PartialEq for Key {
    fn eq(&self, other: &Self) -> bool {
        let mut diff = 0u8;
        for (a, b) in self.0.iter().zip(other.0.iter()) {
            diff |= a ^ b;
        }
        diff == 0
    }
}
impl Eq for Key {}

/// Argon2id parameters: stored with the key record and upgradable; anything below the floor is rejected (downgrade protection).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KdfParams {
    pub m_kib: u32,
    pub t: u32,
    pub p: u32,
}

impl KdfParams {
    /// Floor: 8 MiB / 1 pass / 1 lane (downgrade protection only; see `DEFAULT` for the production default).
    pub const MIN: KdfParams = KdfParams { m_kib: 8 * 1024, t: 1, p: 1 };
    /// Default: 64 MiB / 3 passes / 1 lane. Browser timing is not measured yet; re-evaluate if it is too slow.
    pub const DEFAULT: KdfParams = KdfParams { m_kib: 64 * 1024, t: 3, p: 1 };

    pub fn validate(&self) -> Result<(), Error> {
        if self.m_kib < Self::MIN.m_kib || self.t < Self::MIN.t || self.p < Self::MIN.p {
            return Err(Error::WeakKdf);
        }
        Ok(())
    }
}

/// Passphrase to a 32-byte key (Argon2id v1.3).
pub fn derive_key(pass: &str, salt: &[u8], params: &KdfParams) -> Result<Key, Error> {
    params.validate()?;
    if salt.len() < MIN_SALT_LEN {
        return Err(Error::InvalidArgument("salt too short"));
    }
    let p = Params::new(params.m_kib, params.t, params.p, Some(KEY_LEN))
        .map_err(|_| Error::InvalidArgument("kdf params"))?;
    let mut out = [0u8; KEY_LEN];
    Argon2::new(Algorithm::Argon2id, Version::V0x13, p)
        .hash_password_into(pass.as_bytes(), salt, &mut out)
        .map_err(|_| Error::Internal)?;
    let key = Key(out);
    out.zeroize();
    Ok(key)
}

/// AAD contexts: bind a ciphertext to its purpose and identifier so it cannot be moved elsewhere.
pub mod aad {
    pub const MKWRAP: &[u8] = b"onepaste/mkwrap/v1";
    pub const MKCHECK: &[u8] = b"onepaste/mkcheck/v1";
    pub fn item(id: &str) -> Vec<u8> {
        format!("onepaste/item/v1:{id}").into_bytes()
    }
    pub fn blob(id: &str) -> Vec<u8> {
        format!("onepaste/blob/v1:{id}").into_bytes()
    }
    pub fn device(id: &str) -> Vec<u8> {
        format!("onepaste/device/v1:{id}").into_bytes()
    }
}

/// Encrypts and returns `nonce(24) || ciphertext || tag(16)`. The nonce is random every time.
pub fn seal(key: &Key, aad: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, Error> {
    let mut nonce = [0u8; NONCE_LEN];
    OsRng.try_fill_bytes(&mut nonce).map_err(|_| Error::Rng)?;
    seal_with_nonce(key, aad, plaintext, &nonce)
}

/// Encrypts with a fixed nonce: **for known-answer tests only**. Reusing a nonce destroys security.
#[doc(hidden)]
pub fn seal_with_nonce(
    key: &Key,
    aad: &[u8],
    plaintext: &[u8],
    nonce: &[u8; NONCE_LEN],
) -> Result<Vec<u8>, Error> {
    let cipher = XChaCha20Poly1305::new(key.as_bytes().into());
    let ct = cipher
        .encrypt(XNonce::from_slice(nonce), Payload { msg: plaintext, aad })
        .map_err(|_| Error::Internal)?;
    let mut out = Vec::with_capacity(NONCE_LEN + ct.len());
    out.extend_from_slice(nonce);
    out.extend_from_slice(&ct);
    Ok(out)
}

/// Decrypts the output of `seal`; any failure (key, AAD, tampering, truncation) returns the same error and does not reveal the cause.
pub fn open(key: &Key, aad: &[u8], sealed: &[u8]) -> Result<Vec<u8>, Error> {
    if sealed.len() < NONCE_LEN + TAG_LEN {
        return Err(Error::DecryptFailed);
    }
    let (nonce, ct) = sealed.split_at(NONCE_LEN);
    XChaCha20Poly1305::new(key.as_bytes().into())
        .decrypt(XNonce::from_slice(nonce), Payload { msg: ct, aad })
        .map_err(|_| Error::DecryptFailed)
}

#[cfg(test)]
mod tests {
    use super::*;

    const P: KdfParams = KdfParams::MIN;

    #[test]
    fn kdf_floor_rejects_downgrade() {
        let weak = KdfParams { m_kib: 64, t: 1, p: 1 };
        assert_eq!(derive_key("x", &[0u8; 16], &weak).unwrap_err(), Error::WeakKdf);
        assert!(matches!(derive_key("x", &[0u8; 8], &P), Err(Error::InvalidArgument(_))));
    }

    #[test]
    fn seal_open_roundtrip_and_context_binding() {
        let k = Key::random().unwrap();
        let sealed = seal(&k, &aad::item("a"), b"hello").unwrap();
        assert_eq!(open(&k, &aad::item("a"), &sealed).unwrap(), b"hello");
        assert_eq!(open(&k, &aad::item("b"), &sealed), Err(Error::DecryptFailed));
        assert_eq!(open(&k, &aad::blob("a"), &sealed), Err(Error::DecryptFailed));
        assert_eq!(open(&k, aad::MKWRAP, &sealed), Err(Error::DecryptFailed));
    }

    #[test]
    fn tamper_and_truncation_fail() {
        let k = Key::random().unwrap();
        let mut s = seal(&k, b"ctx", b"hello").unwrap();
        for i in [0, NONCE_LEN, s.len() - 1] {
            let mut bad = s.clone();
            bad[i] ^= 1;
            assert_eq!(open(&k, b"ctx", &bad), Err(Error::DecryptFailed));
        }
        s.truncate(NONCE_LEN + TAG_LEN - 1);
        assert_eq!(open(&k, b"ctx", &s), Err(Error::DecryptFailed));
    }

    #[test]
    fn nonces_are_unique() {
        let k = Key::random().unwrap();
        let a = seal(&k, b"c", b"same").unwrap();
        let b = seal(&k, b"c", b"same").unwrap();
        assert_ne!(a[..NONCE_LEN], b[..NONCE_LEN]);
    }

    #[test]
    fn key_debug_does_not_leak() {
        let k = Key::from_bytes([7u8; 32]);
        assert_eq!(format!("{k:?}"), "Key(<redacted>)");
    }
}
