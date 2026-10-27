//! The connection code: `onepaste1:` + base64url of `{"u": server address, "fp": certificate fingerprint, "t": registration credential}`.

use crate::error::ClientError;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD as B64URL, Engine};
use serde::{Deserialize, Serialize};

const PREFIX: &str = "onepaste1:";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectionCode {
    pub u: String,
    pub fp: String,
    pub t: String,
}

impl ConnectionCode {
    pub fn parse(code: &str) -> Result<ConnectionCode, ClientError> {
        let raw = code.trim().strip_prefix(PREFIX).ok_or(ClientError::BadConnectionCode)?;
        let bytes = B64URL.decode(raw).map_err(|_| ClientError::BadConnectionCode)?;
        let c: ConnectionCode = serde_json::from_slice(&bytes).map_err(|_| ClientError::BadConnectionCode)?;
        if !c.u.starts_with("https://") || c.fp.is_empty() || c.t.is_empty() {
            return Err(ClientError::BadConnectionCode);
        }
        Ok(c)
    }

    pub fn encode(&self) -> String {
        format!("{PREFIX}{}", B64URL.encode(serde_json::to_vec(self).unwrap_or_default()))
    }
}
