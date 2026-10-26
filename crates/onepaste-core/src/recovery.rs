//! Recovery kit: encodes the master key (MK) as a string a person can copy down; it is the only way back if the passphrase is lost.
//!
//! Byte layout (35 bytes): `version(1, fixed 0x01) || MK(32) || check(2)`;
//! check = the first 2 bytes of `SHA-256("onepaste/recovery/v1" || version || MK)`.
//! Encoding: the 35 bytes are read as a big-endian bit stream, 5 bits per Crockford Base32 character, 56 characters in total;
//! shown in upper case, in groups of 4 joined by `-`.

use crate::base32;
use crate::crypto::{Key, KEY_LEN};
use crate::error::Error;
use sha2::{Digest, Sha256};
use zeroize::Zeroize;

pub const KIT_VERSION: u8 = 0x01;
const KIT_BYTES: usize = 1 + KEY_LEN + 2;
const KIT_CHARS: usize = 56;
const CHECK_CONTEXT: &[u8] = b"onepaste/recovery/v1";

fn checksum(version: u8, mk: &[u8]) -> [u8; 2] {
    let mut h = Sha256::new();
    h.update(CHECK_CONTEXT);
    h.update([version]);
    h.update(mk);
    let d = h.finalize();
    [d[0], d[1]]
}

/// Encodes the MK as a recovery kit (`XXXX-XXXX-...`, 14 groups).
pub fn encode(mk: &Key) -> String {
    let mut buf = [0u8; KIT_BYTES];
    buf[0] = KIT_VERSION;
    buf[1..1 + KEY_LEN].copy_from_slice(mk.as_bytes());
    let sum = checksum(KIT_VERSION, mk.as_bytes());
    buf[1 + KEY_LEN..].copy_from_slice(&sum);
    let text = base32::encode(&buf);
    buf.zeroize();
    base32::group4(&text)
}

/// Decodes a recovery kit. Tolerant input: case-insensitive, ignores `-`, spaces and newlines, `O` maps to `0`, `I`/`L` map to `1`.
pub fn decode(kit: &str) -> Result<Key, Error> {
    let mut vals = [0u8; KIT_CHARS];
    let mut n = 0usize;
    for c in kit.chars() {
        if c == '-' || c.is_whitespace() {
            continue;
        }
        match base32::value(c) {
            Some(v) if n < KIT_CHARS => {
                vals[n] = v;
                n += 1;
            }
            _ => {
                vals.zeroize();
                return Err(Error::InvalidRecoveryKit);
            }
        }
    }
    if n != KIT_CHARS {
        vals.zeroize();
        return Err(Error::InvalidRecoveryKit);
    }

    let mut buf = [0u8; KIT_BYTES];
    for (k, v) in vals.iter().enumerate() {
        for b in 0..5 {
            let idx = k * 5 + b;
            buf[idx / 8] |= ((v >> (4 - b)) & 1) << (7 - idx % 8);
        }
    }
    vals.zeroize();

    // Check the checksum before the version: a random miscopy is always "invalid"; only a valid checksum with an unknown version is "unsupported version".
    let result = if buf[1 + KEY_LEN..] != checksum(buf[0], &buf[1..1 + KEY_LEN]) {
        Err(Error::InvalidRecoveryKit)
    } else if buf[0] != KIT_VERSION {
        Err(Error::UnsupportedVersion(buf[0]))
    } else {
        let mut mk = [0u8; KEY_LEN];
        mk.copy_from_slice(&buf[1..1 + KEY_LEN]);
        let key = Key::from_bytes(mk);
        mk.zeroize();
        Ok(key)
    };
    buf.zeroize();
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    const KIT_SEQ: &str = "0400-20G3-0G2G-C1R8-1450-P30D-1R7H-048J-2CA1-A5GQ-30CH-M6RW-3MF1-YQBR";
    const KIT_FF: &str = "07ZZ-ZZZZ-ZZZZ-ZZZZ-ZZZZ-ZZZZ-ZZZZ-ZZZZ-ZZZZ-ZZZZ-ZZZZ-ZZZZ-ZZZZ-ZKZ9";

    fn seq_key() -> Key {
        let mut b = [0u8; 32];
        for (i, x) in b.iter_mut().enumerate() {
            *x = i as u8;
        }
        Key::from_bytes(b)
    }

    #[test]
    fn known_vectors_encode_and_decode() {
        assert_eq!(encode(&seq_key()), KIT_SEQ);
        assert_eq!(decode(KIT_SEQ).unwrap(), seq_key());
        let ff = Key::from_bytes([0xff; 32]);
        assert_eq!(encode(&ff), KIT_FF);
        assert_eq!(decode(KIT_FF).unwrap(), ff);
    }

    #[test]
    fn random_roundtrip_100_times() {
        for _ in 0..100 {
            let mk = Key::random().unwrap();
            let kit = encode(&mk);
            assert_eq!(kit.len(), 56 + 13);
            assert_eq!(decode(&kit).unwrap(), mk);
        }
    }

    #[test]
    fn any_single_character_change_is_rejected() {
        for kit in [KIT_SEQ, KIT_FF] {
            let chars: Vec<char> = kit.chars().collect();
            for i in (0..chars.len()).filter(|&i| chars[i] != '-') {
                for &alt in base32::ALPHABET.iter() {
                    if alt as char == chars[i] {
                        continue;
                    }
                    let mut m = chars.clone();
                    m[i] = alt as char;
                    let s: String = m.into_iter().collect();
                    assert_eq!(decode(&s).unwrap_err(), Error::InvalidRecoveryKit, "{s}");
                }
            }
        }
    }

    #[test]
    fn tolerant_input_forms() {
        let want = seq_key();
        assert_eq!(decode(&KIT_SEQ.to_lowercase()).unwrap(), want);
        assert_eq!(decode(&KIT_SEQ.replace('-', "")).unwrap(), want);
        assert_eq!(decode(&KIT_SEQ.replace('-', " \n ")).unwrap(), want);
        // Write every `0` as `O` and every `1` as `l`
        assert_eq!(decode(&KIT_SEQ.replace('0', "O").replace('1', "l")).unwrap(), want);
        assert_eq!(decode(&KIT_SEQ.replace('1', "I")).unwrap(), want);
    }

    #[test]
    fn rejects_bad_characters_and_lengths() {
        assert_eq!(decode(&KIT_SEQ.replacen('0', "U", 1)).unwrap_err(), Error::InvalidRecoveryKit);
        assert_eq!(decode(&KIT_SEQ.replace('4', "$")).unwrap_err(), Error::InvalidRecoveryKit);
        assert_eq!(decode("").unwrap_err(), Error::InvalidRecoveryKit);
        assert_eq!(decode(&KIT_SEQ[..KIT_SEQ.len() - 1]).unwrap_err(), Error::InvalidRecoveryKit);
        assert_eq!(decode(&format!("{KIT_SEQ}0")).unwrap_err(), Error::InvalidRecoveryKit);
        assert_eq!(decode("é").unwrap_err(), Error::InvalidRecoveryKit);
    }

    #[test]
    fn unknown_version_with_valid_checksum_is_unsupported() {
        let mk = seq_key();
        let mut buf = [0u8; KIT_BYTES];
        buf[0] = 2;
        buf[1..33].copy_from_slice(mk.as_bytes());
        buf[33..].copy_from_slice(&checksum(2, mk.as_bytes()));
        let kit = base32::group4(&base32::encode(&buf));
        assert_eq!(decode(&kit).unwrap_err(), Error::UnsupportedVersion(2));
    }
}
