//! 条目信封：服务端可见字段是**逐字段列出**的，其余全部在密文里。
//! 服务端可见字段：
//! - `v`：版本号，当前为 1，不认识就拒绝
//! - `id`：UUIDv7，由客户端生成，只允许 `[A-Za-z0-9_-]`，长度不超过 64
//! - `created_at` / `updated_at`：秒级时间戳，`updated_at ≥ created_at`
//! - `size`：必须等于 `sealed` 解码后的字节数
//! - `blob_id`（可选）：字符集规则同 `id`
//! - `sealed`：base64(`nonce(24) ‖ 密文 ‖ 标签(16)`)，算法 XChaCha20-Poly1305
//! 内容、标题、预览、标签、来源应用、设备名、文件名一律在 `sealed` 内。

use crate::crypto::{self, aad, Key, NONCE_LEN, TAG_LEN};
use crate::error::Error;
use crate::id::validate_id;
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

pub const ENVELOPE_V: u8 = 1;
/// 单条密文上限的默认值（服务端可配置）。
pub const DEFAULT_MAX_SEALED_BYTES: usize = 1 << 20;

/// 服务端可见的元数据 + 一段密文。未知字段在**读取时忽略**（向后兼容：字段只增不减，新增字段必须可选）。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Envelope {
    pub v: u8,
    pub id: String,
    pub created_at: i64,
    pub updated_at: i64,
    /// `sealed` 解码后的字节数；服务端据此做配额与合理性校验，无需密钥。
    pub size: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blob_id: Option<String>,
    /// base64(nonce || 密文 || 标签)
    pub sealed: String,
}

