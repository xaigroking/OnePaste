//! Versioned, private attachment descriptors and independently authenticated ciphertext chunks.
use crate::{
    crypto::{self, Key},
    envelope::ItemPayload,
    id::validate_id,
    keys::derive_subkey,
    Error,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};

pub const VERSION: u8 = 1;
pub const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;
pub const CHUNK_BYTES: usize = 256 * 1024;
pub const MAX_CHUNKS: u32 = 256;
pub const MAX_SEALED_CHUNK: usize = CHUNK_BYTES + crypto::NONCE_LEN + crypto::TAG_LEN;
const KEY_PURPOSE: &[u8] = b"onepaste/attachment-key/v1";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Attachment {
    pub v: u8,
    pub blob_id: String,
    pub file_name: String,
    pub mime: String,
    pub size: u64,
    pub sha256: String,
    pub chunk_bytes: u32,
    pub chunks: u32,
}

pub fn chunk_count(size: u64) -> u32 {
    size.div_ceil(CHUNK_BYTES as u64)
        .max(1)
        .min(u32::MAX as u64) as u32
}
pub fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

impl Attachment {
    pub fn validate(&self) -> Result<(), Error> {
        if self.v != VERSION {
            return Err(Error::UnsupportedVersion(self.v));
        }
        validate_id(&self.blob_id)?;
        if self.size > MAX_FILE_BYTES {
            return Err(Error::TooLarge);
        }
        if self.chunk_bytes != CHUNK_BYTES as u32
            || self.chunks != chunk_count(self.size)
            || self.chunks > MAX_CHUNKS
        {
            return Err(Error::InvalidArgument("attachment chunks"));
        }
        if self.file_name.is_empty()
            || self.file_name.chars().count() > 255
            || [".", ".."].contains(&self.file_name.as_str())
            || self
                .file_name
                .chars()
                .any(|c| c.is_control() || c == '/' || c == '\\')
        {
            return Err(Error::InvalidArgument("attachment name"));
        }
        if self.mime.is_empty()
            || self.mime.len() > 127
            || !self
                .mime
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"/.-_+".contains(&c))
        {
            return Err(Error::InvalidArgument("attachment mime"));
        }
        if self.sha256.len() != 64
            || !self
                .sha256
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
        {
            return Err(Error::InvalidArgument("attachment digest"));
        }
        Ok(())
    }

    pub fn payload(&self) -> Result<ItemPayload, Error> {
        self.validate()?;
        let mut payload = ItemPayload {
            kind: "file".into(),
            title: Some(self.file_name.clone()),
            ..Default::default()
        };
        payload.extra.insert(
            "attachment".into(),
            serde_json::to_value(self).map_err(|_| Error::Internal)?,
        );
        payload.extra.insert("mime".into(), json!(self.mime));
        payload.extra.insert("size".into(), json!(self.size));
        Ok(payload)
    }

    pub fn from_payload(payload: &ItemPayload) -> Result<Self, Error> {
        if payload.kind != "file" || payload.content.is_some() {
            return Err(Error::InvalidArgument("not an attachment"));
        }
        let value = payload
            .extra
            .get("attachment")
            .ok_or(Error::InvalidArgument("missing attachment"))?;
        let descriptor: Self = serde_json::from_value(value.clone())
            .map_err(|_| Error::InvalidArgument("attachment descriptor"))?;
        descriptor.validate()?;
        Ok(descriptor)
    }

    pub fn plain_chunk_len(&self, index: u32) -> Result<usize, Error> {
        self.validate()?;
        if index >= self.chunks {
            return Err(Error::InvalidArgument("attachment index"));
        }
        Ok((self.size - u64::from(index) * CHUNK_BYTES as u64).min(CHUNK_BYTES as u64) as usize)
    }

    fn aad(&self, item_id: &str, index: u32) -> Result<Vec<u8>, Error> {
        validate_id(item_id)?;
        self.plain_chunk_len(index)?;
        Ok(format!(
            "onepaste/attachment-chunk/v1:{item_id}:{}:{index}:{}",
            self.blob_id, self.chunks
        )
        .into_bytes())
    }

    pub fn seal_chunk(
        &self,
        mk: &Key,
        item_id: &str,
        index: u32,
        plain: &[u8],
    ) -> Result<Vec<u8>, Error> {
        if plain.len() != self.plain_chunk_len(index)? {
            return Err(Error::InvalidArgument("attachment chunk length"));
        }
        crypto::seal(
            &derive_subkey(mk, KEY_PURPOSE),
            &self.aad(item_id, index)?,
            plain,
        )
    }

    pub fn open_chunk(
        &self,
        mk: &Key,
        item_id: &str,
        index: u32,
        sealed: &[u8],
    ) -> Result<Vec<u8>, Error> {
        if sealed.len() != self.plain_chunk_len(index)? + crypto::NONCE_LEN + crypto::TAG_LEN {
            return Err(Error::DecryptFailed);
        }
        crypto::open(
            &derive_subkey(mk, KEY_PURPOSE),
            &self.aad(item_id, index)?,
            sealed,
        )
    }

    pub fn verify(&self, plain: &[u8]) -> Result<(), Error> {
        self.validate()?;
        if plain.len() as u64 != self.size || digest(plain) != self.sha256 {
            return Err(Error::DecryptFailed);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn descriptor(bytes: &[u8]) -> Attachment {
        Attachment {
            v: 1,
            blob_id: "blob-a".into(),
            file_name: "private-file.txt".into(),
            mime: "text/plain".into(),
            size: bytes.len() as u64,
            sha256: digest(bytes),
            chunk_bytes: CHUNK_BYTES as u32,
            chunks: chunk_count(bytes.len() as u64),
        }
    }
    #[test]
    fn chunks_bind_item_blob_index_count_and_version_and_detect_damage() {
        let key = Key::random().unwrap();
        let bytes = vec![17u8; CHUNK_BYTES * 2 + 9];
        let desc = descriptor(&bytes);
        let sealed = desc
            .seal_chunk(&key, "item-a", 0, &bytes[..CHUNK_BYTES])
            .unwrap();
        assert_eq!(
            desc.open_chunk(&key, "item-a", 0, &sealed).unwrap(),
            bytes[..CHUNK_BYTES]
        );
        assert!(desc.open_chunk(&key, "item-b", 0, &sealed).is_err());
        assert!(desc.open_chunk(&key, "item-a", 1, &sealed).is_err());
        let mut changed = desc.clone();
        changed.blob_id = "blob-b".into();
        assert!(changed.open_chunk(&key, "item-a", 0, &sealed).is_err());
        changed = desc.clone();
        changed.size = CHUNK_BYTES as u64 * 2;
        changed.chunks = 2;
        assert!(changed.open_chunk(&key, "item-a", 0, &sealed).is_err());
        changed = desc.clone();
        changed.v = 2;
        assert!(changed.open_chunk(&key, "item-a", 0, &sealed).is_err());
        let mut tampered = sealed.clone();
        tampered[30] ^= 1;
        assert!(desc.open_chunk(&key, "item-a", 0, &tampered).is_err());
        assert!(desc
            .open_chunk(&Key::random().unwrap(), "item-a", 0, &sealed)
            .is_err());
        assert!(desc
            .open_chunk(&key, "item-a", 0, &sealed[..sealed.len() - 1])
            .is_err());
        assert!(desc.verify(&bytes[..bytes.len() - 1]).is_err());
        assert_eq!(
            desc.aad("item-a", 0).unwrap(),
            b"onepaste/attachment-chunk/v1:item-a:blob-a:0:3"
        );
        assert_eq!(
            Attachment::from_payload(&desc.payload().unwrap()).unwrap(),
            desc
        );
    }
    #[test]
    fn empty_files_limits_and_metadata_are_validated() {
        let key = Key::random().unwrap();
        let d = descriptor(&[]);
        let sealed = d.seal_chunk(&key, "empty", 0, &[]).unwrap();
        assert_eq!(sealed.len(), 40);
        assert!(d.open_chunk(&key, "empty", 0, &sealed).unwrap().is_empty());
        d.verify(&[]).unwrap();
        for name in ["../secret", "path\\secret", "..", "bad\0name"] {
            let mut d = d.clone();
            d.file_name = name.into();
            assert!(d.validate().is_err());
        }
        let mut large = d.clone();
        large.size = MAX_FILE_BYTES + 1;
        large.chunks = chunk_count(large.size);
        assert_eq!(large.validate(), Err(Error::TooLarge));
        large.size = MAX_FILE_BYTES;
        large.chunks = MAX_CHUNKS;
        large.validate().unwrap();
        assert!(d.open_chunk(&key, "empty", 1, &sealed).is_err());
    }
}
