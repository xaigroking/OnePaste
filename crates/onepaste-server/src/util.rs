//! Small helpers: time, randomness, hashing, base64url and constant-time comparison.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD as B64URL, Engine};
use rand_core::{OsRng, RngCore};
use sha2::{Digest, Sha256};
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

pub fn unix_now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// The server clock. Tests move it forward instead of waiting for real time to pass.
#[derive(Default)]
pub struct Clock {
    offset: AtomicI64,
}

impl Clock {
    pub fn now(&self) -> i64 {
        unix_now() + self.offset.load(Ordering::Relaxed)
    }

    pub fn advance(&self, secs: i64) {
        self.offset.fetch_add(secs, Ordering::Relaxed);
    }
}

pub fn sha256(data: &[u8]) -> [u8; 32] {
    Sha256::digest(data).into()
}

pub fn b64url_encode(data: &[u8]) -> String {
    B64URL.encode(data)
}

/// Decodes unpadded base64url and requires exactly 32 bytes.
pub fn b64url_decode_32(s: &str) -> Option<[u8; 32]> {
    let v = B64URL.decode(s.as_bytes()).ok()?;
    v.try_into().ok()
}

/// base64url of 32 random bytes, used for device tokens and registration credentials.
pub fn random_token() -> String {
    let mut b = [0u8; 32];
    OsRng.fill_bytes(&mut b);
    b64url_encode(&b)
}

/// Constant-time comparison.
pub fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

pub fn hex(data: &[u8]) -> String {
    data.iter().map(|b| format!("{b:02x}")).collect()
}

/// Request id: UUIDv7.
pub fn request_id() -> String {
    onepaste_core::id::new_id().unwrap_or_else(|_| "0".to_string())
}
