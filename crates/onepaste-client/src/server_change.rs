//! Reconnect a vault using its existing master key. All network work precedes the local commit.
use crate::{ApiClient, ClientError, ConnectionCode, Info, Session, Transport};
use crate::join::register_remote;
use crate::sqlite::ConnectionReplacement;
use onepaste_core::{crypto::{self, aad}, keys::KeyParams, recovery};
use onepaste_core::envelope::Envelope;
use onepaste_sync::{HttpRequest, Intent, ItemRow, Method, OpState, OutboxRow};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use std::collections::{HashMap, HashSet};

// The sync engine's sealed outbox format; Intent's public serializer owns the operation encoding.
#[derive(Serialize, Deserialize)]
struct StoredIntent {
    intent: Intent,
    envelope: Option<String>,
}

impl Session {
    /// Replaces this connection without replacing the vault key or writing to the old server.
    /// A failed request or local commit leaves every local row and this session unchanged.
    pub async fn change_server(&mut self, raw_code: &str, passphrase: Option<&str>, kit: Option<&str>) -> Result<(), ClientError> {
        if passphrase.is_some() && kit.is_some() { return Err(ClientError::Other("choose one vault credential".into())); }
        let code = ConnectionCode::parse(raw_code)?;
        let before = self.engine.store_mut().connection_snapshot()?;
        let next_store = self.engine.store_mut().reopen()?;
        let mk = self.master_key().clone();
        let original: KeyParams = serde_json::from_str(before.value("keyparams").ok_or(ClientError::NotJoined)?).map_err(|_| ClientError::Storage("key parameters".into()))?;
        original.verify_mk(&mk)?;
        let transport = Transport::new(&code.u, &code.fp);
        let request = HttpRequest { method: Method::Get, path_and_query: "/v1/vault/keyparams".into(), headers: vec![("OnePaste-Proto".into(), "1".into()), ("OnePaste-Reg-Token".into(), code.t.clone())], body: vec![] };
        let response = transport.send(&request).await?;
        let json: Value = serde_json::from_slice(&response.body).map_err(|_| ClientError::Other("malformed server answer".into()))?;
        let empty = response.status == 404 && json["code"] == "vault_empty";
        let record = if empty {
            original
        } else if response.status == 200 {
            let record: KeyParams = serde_json::from_value(json).map_err(|_| ClientError::Other("malformed key parameters".into()))?;
            let unlocked = match kit {
                Some(kit) => recovery::decode(kit)?,
                None => record.unlock(passphrase.ok_or(ClientError::PassphraseRequired)?)?,
            };
            record.verify_mk(&unlocked)?;
            if unlocked != mk { return Err(ClientError::WrongKey); }
            record
        } else {
            return Err(ClientError::Server { status: response.status, code: stable_code(&json) });
        };
        let name = match before.value("sealed_device_name") {
            Some(value) => {
                let sealed = B64.decode(value).map_err(|_| ClientError::Storage("device name".into()))?;
                String::from_utf8(crypto::open(&mk, &aad::device(&self.info.device_id), &sealed)?).map_err(|_| ClientError::Storage("device name".into()))?
            }
            None => "OnePaste".into(),
        };
        let registered = register_remote(&code, &transport, &record, &mk, empty, &name).await.map_err(safe_server_error)?;
        let next = Session::for_connection(next_store, &mk, Info { url: code.u.clone(), fingerprint: code.fp.clone(), device_id: registered.device_id.clone() }, registered.token);
        let wanted: HashSet<_> = before.items.iter().map(|r| r.id.as_str()).collect();
        let remote = if empty { HashMap::new() } else { remote_rows(next.api(), &wanted).await? };
        let mut items = before.items.clone();
        let mut outbox = Vec::new();
        let mut op_id = 1u64;
        for row in &mut items {
            row.seq = 0;
            row.mark = 0;
            let there = remote.get(&row.id);
            if let Some(there) = there {
                if there.deleted == row.deleted && there.envelope_json == row.envelope_json {
                    row.seq = there.seq;
                    continue;
                }
            } else if row.deleted { continue; }
            let old: Vec<_> = before.outbox.iter().filter(|o| o.item_id == row.id).collect();
            let blobs = if there.is_some() && !old.is_empty() {
                old.iter().map(|o| {
                    let data = crypto::open(&mk, &aad::outbox(o.op_id), &o.sealed_intent)?;
                    serde_json::from_slice::<StoredIntent>(&data).map_err(|_| ClientError::Storage("queued operation".into()))
                }).collect::<Result<Vec<_>, ClientError>>()?
            } else {
                vec![StoredIntent { intent: if row.deleted { Intent::Delete } else { Intent::Create }, envelope: row.envelope_json.clone() }]
            };
            for blob in blobs {
                let data = serde_json::to_vec(&blob).map_err(|_| ClientError::Storage("queued operation".into()))?;
                outbox.push(OutboxRow { op_id, item_id: row.id.clone(), sealed_intent: crypto::seal(&mk, &aad::outbox(op_id), &data)?, idem_key: onepaste_core::id::new_id()?, attempts: 0, state: OpState::Pending, last_error: None });
                op_id = op_id.checked_add(1).ok_or_else(|| ClientError::Storage("operation counter limit".into()))?;
            }
        }
        // A new server's sequences may be lower than the history cache's existing high-water mark.
        let old_top = before.items.iter().map(|r| r.seq).max().unwrap_or(0).max(before.sync.max_seen_seq).max(before.sync.cursor);
        let offset = before.value("history_seq_offset").unwrap_or("0").parse::<i64>().map_err(|_| ClientError::Storage("history offset".into()))?
            .checked_add(old_top).and_then(|v| v.checked_add(1)).ok_or_else(|| ClientError::Storage("history counter limit".into()))?;
        let replacement = ConnectionReplacement {
            values: vec![("u".into(), code.u), ("fp".into(), code.fp), ("device_id".into(), registered.device_id), ("keyparams".into(), serde_json::to_string(&record).map_err(|_| ClientError::Storage("key parameters".into()))?), ("sealed_device_name".into(), registered.sealed_name), ("history_seq_offset".into(), offset.to_string()), ("server_initial_sync_done".into(), "false".into())],
            token: registered.sealed_token, items, outbox, next_op_id: op_id,
        };
        self.engine.store_mut().replace_connection(&before, &replacement)?;
        *self = next;
        Ok(())
    }
}

