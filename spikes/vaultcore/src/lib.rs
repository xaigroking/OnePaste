//! vaultcore spike v2: wrapped master key (random MK, passphrase-derived KEK wraps it),
//! domain-separated AEAD contexts, and an auth token that is independent of the vault key.
use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::{aead::{Aead, KeyInit, Payload}, XChaCha20Poly1305, XNonce};
use rand_core::{OsRng, RngCore};

pub type Key = [u8; 32];

fn argon(pass: &str, salt: &[u8], mem_kib: u32) -> Key {
    let p = Params::new(mem_kib, 3, 1, Some(32)).unwrap();
    let mut out = [0u8; 32];
    Argon2::new(Algorithm::Argon2id, Version::V0x13, p)
        .hash_password_into(pass.as_bytes(), salt, &mut out).unwrap();
    out
}
/// KEK: wraps the master key. Salt is per-vault and stored (public) next to the wrapped key.
pub fn derive_kek(pass: &str, vault_salt: &[u8], mem_kib: u32) -> Key { argon(pass, vault_salt, mem_kib) }
/// Self-host auth token: a *different* Argon2 run with a *server-issued* salt (not an app-wide
/// constant), so tokens are not precomputable across servers and never equal the KEK.
pub fn derive_auth_token(pass: &str, server_salt: &[u8], mem_kib: u32) -> Key { argon(pass, server_salt, mem_kib) }

pub fn seal(key: &Key, aad: &[u8], pt: &[u8]) -> Vec<u8> {
    let c = XChaCha20Poly1305::new(key.into());
    let mut n = [0u8; 24]; OsRng.fill_bytes(&mut n);
    let mut out = n.to_vec();
    out.extend(c.encrypt(XNonce::from_slice(&n), Payload { msg: pt, aad }).unwrap());
    out
}
pub fn open(key: &Key, aad: &[u8], blob: &[u8]) -> Option<Vec<u8>> {
    if blob.len() < 24 + 16 { return None }
    XChaCha20Poly1305::new(key.into())
        .decrypt(XNonce::from_slice(&blob[..24]), Payload { msg: &blob[24..], aad }).ok()
}

const AAD_MKWRAP: &[u8] = b"vault.mkwrap.v1";
pub fn new_master_key() -> Key { let mut k = [0u8; 32]; OsRng.fill_bytes(&mut k); k }
pub fn wrap_mk(kek: &Key, mk: &Key) -> Vec<u8> { seal(kek, AAD_MKWRAP, mk) }
/// `None` = wrong passphrase (the AEAD failure doubles as the key check).
pub fn unwrap_mk(kek: &Key, wrapped: &[u8]) -> Option<Key> {
    open(kek, AAD_MKWRAP, wrapped).and_then(|v| v.try_into().ok())
}
/// Re-wrap under a new passphrase: O(1), no item is re-encrypted.
pub fn change_passphrase(old_kek: &Key, new_kek: &Key, wrapped: &[u8]) -> Option<Vec<u8>> {
    unwrap_mk(old_kek, wrapped).map(|mk| wrap_mk(new_kek, &mk))
}
pub fn item_aad(uid: &str) -> Vec<u8> { format!("vault.item.v1:{uid}").into_bytes() }
pub fn blob_aad(id: &str) -> Vec<u8> { format!("vault.blob.v1:{id}").into_bytes() }

#[cfg(test)] mod t { use super::*;
  const M: u32 = 8 * 1024; // test-only memory; production uses >= 64 MiB
  #[test] fn kek_token_and_mk_are_independent() {
    let kek = derive_kek("correct horse", b"vault-salt-0123456", M);
    let tok = derive_auth_token("correct horse", b"server-salt-012345", M);
    assert_ne!(kek, tok);
    assert_eq!(kek, derive_kek("correct horse", b"vault-salt-0123456", M));   // deterministic
    assert_ne!(kek, derive_kek("correct horse", b"other-salt-0123456", M));    // per-vault salt matters
    assert_ne!(tok, derive_auth_token("correct horse", b"other-server-salt1", M)); // not precomputable across servers
  }
  #[test] fn wrap_unwrap_and_wrong_passphrase() {
    let mk = new_master_key();
    let kek = derive_kek("pw1", b"vault-salt-0123456", M);
    let w = wrap_mk(&kek, &mk);
    assert_eq!(unwrap_mk(&kek, &w).unwrap(), mk);
    assert!(unwrap_mk(&derive_kek("wrong", b"vault-salt-0123456", M), &w).is_none());
  }
  #[test] fn passphrase_change_does_not_touch_items() {
    let mk = new_master_key();
    let (k1, k2) = (derive_kek("old", b"s1-vault-salt-0123", M), derive_kek("new", b"s1-vault-salt-0123", M));
    let item = seal(&mk, &item_aad("u1"), b"secret clip");            // encrypted once
    let w2 = change_passphrase(&k1, &k2, &wrap_mk(&k1, &mk)).unwrap();  // passphrase rotated
    let mk2 = unwrap_mk(&k2, &w2).unwrap();
    assert_eq!(open(&mk2, &item_aad("u1"), &item).unwrap(), b"secret clip"); // old item still opens
    assert!(unwrap_mk(&k1, &w2).is_none());                                  // old passphrase no longer works
  }
  #[test] fn aad_binds_context_and_tamper_fails() {
    let mk = new_master_key();
    let c = seal(&mk, &item_aad("u1"), b"x");
    assert!(open(&mk, &item_aad("u2"), &c).is_none());   // moved to another item
    assert!(open(&mk, &blob_aad("u1"), &c).is_none());   // moved to a blob context
    assert!(open(&mk, AAD_MKWRAP, &c).is_none());        // masquerading as a wrapped key
    let mut bad = c.clone(); *bad.last_mut().unwrap() ^= 1;
    assert!(open(&mk, &item_aad("u1"), &bad).is_none());
    let mut short = c.clone(); short.truncate(30); assert!(open(&mk, &item_aad("u1"), &short).is_none());
  }
  #[test] fn nonces_are_unique_per_seal() {
    let mk = new_master_key();
    let a = seal(&mk, b"a", b"same"); let b = seal(&mk, b"a", b"same");
    assert_ne!(a[..24], b[..24]); assert_ne!(a, b);
  }
}