/// 密文里的私有负载。`extra` 保留未知字段，**读改写不丢字段**（避免旧客户端吃掉新字段）。
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ItemPayload {
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_app: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Envelope {
    /// 解析并校验（服务端与客户端共用）。不接触任何密钥。
    pub fn from_json(json: &str, max_sealed_bytes: usize) -> Result<Self, Error> {
        let env: Envelope = serde_json::from_str(json).map_err(|_| Error::MalformedEnvelope)?;
        env.validate(max_sealed_bytes)?;
        Ok(env)
    }

    pub fn validate(&self, max_sealed_bytes: usize) -> Result<(), Error> {
        if self.v != ENVELOPE_V {
            return Err(Error::UnsupportedVersion(self.v));
        }
        validate_id(&self.id)?;
        if let Some(b) = &self.blob_id {
            validate_id(b)?;
        }
        if self.created_at < 0 || self.updated_at < self.created_at {
            return Err(Error::MalformedEnvelope);
        }
        // 先看长度再解码，避免对超大输入做无谓分配
        if self.sealed.len() > max_sealed_bytes.saturating_mul(4) / 3 + 8 {
            return Err(Error::TooLarge);
        }
        let raw = B64.decode(&self.sealed).map_err(|_| Error::MalformedEnvelope)?;
        if raw.len() < NONCE_LEN + TAG_LEN {
            return Err(Error::MalformedEnvelope);
        }
        if raw.len() > max_sealed_bytes {
            return Err(Error::TooLarge);
        }
        if raw.len() as u64 != self.size {
            return Err(Error::MalformedEnvelope); // 声明大小与实际不符（少报=占便宜）
        }
        Ok(())
    }

    pub fn to_json(&self) -> Result<String, Error> {
        serde_json::to_string(self).map_err(|_| Error::Internal)
    }
}

/// 加密一个条目。`id` 经 AAD 绑定进密文：把密文挪到别的条目下会解密失败。
pub fn seal_item(
    mk: &Key,
    id: &str,
    created_at: i64,
    updated_at: i64,
    blob_id: Option<String>,
    payload: &ItemPayload,
) -> Result<Envelope, Error> {
    validate_id(id)?;
    let plain = serde_json::to_vec(payload).map_err(|_| Error::Internal)?;
    let sealed = crypto::seal(mk, &aad::item(id), &plain)?;
    let env = Envelope {
        v: ENVELOPE_V,
        id: id.to_owned(),
        created_at,
        updated_at,
        size: sealed.len() as u64,
        blob_id,
        sealed: B64.encode(&sealed),
    };
    env.validate(usize::MAX / 8)?;
    Ok(env)
}

pub fn open_item(mk: &Key, env: &Envelope) -> Result<ItemPayload, Error> {
    env.validate(usize::MAX / 8)?;
    let raw = B64.decode(&env.sealed).map_err(|_| Error::MalformedEnvelope)?;
    let plain = crypto::open(mk, &aad::item(&env.id), &raw)?;
    serde_json::from_slice(&plain).map_err(|_| Error::MalformedEnvelope)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload(text: &str) -> ItemPayload {
        ItemPayload { kind: "text".into(), content: Some(text.into()), ..Default::default() }
    }

    #[test]
    fn roundtrip() {
        let mk = Key::random().unwrap();
        let env = seal_item(&mk, "item-1", 10, 10, None, &payload("你好")).unwrap();
        assert_eq!(open_item(&mk, &env).unwrap().content.as_deref(), Some("你好"));
    }

    #[test]
    fn server_visible_json_never_contains_plaintext() {
        // 单元级探针：植入标记串，信封 JSON 里不得出现（服务端可见部分不含明文）
        let mk = Key::random().unwrap();
        let mut p = payload("PROBE-7f3a-secret-content");
        p.title = Some("PROBE-title".into());
        p.preview = Some("PROBE-preview".into());
        p.tags = vec!["PROBE-tag".into()];
        p.source_app = Some("PROBE-app".into());
        let json = seal_item(&mk, "item-1", 1, 1, None, &p).unwrap().to_json().unwrap();
        assert!(!json.contains("PROBE"), "信封泄漏了明文: {json}");
    }

    #[test]
    fn moving_ciphertext_to_another_item_fails() {
        let mk = Key::random().unwrap();
        let mut env = seal_item(&mk, "a", 1, 1, None, &payload("x")).unwrap();
        env.id = "b".into();
        assert_eq!(open_item(&mk, &env).unwrap_err(), Error::DecryptFailed);
    }

    #[test]
    fn wrong_key_fails() {
        let env = seal_item(&Key::random().unwrap(), "a", 1, 1, None, &payload("x")).unwrap();
        assert_eq!(open_item(&Key::random().unwrap(), &env).unwrap_err(), Error::DecryptFailed);
    }

    #[test]
    fn unknown_envelope_fields_are_ignored_on_read() {
        let mk = Key::random().unwrap();
        let env = seal_item(&mk, "a", 1, 1, None, &payload("x")).unwrap();
        let mut v: Value = serde_json::from_str(&env.to_json().unwrap()).unwrap();
        v["future_field"] = Value::from("whatever");
        let back = Envelope::from_json(&v.to_string(), DEFAULT_MAX_SEALED_BYTES).unwrap();
        assert_eq!(back, env);
    }

    #[test]
    fn unknown_payload_fields_survive_read_modify_write() {
        let mk = Key::random().unwrap();
        let mut p = payload("x");
        p.extra.insert("from_newer_client".into(), Value::from(42));
        let env = seal_item(&mk, "a", 1, 1, None, &p).unwrap();
        let mut got = open_item(&mk, &env).unwrap();
        got.title = Some("edited".into());
        let env2 = seal_item(&mk, "a", 1, 2, None, &got).unwrap();
        assert_eq!(open_item(&mk, &env2).unwrap().extra.get("from_newer_client"), Some(&Value::from(42)));
    }

    #[test]
    fn validation_rejects_bad_input() {
        let mk = Key::random().unwrap();
        let good = seal_item(&mk, "a", 1, 1, None, &payload("x")).unwrap();

        let mut e = good.clone();
        e.v = 2;
        assert_eq!(e.validate(1024), Err(Error::UnsupportedVersion(2)));

        let mut e = good.clone();
        e.size -= 1; // 少报大小
        assert_eq!(e.validate(1024), Err(Error::MalformedEnvelope));

        let mut e = good.clone();
        e.id = "../etc".into();
        assert!(matches!(e.validate(1024), Err(Error::InvalidArgument(_))));

        let mut e = good.clone();
        e.sealed = "!!!not base64".into();
        assert_eq!(e.validate(1024), Err(Error::MalformedEnvelope));

        assert_eq!(good.validate(10), Err(Error::TooLarge));

        let mut e = good.clone();
        e.updated_at = 0;
        e.created_at = 5;
        assert_eq!(e.validate(1024), Err(Error::MalformedEnvelope));
    }

    #[test]
    fn malformed_json_is_an_error_not_a_panic() {
        for bad in ["", "{}", "null", "[", "{\"v\":1}", &"x".repeat(100_000)] {
            assert!(Envelope::from_json(bad, 1024).is_err());
        }
    }
}