fn stable_code(json: &Value) -> String {
    safe_code(json["code"].as_str()).into()
}

fn safe_code(code: Option<&str>) -> &'static str {
    match code {
        Some("registration_invalid") => "registration_invalid",
        Some("rate_limited") => "rate_limited",
        Some("unsupported_version") => "unsupported_version",
        _ => "server_change_failed",
    }
}

fn safe_server_error(error: ClientError) -> ClientError {
    match error {
        ClientError::Server { status, code } => ClientError::Server { status, code: safe_code(Some(&code)).into() },
        other => other,
    }
}

async fn remote_rows(api: ApiClient, wanted: &HashSet<&str>) -> Result<HashMap<String, ItemRow>, ClientError> {
    let mut rows = HashMap::new();
    let mut after = 0;
    loop {
        let page = api.call(Method::Get, &format!("/v1/items?after={after}&limit=200"), None).await.map_err(safe_server_error)?;
        let entries = page["items"].as_array().ok_or_else(|| ClientError::Other("malformed item page".into()))?;
        for entry in entries {
            let id = entry["id"].as_str().ok_or_else(|| ClientError::Other("malformed item".into()))?;
            if !wanted.contains(id) { continue; }
            let envelope_json = if entry["envelope"].is_null() { None } else {
                let envelope: Envelope = serde_json::from_value(entry["envelope"].clone()).map_err(|_| ClientError::Other("malformed envelope".into()))?;
                Some(envelope.to_json()?)
            };
            rows.insert(id.into(), ItemRow { id: id.into(), seq: entry["seq"].as_i64().ok_or_else(|| ClientError::Other("malformed sequence".into()))?, deleted: entry["deleted"].as_bool().ok_or_else(|| ClientError::Other("malformed item".into()))?, envelope_json, mark: 0 });
        }
        if page["has_more"].as_bool() == Some(false) { return Ok(rows); }
        let next = page["next_after"].as_i64().filter(|n| *n > after).ok_or_else(|| ClientError::Other("malformed page cursor".into()))?;
        after = next;
    }
}
