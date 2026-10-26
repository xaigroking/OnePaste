//! 密钥层级：随机主密钥 MK 加密全部数据；口令派生 KEK 只用来包装 MK。
//! 改口令只重新包装 MK，不重加密任何条目。

use crate::crypto::{self, aad, KdfParams, Key, MIN_SALT_LEN};
use crate::error::Error;
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};

pub const KEYPARAMS_V: u8 = 1;

/// 保存在服务端/本地的密钥参数记录：盐是公开信息，被包装的 MK 没有口令不可恢复。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyParams {
    pub v: u8,
    pub salt: String,
    pub kdf: KdfParams,
    pub wrapped_mk: String,
}

impl KeyParams {
    /// 首台设备创建：随机盐 + 随机 MK，返回参数记录与 MK。
    pub fn create(pass: &str, kdf: KdfParams) -> Result<(Self, Key), Error> {
        let mk = Key::random()?;
        let rec = Self::wrap(pass, kdf, &mk)?;
        Ok((rec, mk))
    }

    fn wrap(pass: &str, kdf: KdfParams, mk: &Key) -> Result<Self, Error> {
        let mut salt = [0u8; MIN_SALT_LEN];
        OsRng.try_fill_bytes(&mut salt).map_err(|_| Error::Rng)?;
        let kek = crypto::derive_key(pass, &salt, &kdf)?;
        let wrapped = crypto::seal(&kek, aad::MKWRAP, mk.as_bytes())?;
        Ok(KeyParams { v: KEYPARAMS_V, salt: B64.encode(salt), kdf, wrapped_mk: B64.encode(wrapped) })
    }

    /// 用口令取回 MK。AEAD 校验失败即“口令错误”，同时充当口令校验，无需试解真实条目。
    pub fn unlock(&self, pass: &str) -> Result<Key, Error> {
        if self.v != KEYPARAMS_V {
            return Err(Error::UnsupportedVersion(self.v));
        }
        let salt = B64.decode(&self.salt).map_err(|_| Error::MalformedEnvelope)?;
        let wrapped = B64.decode(&self.wrapped_mk).map_err(|_| Error::MalformedEnvelope)?;
        let kek = crypto::derive_key(pass, &salt, &self.kdf)?;
        let mk = crypto::open(&kek, aad::MKWRAP, &wrapped).map_err(|_| Error::WrongPassphrase)?;
        let arr: [u8; 32] = mk.try_into().map_err(|_| Error::MalformedEnvelope)?;
        Ok(Key::from_bytes(arr))
    }

    /// 改口令：O(1)，换新盐重新包装同一把 MK；旧条目无需改动。
    pub fn change_passphrase(&self, old: &str, new: &str) -> Result<Self, Error> {
        let mk = self.unlock(old)?;
        Self::wrap(new, self.kdf, &mk)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KDF: KdfParams = KdfParams::MIN;

    #[test]
    fn create_unlock_and_wrong_passphrase() {
        let (rec, mk) = KeyParams::create("pw1", KDF).unwrap();
        assert_eq!(rec.unlock("pw1").unwrap(), mk);
        assert_eq!(rec.unlock("pw2").unwrap_err(), Error::WrongPassphrase);
    }

    #[test]
    fn change_passphrase_keeps_mk_and_invalidates_old() {
        let (rec, mk) = KeyParams::create("old", KDF).unwrap();
        let rec2 = rec.change_passphrase("old", "new").unwrap();
        assert_eq!(rec2.unlock("new").unwrap(), mk);
        assert_eq!(rec2.unlock("old").unwrap_err(), Error::WrongPassphrase);
        assert_ne!(rec.salt, rec2.salt);
    }

    #[test]
    fn rejects_unknown_version_and_weak_params() {
        let (mut rec, _) = KeyParams::create("pw", KDF).unwrap();
        rec.v = 9;
        assert_eq!(rec.unlock("pw").unwrap_err(), Error::UnsupportedVersion(9));
        let (mut rec, _) = KeyParams::create("pw", KDF).unwrap();
        rec.kdf.m_kib = 64; // 降级攻击：服务端把参数改弱
        assert_eq!(rec.unlock("pw").unwrap_err(), Error::WeakKdf);
    }

    #[test]
    fn record_json_has_no_plaintext_key_material() {
        let (rec, mk) = KeyParams::create("secret-passphrase-marker", KDF).unwrap();
        let json = serde_json::to_string(&rec).unwrap();
        assert!(!json.contains("secret-passphrase-marker"));
        assert!(!json.contains(&B64.encode(mk.as_bytes())));
    }
}
