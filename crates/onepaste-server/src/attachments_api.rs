use super::*;
use onepaste_core::attachment::{CHUNK_BYTES, MAX_CHUNKS, MAX_FILE_BYTES, MAX_SEALED_CHUNK};

pub(super) async fn manifest(
    State(s): St,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    auth(&s, &headers)?;
    onepaste_core::id::validate_id(&id).map_err(|_| ApiError::invalid())?;
    Ok(axum::Json(s.store.blob_manifest(&id)?).into_response())
}

pub(super) async fn capabilities(State(s): St, headers: HeaderMap) -> Result<Response, ApiError> {
    auth(&s, &headers)?;
    Ok(axum::Json(json!({"features":["attachments-v1"],"attachments":{"v":1,"max_file_bytes":MAX_FILE_BYTES,"chunk_bytes":CHUNK_BYTES,"max_chunks":MAX_CHUNKS}})).into_response())
}

pub(super) async fn put_chunk(
    State(s): St,
    headers: HeaderMap,
    Path((id, index)): Path<(String, String)>,
    body: Bytes,
) -> Result<Response, ApiError> {
    let (device, _) = auth(&s, &headers)?;
    let key = idem_key(&headers)?;
    onepaste_core::id::validate_id(&id).map_err(|_| ApiError::invalid())?;
    let index = index.parse::<u32>().map_err(|_| ApiError::invalid())?;
    if headers
        .get("onepaste-blob-version")
        .and_then(|v| v.to_str().ok())
        != Some("1")
    {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "unsupported_attachment_version",
        ));
    }
    let chunks = headers
        .get("onepaste-blob-chunks")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u32>().ok())
        .ok_or_else(ApiError::invalid)?;
    if body.len() > MAX_SEALED_CHUNK {
        return Err(ApiError::too_large());
    }
    // Bind all immutable transport metadata into the idempotency request hash.
    let path = format!("/v1/blobs/{id}/{index}?v=1&chunks={chunks}");
    let now = s.clock.now();
    let reply = s.store.idempotent(
        &format!("d:{device}:blob"),
        &key,
        &request_hash("PUT", &path, &body),
        now,
        s.cfg.idempotency_ttl_secs,
        |tx| Store::put_blob_chunk_tx(tx, &id, index, chunks, &body, now),
    )?;
    Ok(finish_write(&s, reply))
}

pub(super) async fn get_chunk(
    State(s): St,
    headers: HeaderMap,
    Path((id, index)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    auth(&s, &headers)?;
    onepaste_core::id::validate_id(&id).map_err(|_| ApiError::invalid())?;
    let index = index.parse::<u32>().map_err(|_| ApiError::invalid())?;
    if index >= MAX_CHUNKS {
        return Err(ApiError::invalid());
    }
    let chunk = s.store.blob_chunk(&id, index)?;
    let mut response = Response::new(Body::from(chunk.data));
    let h = response.headers_mut();
    h.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/octet-stream"),
    );
    h.insert(
        HeaderName::from_static("onepaste-blob-version"),
        HeaderValue::from_static("1"),
    );
    h.insert(
        HeaderName::from_static("onepaste-blob-chunks"),
        HeaderValue::from_str(&chunk.chunks.to_string()).map_err(|_| ApiError::internal())?,
    );
    Ok(response)
}
