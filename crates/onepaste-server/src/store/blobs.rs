//! Immutable ciphertext chunks and item references, always changed within storage transactions.
use super::{Reply, Store};
use crate::error::ApiError;
use axum::http::StatusCode;
use onepaste_core::attachment::{MAX_CHUNKS, MAX_SEALED_CHUNK};
use rusqlite::{params, OptionalExtension, Transaction};
use serde_json::json;

pub const UNREFERENCED_GRACE_SECS: i64 = 24 * 3600;
pub struct BlobChunk {
    pub chunks: u32,
    pub data: Vec<u8>,
}

impl Store {
    pub fn put_blob_chunk_tx(
        tx: &Transaction,
        id: &str,
        index: u32,
        chunks: u32,
        data: &[u8],
        now: i64,
    ) -> Result<Reply, ApiError> {
        onepaste_core::id::validate_id(id).map_err(|_| ApiError::invalid())?;
        if data.len() > MAX_SEALED_CHUNK {
            return Err(ApiError::too_large());
        }
        if chunks == 0
            || chunks > MAX_CHUNKS
            || index >= chunks
            || data.len() < 40
            || index + 1 < chunks && data.len() != MAX_SEALED_CHUNK
        {
            return Err(ApiError::invalid());
        }
        let existing: Option<u32> = tx
            .query_row("SELECT chunks FROM blobs WHERE id=?1", [id], |r| r.get(0))
            .optional()?;
        if existing.is_some_and(|count| count != chunks) {
            return Err(ApiError::new(StatusCode::CONFLICT, "attachment_conflict"));
        }
        let old: Option<Vec<u8>> = tx
            .query_row(
                "SELECT sealed FROM blob_chunks WHERE blob_id=?1 AND chunk_index=?2",
                params![id, index],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(old) = &old {
            if old.as_slice() != data {
                return Err(ApiError::new(StatusCode::CONFLICT, "attachment_conflict"));
            }
        }
        tx.execute("INSERT INTO blobs(id,chunks,updated_at) VALUES(?1,?2,?3) ON CONFLICT(id) DO UPDATE SET updated_at=excluded.updated_at",params![id,chunks,now])?;
        if old.is_none() {
            tx.execute(
                "INSERT INTO blob_chunks(blob_id,chunk_index,sealed) VALUES(?1,?2,?3)",
                params![id, index, data],
            )?;
        }
        Ok(Reply {
            status: 200,
            body: json!({"stored":true,"index":index})
                .to_string()
                .into_bytes(),
            stored: None,
            notify_seq: None,
            replayed: false,
        })
    }

    pub fn blob_manifest(&self, id: &str) -> Result<serde_json::Value, ApiError> {
        self.read(|tx| {
            let chunks: u32 = tx
                .query_row("SELECT chunks FROM blobs WHERE id=?1", [id], |r| r.get(0))
                .optional()?
                .ok_or_else(ApiError::not_found)?;
            let indices = tx
                .prepare(
                    "SELECT chunk_index FROM blob_chunks WHERE blob_id=?1 ORDER BY chunk_index",
                )?
                .query_map([id], |r| r.get::<_, u32>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(json!({"v":1,"chunks":chunks,"indices":indices}))
        })
    }

    pub fn blob_chunk(&self, id: &str, index: u32) -> Result<BlobChunk, ApiError> {
        self.read(|tx| tx.query_row("SELECT b.chunks,c.sealed FROM blobs b JOIN blob_chunks c ON c.blob_id=b.id WHERE b.id=?1 AND c.chunk_index=?2",params![id,index],|r|Ok(BlobChunk{chunks:r.get(0)?,data:r.get(1)?})).optional()?.ok_or_else(ApiError::not_found))
    }

    pub(super) fn set_item_blob_tx(
        tx: &Transaction,
        item_id: &str,
        blob_id: Option<&str>,
        now: i64,
    ) -> Result<(), ApiError> {
        if let Some(blob_id) = blob_id {
            let declared: Option<u32> = tx
                .query_row("SELECT chunks FROM blobs WHERE id=?1", [blob_id], |r| {
                    r.get(0)
                })
                .optional()?;
            let stored: u32 = tx.query_row(
                "SELECT count(*) FROM blob_chunks WHERE blob_id=?1",
                [blob_id],
                |r| r.get(0),
            )?;
            if declared.is_none_or(|count| count != stored) {
                return Err(ApiError::new(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "attachment_incomplete",
                ));
            }
        }
        let old: Option<String> = tx
            .query_row(
                "SELECT blob_id FROM item_blobs WHERE item_id=?1",
                [item_id],
                |r| r.get(0),
            )
            .optional()?;
        if old.as_deref() != blob_id {
            if let Some(old) = old {
                tx.execute(
                    "UPDATE blobs SET updated_at=?1 WHERE id=?2",
                    params![now, old],
                )?;
            }
        }
        tx.execute("DELETE FROM item_blobs WHERE item_id=?1", [item_id])?;
        if let Some(blob_id) = blob_id {
            tx.execute(
                "INSERT INTO item_blobs(item_id,blob_id) VALUES(?1,?2)",
                params![item_id, blob_id],
            )?;
        }
        Ok(())
    }

    pub(super) fn cleanup_blobs_tx(tx: &Transaction, now: i64) -> Result<(), ApiError> {
        let cutoff = now.saturating_sub(UNREFERENCED_GRACE_SECS);
        tx.execute("DELETE FROM blob_chunks WHERE blob_id IN (SELECT id FROM blobs WHERE updated_at<?1 AND NOT EXISTS(SELECT 1 FROM item_blobs WHERE item_blobs.blob_id=blobs.id))",[cutoff])?;
        tx.execute("DELETE FROM blobs WHERE updated_at<?1 AND NOT EXISTS(SELECT 1 FROM item_blobs WHERE item_blobs.blob_id=blobs.id)",[cutoff])?;
        Ok(())
    }
}
