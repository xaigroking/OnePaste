/// Reads width and height from the PNG header; returns `None` if it is not a valid PNG.
#[cfg_attr(not(any(windows, target_os = "macos")), allow(dead_code))]
pub(crate) fn dimensions(png: &[u8]) -> Option<(u32, u32)> {
    const SIG: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    if png.len() < 24 || png[..8] != SIG || &png[12..16] != b"IHDR" {
        return None;
    }
    let w = u32::from_be_bytes([png[16], png[17], png[18], png[19]]);
    let h = u32::from_be_bytes([png[20], png[21], png[22], png[23]]);
    Some((w, h))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_header_and_rejects_junk() {
        let mut p = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 13];
        p.extend_from_slice(b"IHDR");
        p.extend_from_slice(&3u32.to_be_bytes());
        p.extend_from_slice(&5u32.to_be_bytes());
        assert_eq!(dimensions(&p), Some((3, 5)));
        assert_eq!(dimensions(b"not a png"), None);
        assert_eq!(dimensions(&[]), None);
    }
}
