//! Default passphrase generation: gives the user a random strong passphrase on first setup.

use crate::base32;
use crate::error::Error;
use rand_core::{OsRng, RngCore};
use zeroize::Zeroize;

const CHARS: usize = 20; // 20 x 5 = 100 bits of entropy
const RAND_BYTES: usize = 13; // 104 bits, of which the first 100 are used

/// Generates a passphrase like `7K3M-QX9D-...`: 20 Crockford Base32 characters in 5 groups of 4.
pub fn generate() -> Result<String, Error> {
    let mut raw = [0u8; RAND_BYTES];
    OsRng.try_fill_bytes(&mut raw).map_err(|_| Error::Rng)?;
    let mut text = base32::encode(&raw);
    raw.zeroize();
    text.truncate(CHARS);
    let out = base32::group4(&text);
    text.zeroize();
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn format_is_five_groups_of_four_in_alphabet() {
        let p = generate().unwrap();
        let groups: Vec<&str> = p.split('-').collect();
        assert_eq!(groups.len(), 5);
        for g in groups {
            assert_eq!(g.len(), 4);
            assert!(g.bytes().all(|b| base32::ALPHABET.contains(&b)), "{p}");
        }
    }

    #[test]
    fn thousand_generations_are_unique() {
        let set: HashSet<String> = (0..1000).map(|_| generate().unwrap()).collect();
        assert_eq!(set.len(), 1000);
    }
}
