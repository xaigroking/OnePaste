//! Plain data types shared by the engine, the storage trait and the drivers.

use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Method {
    Get,
    Put,
    Post,
    Delete,
}

impl Method {
    pub fn as_str(&self) -> &'static str {
        match self {
            Method::Get => "GET",
            Method::Put => "PUT",
            Method::Post => "POST",
            Method::Delete => "DELETE",
        }
    }
}

/// A request the engine wants the driver to send.
#[derive(Clone, PartialEq, Eq)]
pub struct HttpRequest {
    pub method: Method,
    pub path_and_query: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl HttpRequest {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }
}

/// `Debug` never shows header values or the body: they hold the device token and ciphertext.
impl fmt::Debug for HttpRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let names: Vec<&str> = self.headers.iter().map(|(k, _)| k.as_str()).collect();
        f.debug_struct("HttpRequest")
            .field("method", &self.method)
            .field("path_and_query", &self.path_and_query)
            .field("headers", &names)
            .field("body_len", &self.body.len())
            .finish()
    }
}

/// A response the driver hands back to the engine.
#[derive(Clone, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl HttpResponse {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }
}

impl fmt::Debug for HttpResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HttpResponse").field("status", &self.status).field("body_len", &self.body.len()).finish()
    }
}

/// One item as the local store keeps it. `envelope_json` is the server envelope as is; nothing is stored in the clear.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ItemRow {
    pub id: String,
    pub seq: i64,
    pub deleted: bool,
    pub envelope_json: Option<String>,
    /// The refetch epoch in which the server last confirmed the item.
    pub mark: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpState {
    Pending,
    Conflict,
    Failed,
}

impl OpState {
    pub fn as_str(&self) -> &'static str {
        match self {
            OpState::Pending => "pending",
            OpState::Conflict => "conflict",
            OpState::Failed => "failed",
        }
    }

    pub fn parse(s: &str) -> Option<OpState> {
        match s {
            "pending" => Some(OpState::Pending),
            "conflict" => Some(OpState::Conflict),
            "failed" => Some(OpState::Failed),
            _ => None,
        }
    }
}

/// A queued outbound operation. The operation itself is sealed with the master key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutboxRow {
    pub op_id: u64,
    pub item_id: String,
    pub sealed_intent: Vec<u8>,
    pub idem_key: String,
    pub attempts: u32,
    pub state: OpState,
    pub last_error: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PauseReason {
    ServerRollback,
    Revoked,
}

impl PauseReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            PauseReason::ServerRollback => "server_rollback",
            PauseReason::Revoked => "revoked",
        }
    }

    pub fn parse(s: &str) -> Option<PauseReason> {
        match s {
            "server_rollback" => Some(PauseReason::ServerRollback),
            "revoked" => Some(PauseReason::Revoked),
            _ => None,
        }
    }
}

/// The engine's persistent bookkeeping.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SyncMeta {
    pub cursor: i64,
    pub max_seen_seq: i64,
    pub paused: Option<PauseReason>,
    /// A full re-pull is in progress; it survives restarts.
    pub refetching: bool,
    pub epoch: u64,
    pub next_op_id: u64,
}
