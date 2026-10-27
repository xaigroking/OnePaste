//! Operation intents and the sealed outbox record.

use crate::error::Error;
use onepaste_core::crypto::{self, aad, Key};
use onepaste_core::envelope::{open_item, seal_item, Envelope, ItemPayload};
use serde::{Deserialize, Serialize};

/// A change to one item. The body (`Kind`, `Content`) cannot be changed after creation: the engine refuses those.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "field", content = "value", rename_all = "snake_case")]
pub enum Edit {
    Favorite(bool),
    Pinned(bool),
    Note(Option<String>),
    Tags(Vec<String>),
    Kind(String),
    Content(Option<String>),
}

impl Edit {
    /// Applies exactly this one field and leaves every other field as it is.
    pub fn apply(&self, p: &mut ItemPayload) -> Result<(), Error> {
        match self {
            Edit::Favorite(v) => p.favorite = *v,
            Edit::Pinned(v) => p.pinned = *v,
            Edit::Note(v) => p.note = v.clone(),
            Edit::Tags(v) => p.tags = v.clone(),
            Edit::Kind(_) | Edit::Content(_) => return Err(Error::BodyImmutable),
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Intent {
    Create,
    Edit { edit: Edit },
    Delete,
}

/// What is sealed into an outbox row: the intent, and the exact envelope to send so a retry carries identical bytes.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OpBlob {
    pub intent: Intent,
    /// The envelope JSON to upload; `None` for a delete.
    pub envelope: Option<String>,
}

pub fn seal_blob(mk: &Key, op_id: u64, blob: &OpBlob) -> Result<Vec<u8>, Error> {
    let plain = serde_json::to_vec(blob).map_err(|_| Error::MalformedResponse)?;
    Ok(crypto::seal(mk, &aad::outbox(op_id), &plain)?)
}

pub fn open_blob(mk: &Key, op_id: u64, sealed: &[u8]) -> Result<OpBlob, Error> {
    let plain = crypto::open(mk, &aad::outbox(op_id), sealed)?;
    serde_json::from_slice(&plain).map_err(|_| Error::MalformedResponse)
}

pub fn secs(now_ms: u64) -> i64 {
    (now_ms / 1000) as i64
}

pub fn parse_envelope(json: &str) -> Result<Envelope, Error> {
    serde_json::from_str(json).map_err(|_| Error::Core(onepaste_core::Error::MalformedEnvelope))
}

/// Seals a new version of an item: same id, creation time and blob reference, fresh nonce.
pub fn reseal(mk: &Key, old: &Envelope, now_secs: i64, payload: &ItemPayload) -> Result<String, Error> {
    let env = seal_item(mk, &old.id, old.created_at, now_secs.max(old.created_at), old.blob_id.clone(), payload)?;
    Ok(env.to_json()?)
}

pub fn open_envelope(mk: &Key, env: &Envelope) -> Result<ItemPayload, Error> {
    Ok(open_item(mk, env)?)
}
