//! 条目标识：UUIDv7（时间有序，客户端生成）。服务端只把它当不透明字符串。

use crate::error::Error;
use rand_core::{OsRng, RngCore};

pub const MAX_ID_LEN: usize = 64;

/// 以给定毫秒时间戳生成 UUIDv7（wasm 下没有系统时钟，由调用方传入）。
pub fn new_id_at(unix_ms: u64) -> Result<String, Error> {
    let mut b = [0u8; 16];
    OsRng.try_fill_bytes(&mut b).map_err(|_| Error::Rng)?;
    b[..6].copy_from_slice(&unix_ms.to_be_bytes()[2..]);
    b[6] = (b[6] & 0x0f) | 0x70; // 版本 7
    b[8] = (b[8] & 0x3f) | 0x80; // 变体 10
    let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
    Ok(format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32]))
}

#[cfg(not(target_arch = "wasm32"))]
pub fn new_id() -> Result<String, Error> {
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| Error::Internal)?
        .as_millis() as u64;
    new_id_at(ms)
}

/// 标识只允许 `[A-Za-z0-9_-]`，长度 1..=64（防路径穿越、日志注入与超长）。
pub fn validate_id(id: &str) -> Result<(), Error> {
    if id.is_empty() || id.len() > MAX_ID_LEN {
        return Err(Error::InvalidArgument("id length"));
    }
    if !id.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_') {
        return Err(Error::InvalidArgument("id charset"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uuid_v7_shape_and_order() {
        let a = new_id_at(1_700_000_000_000).unwrap();
        let b = new_id_at(1_700_000_000_001).unwrap();
        assert_eq!(a.len(), 36);
        assert_eq!(a.as_bytes()[14], b'7');
        assert!(matches!(a.as_bytes()[19], b'8' | b'9' | b'a' | b'b'));
        assert!(a < b, "时间有序");
        validate_id(&a).unwrap();
    }

    #[test]
    fn id_validation_blocks_traversal_and_junk() {
        for bad in ["", "../x", "a/b", "a b", "x\n", &"a".repeat(65), "é"] {
            assert!(validate_id(bad).is_err(), "{bad:?}");
        }
    }
}
