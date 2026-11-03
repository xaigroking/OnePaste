//! Word 97–2003 main-document text from MS-DOC FIB/CLX, not a scan of arbitrary binary strings.
use super::{bounded, error, MAX_PREVIEW_TEXT};
use crate::AppError;
use std::io::{Cursor, Read};
fn u16le(b: &[u8], at: usize) -> Result<u16, AppError> {
    let s = b.get(at..at + 2).ok_or(error("preview_corrupt"))?;
    Ok(u16::from_le_bytes([s[0], s[1]]))
}
fn u32le(b: &[u8], at: usize) -> Result<u32, AppError> {
    let s = b.get(at..at + 4).ok_or(error("preview_corrupt"))?;
    Ok(u32::from_le_bytes(s.try_into().unwrap()))
}
fn stream(cfb: &mut cfb::CompoundFile<Cursor<&[u8]>>, name: &str) -> Result<Vec<u8>, AppError> {
    let mut s = cfb
        .open_stream(name)
        .map_err(|_| error("preview_corrupt"))?;
    let max = onepaste_core::attachment::MAX_FILE_BYTES;
    if s.len() > max {
        return Err(error("preview_limit"));
    }
    let mut data = Vec::new();
    (&mut s)
        .take(max + 1)
        .read_to_end(&mut data)
        .map_err(|_| error("preview_corrupt"))?;
    if data.len() as u64 > max {
        return Err(error("preview_limit"));
    }
    Ok(data)
}
pub(super) fn text(bytes: &[u8]) -> Result<String, AppError> {
    let mut file =
        cfb::CompoundFile::open(Cursor::new(bytes)).map_err(|_| error("preview_corrupt"))?;
    if file.exists("/EncryptedPackage") || file.exists("/EncryptionInfo") {
        return Err(error("preview_encrypted"));
    }
    let word = zeroize::Zeroizing::new(stream(&mut file, "/WordDocument")?);
    if u16le(&word, 0)? != 0xa5ec {
        return Err(error("preview_corrupt"));
    }
    let version = u16le(&word, 2)?;
    if !(0xc1..=0x112).contains(&version) {
        return Err(error("preview_unsupported"));
    }
    let flags = u16le(&word, 10)?;
    if flags & 0x100 != 0 {
        return Err(error("preview_encrypted"));
    }
    let table = stream(
        &mut file,
        if flags & 0x200 != 0 {
            "/1Table"
        } else {
            "/0Table"
        },
    )?;
    let csw = u16le(&word, 32)? as usize;
    if csw > 256 {
        return Err(error("preview_corrupt"));
    }
    let cslw_at = 34 + csw * 2;
    let cslw = u16le(&word, cslw_at)? as usize;
    if !(4..=256).contains(&cslw) {
        return Err(error("preview_corrupt"));
    }
    let main_len = u32le(&word, cslw_at + 2 + 12)? as usize;
    if main_len > MAX_PREVIEW_TEXT {
        return Err(error("preview_limit"));
    }
    let cb_at = cslw_at + 2 + cslw * 4;
    if u16le(&word, cb_at)? < 34 {
        return Err(error("preview_corrupt"));
    }
    let fc = u32le(&word, cb_at + 2 + 33 * 8)? as usize;
    let len = u32le(&word, cb_at + 2 + 33 * 8 + 4)? as usize;
    if len > 8 * 1024 * 1024 {
        return Err(error("preview_limit"));
    }
    let clx = table
        .get(fc..fc.checked_add(len).ok_or(error("preview_corrupt"))?)
        .ok_or(error("preview_corrupt"))?;
    let mut at = 0;
    while clx.get(at) == Some(&1) {
        let n = u16le(clx, at + 1)? as usize;
        at = at.checked_add(3 + n).ok_or(error("preview_corrupt"))?;
    }
    if clx.get(at) != Some(&2) {
        return Err(error("preview_corrupt"));
    }
    let size = u32le(clx, at + 1)? as usize;
    if size < 4 || (size - 4) % 12 != 0 {
        return Err(error("preview_corrupt"));
    }
    let pieces = (size - 4) / 12;
    if pieces > 200_000 {
        return Err(error("preview_limit"));
    }
    let p = clx
        .get(at + 5..at + 5 + size)
        .ok_or(error("preview_corrupt"))?;
    if u32le(p, 0)? != 0 || (u32le(p, pieces * 4)? as usize) < main_len {
        return Err(error("preview_corrupt"));
    }
    let mut units = zeroize::Zeroizing::new(Vec::with_capacity(main_len));
    for i in 0..pieces {
        let start = u32le(p, i * 4)? as usize;
        let end = u32le(p, (i + 1) * 4)? as usize;
        if end < start {
            return Err(error("preview_corrupt"));
        }
        if start >= main_len {
            break;
        }
        let count = end.min(main_len) - start;
        let fc = u32le(p, (pieces + 1) * 4 + i * 8 + 2)?;
        let compressed = fc & 0x40000000 != 0;
        let offset = (fc & 0x3fffffff) as usize / if compressed { 2 } else { 1 };
        let length = count * if compressed { 1 } else { 2 };
        let raw = word
            .get(offset..offset.checked_add(length).ok_or(error("preview_corrupt"))?)
            .ok_or(error("preview_corrupt"))?;
        if compressed {
            const SPECIAL: [u16; 32] = [
                0x80, 0x81, 0x201a, 0x192, 0x201e, 0x2026, 0x2020, 0x2021, 0x2c6, 0x2030, 0x160,
                0x2039, 0x152, 0x8d, 0x8e, 0x8f, 0x90, 0x2018, 0x2019, 0x201c, 0x201d, 0x2022,
                0x2013, 0x2014, 0x2dc, 0x2122, 0x161, 0x203a, 0x153, 0x9d, 0x9e, 0x178,
            ];
            units.extend(raw.iter().map(|b| {
                if (0x80..=0x9f).contains(b) {
                    SPECIAL[(*b - 0x80) as usize]
                } else {
                    *b as u16
                }
            }));
        } else {
            units.extend(
                raw.chunks_exact(2)
                    .map(|s| u16::from_le_bytes([s[0], s[1]])),
            );
        }
    }
    if units.len() != main_len {
        return Err(error("preview_corrupt"));
    }
    let raw =
        zeroize::Zeroizing::new(String::from_utf16(&units).map_err(|_| error("preview_encoding"))?);
    let mut output = String::new();
    let mut fields = Vec::new();
    for c in raw.chars() {
        match c {
            '\u{13}' => {
                if fields.len() >= 64 {
                    return Err(error("preview_limit"));
                }
                fields.push(false);
            }
            '\u{14}' => {
                if let Some(top) = fields.last_mut() {
                    *top = true;
                }
            }
            '\u{15}' => {
                fields.pop();
            }
            _ if fields.iter().all(|v| *v) => match c {
                '\r' | '\u{7}' | '\u{c}' => output.push('\n'),
                '\t' | '\n' => output.push(c),
                _ if !c.is_control() => output.push(c),
                _ => {}
            },
            _ => {}
        }
    }
    bounded(output)
}
