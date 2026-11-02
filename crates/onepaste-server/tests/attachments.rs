mod common;
use common::*;
use reqwest::Method;

#[tokio::test(flavor = "multi_thread")]
async fn authenticated_immutable_chunks_can_be_uploaded_and_replayed() {
    let env = Env::new().await;
    let dev = first_device(&env).await;
    let r = env
        .send(
            Method::PUT,
            "/v1/blobs/opaque-blob/0",
            Some(&dev.token),
            Some("opaque-blob-0"),
            Some(vec![7; 64]),
            &[
                ("OnePaste-Blob-Version", "1"),
                ("OnePaste-Blob-Chunks", "1"),
            ],
        )
        .await;
    assert_eq!(r.status, 200, "{}", r.json);
}

async fn put(
    env: &Env,
    dev: &Dev,
    id: &str,
    index: u32,
    chunks: u32,
    data: Vec<u8>,
    key: &str,
) -> Resp {
    env.send(
        Method::PUT,
        &format!("/v1/blobs/{id}/{index}"),
        Some(&dev.token),
        Some(key),
        Some(data),
        &[
            ("OnePaste-Blob-Version", "1"),
            ("OnePaste-Blob-Chunks", &chunks.to_string()),
        ],
    )
    .await
}

#[tokio::test(flavor = "multi_thread")]
async fn chunk_bytes_and_manifests_survive_restarts_and_replays_cannot_replace_ciphertext() {
    let mut env = Env::new().await;
    let dev = first_device(&env).await;
    let data = vec![9; 91];
    for _ in 0..2 {
        assert_eq!(
            put(&env, &dev, "immutable", 0, 1, data.clone(), "fixed-key")
                .await
                .status,
            200
        );
    }
    assert_eq!(
        put(&env, &dev, "immutable", 0, 1, vec![8; 91], "fixed-key")
            .await
            .status,
        422
    );
    assert_eq!(
        put(&env, &dev, "immutable", 0, 1, vec![8; 91], "other-key")
            .await
            .status,
        409
    );
    env.restart().await;
    let r = env
        .http
        .get(format!("{}/v1/blobs/immutable/0", env.base))
        .header("OnePaste-Proto", "1")
        .bearer_auth(&dev.token)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status().as_u16(), 200);
    assert_eq!(r.headers()["onepaste-blob-chunks"], "1");
    assert_eq!(r.bytes().await.unwrap().as_ref(), data);
    let manifest = env.get("/v1/blobs/immutable", &dev.token).await;
    assert_eq!(manifest.json["indices"], serde_json::json!([0]));
    let capabilities = env.get("/v1/capabilities", &dev.token).await;
    assert_eq!(capabilities.json["features"][0], "attachments-v1");
}

