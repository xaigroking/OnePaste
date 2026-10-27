//! Parsing and checking of the item records the server returns (page entries and `current_item`).

use onepaste_core::envelope::Envelope;
use onepaste_core::id::validate_id;
use serde_json::Value;

/// Largest sealed item the server accepts.
pub const MAX_SEALED_BYTES: usize = 8 * 1024 * 1024;

pub struct Record {
    pub id: String,
    pub seq: i64,
    pub deleted: bool,
    pub envelope: Option<Envelope>,
}

/// Returns the record, or a stable code saying why it is unusable.
pub fn parse_record(v: &Value) -> Result<Record, (String, &'static str)> {
    let id = v.get("id").and_then(Value::as_str).unwrap_or("").to_string();
    let bad = |code: &'static str| (id.clone(), code);
    validate_id(&id).map_err(|_| bad("malformed_record"))?;
    let seq = v.get("seq").and_then(Value::as_i64).filter(|n| *n >= 1).ok_or_else(|| bad("malformed_record"))?;
    let deleted = v.get("deleted").and_then(Value::as_bool).ok_or_else(|| bad("malformed_record"))?;
    if deleted {
        return Ok(Record { id, seq, deleted, envelope: None });
    }
    let raw = v.get("envelope").ok_or_else(|| bad("malformed_envelope"))?;
    let env: Envelope = serde_json::from_value(raw.clone()).map_err(|_| bad("malformed_envelope"))?;
    env.validate(MAX_SEALED_BYTES).map_err(|e| bad(e.code()))?;
    if env.id != id {
        return Err(bad("id_mismatch"));
    }
    Ok(Record { id, seq, deleted, envelope: Some(env) })
}
