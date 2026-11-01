//! A recoverable key-parameter update. The journal contains wrapped records, never either passphrase or the vault proof.
use crate::{ClientError, Session};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD as B64, Engine};
use onepaste_core::keys::{vault_proof, KeyParams};
use onepaste_sync::{HttpResponse, Method};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

const PENDING: &str = "pending_passphrase_change";

#[derive(Serialize, Deserialize)]
struct Pending { record: KeyParams, base_version: i64, idempotency_key: String }

fn parse_view(value: Value) -> Result<(KeyParams, i64), ClientError> {
    let version = value["version"].as_i64().filter(|v| *v > 0).ok_or_else(|| ClientError::Other("invalid key parameter version".into()))?;
    let record = serde_json::from_value(value).map_err(|_| ClientError::Other("invalid key parameters".into()))?;
    Ok((record, version))
}

fn answer(response: &HttpResponse) -> Value { serde_json::from_slice(&response.body).unwrap_or(Value::Null) }

fn refusal(response: &HttpResponse) -> ClientError {
    if response.status == 401 { return ClientError::Paused(onepaste_sync::PauseReason::Revoked); }
    let value = answer(response);
    let code = match value["code"].as_str() { Some("rate_limited") => "rate_limited", Some("unsupported_version") => "unsupported_version", _ => "passphrase_change_failed" };
    ClientError::Server { status: response.status, code: code.into() }
}

impl Session {
    pub fn passphrase_change_pending(&mut self) -> Result<bool, ClientError> {
        Ok(self.engine.store_mut().meta_get(PENDING)?.is_some_and(|s| !s.is_empty()))
    }

    fn pending_passphrase(&mut self) -> Result<Option<Pending>, ClientError> {
        self.engine.store_mut().meta_get(PENDING)?.filter(|s| !s.is_empty()).map(|s| serde_json::from_str(&s).map_err(|_| ClientError::Storage("pending key parameters".into()))).transpose()
    }

    fn stage_passphrase(&mut self, record: KeyParams, base_version: i64) -> Result<Pending, ClientError> {
        let pending = Pending { record, base_version, idempotency_key: onepaste_core::id::new_id()? };
        let encoded = serde_json::to_string(&pending).map_err(|_| ClientError::Storage("pending key parameters".into()))?;
        self.engine.store_mut().save(&[(PENDING, &encoded)], &[])?;
        Ok(pending)
    }

    fn finish_passphrase(&mut self, record: &KeyParams) -> Result<(), ClientError> {
        record.verify_mk(self.master_key())?;
        let encoded = serde_json::to_string(record).map_err(|_| ClientError::Storage("key parameters".into()))?;
        // One SQLite transaction publishes the confirmed record and clears the recovery journal.
        self.engine.store_mut().save(&[("keyparams", &encoded), (PENDING, "")], &[]).map_err(|_| ClientError::PassphrasePending)
    }

    async fn remote_keyparams(&mut self) -> Result<(KeyParams, i64), ClientError> {
        let response = self.api().keyparams(Method::Get, None, None).await?;
        if response.status != 200 { return Err(refusal(&response)); }
        let (record, version) = parse_view(answer(&response))?;
        record.verify_mk(self.master_key())?;
        Ok((record, version))
    }

    async fn send_passphrase(&mut self, pending: &Pending) -> Result<HttpResponse, ClientError> {
        let body = json!({"record": pending.record, "base_version": pending.base_version, "vault_proof": B64.encode(vault_proof(self.master_key()))});
        self.api().keyparams(Method::Put, Some(body), Some(&pending.idempotency_key)).await
    }

    /// Resolves an uncertain response without guessing that the server rolled a committed write back.
    /// The user explicitly chooses this retry; an offline startup never silently submits a new change.
    pub async fn resume_passphrase_change(&mut self) -> Result<(), ClientError> {
        let Some(pending) = self.pending_passphrase()? else { return Ok(()); };
        let (current, version) = self.remote_keyparams().await.map_err(|_| ClientError::PassphrasePending)?;
        if version > pending.base_version {
            self.finish_passphrase(&current)?;
            return if current == pending.record { Ok(()) } else { Err(ClientError::PassphraseStale) };
        }
        if version < pending.base_version { return Err(ClientError::PassphrasePending); }
        let response = self.send_passphrase(&pending).await.map_err(|_| ClientError::PassphrasePending)?;
        if response.status == 200 {
            // A replay can return an earlier cached success. Read the current record before publishing it locally.
            let (current, version) = self.remote_keyparams().await.map_err(|_| ClientError::PassphrasePending)?;
            if version <= pending.base_version { return Err(ClientError::PassphrasePending); }
            self.finish_passphrase(&current)?;
            return if current == pending.record { Ok(()) } else { Err(ClientError::PassphraseStale) };
        }
        if response.status == 409 && answer(&response)["code"] == "stale" {
            let (current, _) = parse_view(answer(&response)["current_keyparams"].clone())?;
            self.finish_passphrase(&current)?;
            return Err(ClientError::PassphraseStale);
        }
        if response.status >= 500 { return Err(ClientError::PassphrasePending); }
        self.engine.store_mut().save(&[(PENDING, "")], &[])?;
        Err(refusal(&response))
    }

    /// Re-wraps the same master key; no item, history index, device credential or outbox row is rewritten.
    pub async fn change_passphrase(&mut self, old: &str, new: &str) -> Result<(), ClientError> {
        if self.passphrase_change_pending()? { return Err(ClientError::PassphrasePending); }
        if new.is_empty() { return Err(ClientError::PassphraseRequired); }
        let (mut current, mut version) = self.remote_keyparams().await?;
        for attempt in 0..2 {
            if current.unlock(old)? != *self.master_key() { return Err(ClientError::WrongKey); }
            let candidate = current.change_passphrase(old, new)?;
            let pending = self.stage_passphrase(candidate, version)?;
            let response = match self.send_passphrase(&pending).await {
                Ok(response) => response,
                Err(_) => {
                    // A complete read can confirm a lost success; otherwise the durable journal keeps the outcome visible.
                    if let Ok((record, next_version)) = self.remote_keyparams().await {
                        if next_version > pending.base_version {
                            self.finish_passphrase(&record)?;
                            return if record == pending.record { Ok(()) } else { Err(ClientError::PassphraseStale) };
                        }
                    }
                    return Err(ClientError::PassphrasePending);
                }
            };
            if response.status == 200 {
                if answer(&response)["version"].as_i64() != Some(version + 1) { return Err(ClientError::PassphrasePending); }
                return self.finish_passphrase(&pending.record);
            }
            if response.status == 409 && answer(&response)["code"] == "stale" {
                let (record, next_version) = parse_view(answer(&response)["current_keyparams"].clone())?;
                self.finish_passphrase(&record)?;
                if attempt == 1 { return Err(ClientError::PassphraseStale); }
                current = record; version = next_version;
                continue;
            }
            if response.status >= 500 { return Err(ClientError::PassphrasePending); }
            self.engine.store_mut().save(&[(PENDING, "")], &[])?;
            return Err(refusal(&response));
        }
        Err(ClientError::PassphraseStale)
    }
}
