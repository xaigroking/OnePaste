//! The key parameter record, the vault proof and device registration.

use super::{create_reg_token_tx, reg_token_valid_tx, Reply, Store};
use crate::error::ApiError;
use crate::util::{ct_eq, sha256};
use axum::http::StatusCode;
use rusqlite::{params, OptionalExtension, Transaction};
use serde_json::{json, Value};

pub struct RegisterIn {
    pub reg_hash: [u8; 32],
    pub device_id: String,
    pub sealed_name: String,
    pub token_hash: [u8; 32],
    /// Top-level proof, for a vault that already exists.
    pub proof: Option<[u8; 32]>,
    /// Canonical record JSON and proof, for an empty vault.
    pub init: Option<(String, [u8; 32])>,
}

/// The record fields plus `version`, the shape of `KeyParamsView`.
pub fn view_json(record: &str, version: i64) -> Value {
    let mut v: Value = serde_json::from_str(record).unwrap_or(Value::Null);
    if let Value::Object(m) = &mut v {
        m.insert("version".to_string(), json!(version));
    }
    v
}

fn stored_record(tx: &Transaction) -> Result<Option<(String, i64)>, ApiError> {
    let row: Option<(Option<String>, i64)> = tx
        .query_row("SELECT keyparams, keyparams_version FROM vault WHERE id = 1", [], |r| Ok((r.get(0)?, r.get(1)?)))
        .optional()?;
    Ok(row.and_then(|(k, v)| k.map(|k| (k, v))))
}

impl Store {
    /// The stored record as a `KeyParamsView`, or `None` while the vault is empty.
    pub fn keyparams_view(&self) -> Result<Option<Value>, ApiError> {
        self.read(|tx| Ok(stored_record(tx)?.map(|(rec, ver)| view_json(&rec, ver))))
    }

    pub fn register_tx(tx: &Transaction, now: i64, input: &RegisterIn) -> Result<(), ApiError> {
        if !reg_token_valid_tx(tx, &input.reg_hash, now)? {
            return Err(ApiError::registration_invalid());
        }
        let existing = stored_record(tx)?;
        match (&existing, &input.init, &input.proof) {
            (None, Some((record, proof)), None) => {
                tx.execute(
                    "UPDATE vault SET keyparams = ?1, keyparams_version = 1, proof_hash = ?2 WHERE id = 1",
                    params![record, &sha256(proof)[..]],
                )?;
            }
            (Some(_), None, Some(proof)) => {
                let want: Option<Vec<u8>> =
                    tx.query_row("SELECT proof_hash FROM vault WHERE id = 1", [], |r| r.get(0))?;
                let ok = want.map(|w| ct_eq(&w, &sha256(proof))).unwrap_or(false);
                if !ok {
                    return Err(ApiError::registration_invalid());
                }
            }
            _ => return Err(ApiError::invalid()),
        }
        let inserted = tx.execute(
            "INSERT OR IGNORE INTO devices (id, token_hash, sealed_name, created_at, revoked) VALUES (?1, ?2, ?3, ?4, 0)",
            params![input.device_id, &input.token_hash[..], input.sealed_name, now],
        )?;
        if inserted == 0 {
            return Err(ApiError::invalid());
        }
        tx.execute("UPDATE reg_tokens SET used = 1 WHERE hash = ?1", [&input.reg_hash[..]])?;
        Ok(())
    }

    /// Creates a registration credential on behalf of an authenticated device, inside an idempotent write.
    pub fn invite_tx(tx: &Transaction, hash: &[u8; 32], now: i64, ttl: i64) -> Result<i64, ApiError> {
        create_reg_token_tx(tx, hash, now, ttl, false)
    }

    /// Replaces the record (passphrase change or recovery kit reset). The vault proof must match.
    pub fn put_keyparams_tx(
        tx: &Transaction,
        record: &str,
        base_version: i64,
        proof: &[u8; 32],
    ) -> Result<Reply, ApiError> {
        let (current, version) = stored_record(tx)?.ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "vault_empty"))?;
        let want: Option<Vec<u8>> = tx.query_row("SELECT proof_hash FROM vault WHERE id = 1", [], |r| r.get(0))?;
        if !want.map(|w| ct_eq(&w, &sha256(proof))).unwrap_or(false) {
            return Err(ApiError::unauthorized());
        }
        if base_version != version {
            return Err(ApiError::new(StatusCode::CONFLICT, "stale")
                .with("current_keyparams", view_json(&current, version)));
        }
        let next = version + 1;
        tx.execute("UPDATE vault SET keyparams = ?1, keyparams_version = ?2 WHERE id = 1", params![record, next])?;
        Ok(Reply {
            status: 200,
            body: json!({ "version": next }).to_string().into_bytes(),
            stored: None,
            notify_seq: None,
            replayed: false,
        })
    }
}
