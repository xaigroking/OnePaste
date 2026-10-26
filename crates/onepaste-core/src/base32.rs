//! Crockford Base32: the alphabet excludes I, L, O and U so it is easy to copy by hand. Internal to this crate.

pub(crate) const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// Treats the bytes as a big-endian bit stream and encodes 5 bits per character; a short tail is padded with zero bits.
pub(crate) fn encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity((bytes.len() * 8 + 4) / 5);
    let mut acc: u16 = 0;
    let mut bits = 0u32;
    for &b in bytes {
        acc = (acc << 8) | u16::from(b);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(ALPHABET[usize::from((acc >> bits) & 31)] as char);
        }
        acc &= (1 << bits) - 1;
    }
    if bits > 0 {
        out.push(ALPHABET[usize::from((acc << (5 - bits)) & 31)] as char);
    }
    out
}

/// One character to its 5-bit value. Case-insensitive; `O` maps to `0`, `I`/`L` map to `1`; any other character outside the alphabet returns `None`.
pub(crate) fn value(c: char) -> Option<u8> {
    let c = match c.to_ascii_uppercase() {
        'O' => '0',
        'I' | 'L' => '1',
        other => other,
    };
    if !c.is_ascii() {
        return None;
    }
    ALPHABET.iter().position(|&a| a as char == c).map(|i| i as u8)
}

/// Groups the characters in fours joined by `-`.
pub(crate) fn group4(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + s.len() / 4);
    for (i, c) in s.chars().enumerate() {
        if i > 0 && i % 4 == 0 {
            out.push('-');
        }
        out.push(c);
    }
    out
}