#[tokio::test(flavor = "multi_thread")]
async fn attachment_routes_enforce_authentication_revocation_and_bounded_binary_inputs() {
    use onepaste_core::attachment::MAX_SEALED_CHUNK;
    let env = Env::new().await;
    let dev = first_device(&env).await;
    assert_eq!(env.get("/v1/capabilities", "bad-token").await.status, 401);
    assert_eq!(
        put(&env, &dev, "bounds", 0, 0, vec![1; 40], "zero")
            .await
            .status,
        400
    );
    assert_eq!(
        put(&env, &dev, "bounds", 256, 256, vec![1; 40], "index")
            .await
            .status,
        400
    );
    assert_eq!(
        put(&env, &dev, "bounds", 0, 257, vec![1; 40], "chunks")
            .await
            .status,
        400
    );
    assert_eq!(
        put(&env, &dev, "bounds", 0, 1, vec![1; 39], "short")
            .await
            .status,
        400
    );
    assert_eq!(
        put(
            &env,
            &dev,
            "bounds",
            0,
            1,
            vec![1; MAX_SEALED_CHUNK + 1],
            "large"
        )
        .await
        .status,
        413
    );
    assert_eq!(
        put(
            &env,
            &dev,
            "bounds",
            0,
            2,
            vec![1; 40],
            "incomplete-nonfinal"
        )
        .await
        .status,
        400
    );
    let b = join_device(&env, &invite(&env, &dev).await, &dev).await;
    assert_eq!(
        put(&env, &dev, "bounds", 0, 1, vec![1; 40], "valid")
            .await
            .status,
        200
    );
    let result = env
        .call(
            Method::DELETE,
            &format!("/v1/devices/{}", b.id),
            Some(&dev.token),
            None,
        )
        .await;
    assert_eq!(result.status, 204);
    assert_eq!(env.get("/v1/blobs/bounds/0", &b.token).await.status, 401);
    assert_eq!(
        put(&env, &b, "revoked", 0, 1, vec![1; 40], "denied")
            .await
            .status,
        401
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn item_references_require_all_chunks_and_garbage_collection_keeps_live_files() {
    use onepaste_core::attachment::{chunk_count, digest, Attachment, CHUNK_BYTES};
    use onepaste_core::envelope::seal_item;
    use serde_json::json;
    let env = Env::new().await;
    let dev = first_device(&env).await;
    let plain = vec![23u8; CHUNK_BYTES + 17];
    let desc = Attachment {
        v: 1,
        blob_id: "complete-file".into(),
        file_name: "PRIVATE-NAME-PROBE.txt".into(),
        mime: "text/plain".into(),
        size: plain.len() as u64,
        sha256: digest(&plain),
        chunk_bytes: CHUNK_BYTES as u32,
        chunks: chunk_count(plain.len() as u64),
    };
    let payload = desc.payload().unwrap();
    let sealed = seal_item(
        &dev.mk,
        "file-item",
        1,
        1,
        Some(desc.blob_id.clone()),
        &payload,
    )
    .unwrap();
    let body = json!({"base_seq":0,"envelope":sealed});
    assert_eq!(
        env.call(
            Method::PUT,
            "/v1/items/file-item",
            Some(&dev.token),
            Some(body.clone())
        )
        .await
        .status,
        422
    );
    assert_eq!(
        put(
            &env,
            &dev,
            &desc.blob_id,
            0,
            2,
            desc.seal_chunk(&dev.mk, "file-item", 0, &plain[..CHUNK_BYTES])
                .unwrap(),
            "chunk-0"
        )
        .await
        .status,
        200
    );
    assert_eq!(
        env.call(
            Method::PUT,
            "/v1/items/file-item",
            Some(&dev.token),
            Some(body.clone())
        )
        .await
        .status,
        422
    );
    assert_eq!(
        put(
            &env,
            &dev,
            &desc.blob_id,
            1,
            2,
            desc.seal_chunk(&dev.mk, "file-item", 1, &plain[CHUNK_BYTES..])
                .unwrap(),
            "chunk-1"
        )
        .await
        .status,
        200
    );
    let r = env
        .call(
            Method::PUT,
            "/v1/items/file-item",
            Some(&dev.token),
            Some(body),
        )
        .await;
    assert_eq!(r.status, 200);
    let seq = r.json["seq"].clone();
    assert_eq!(
        put(&env, &dev, "orphan", 0, 1, vec![4; 40], "orphan-0")
            .await
            .status,
        200
    );
    env.server().clock.advance(24 * 3600 + 1);
    let store = onepaste_server::store::Store::open(&env.dir.0.join("data")).unwrap();
    store
        .cleanup(
            env.server().clock.now(),
            env.cfg.tombstone_retention_secs,
            env.cfg.idempotency_ttl_secs,
        )
        .unwrap();
    assert_eq!(env.get("/v1/blobs/orphan", &dev.token).await.status, 404);
    assert_eq!(
        env.get("/v1/blobs/complete-file/0", &dev.token)
            .await
            .status,
        200
    );
    let page = env.get("/v1/items?after=0&limit=100", &dev.token).await;
    assert!(!page.json.to_string().contains("PRIVATE-NAME-PROBE"));
    assert!(!captured_logs().contains("PRIVATE-NAME-PROBE"));
    let r = env
        .call(
            Method::PUT,
            "/v1/items/file-item",
            Some(&dev.token),
            Some(json!({"base_seq":seq,"deleted":true})),
        )
        .await;
    assert_eq!(r.status, 200);
    env.server().clock.advance(24 * 3600 + 1);
    store
        .cleanup(
            env.server().clock.now(),
            env.cfg.tombstone_retention_secs,
            env.cfg.idempotency_ttl_secs,
        )
        .unwrap();
    assert_eq!(
        env.get("/v1/blobs/complete-file/0", &dev.token)
            .await
            .status,
        404
    );
}
