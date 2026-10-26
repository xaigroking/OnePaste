//! Crockford Base32：字母表不含 I、L、O、U，便于人工抄写。仅供本 crate 内部使用。

pub(crate) const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// 把字节当成大端位流，每 5 位编成 1 个字符；末尾不足 5 位时在低位补 0。
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

/// 单个字符 → 5 位数值。不区分大小写；`O`→`0`，`I`/`L`→`1`；其他不在字母表内的字符返回 `None`。
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

/// 每 4 个字符一组，用 `-` 连接。
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
