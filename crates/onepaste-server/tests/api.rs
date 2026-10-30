mod common;

use base64::{engine::general_purpose::STANDARD as B64, Engine};
use common::*;
use onepaste_core::crypto::Key;
use onepaste_core::keys::KeyParams;
use reqwest::Method;
use serde_json::{json, Value};

fn code(r: &Resp) -> &str {
    r.json["code"].as_str().unwrap_or("")
}

#[tokio::test(flavor = "multi_thread")]
async fn first_device_registers_with_init_and_reads_the_record() {
    let env = Env::new().await;
    let dev = first_device(&env).await;
    let r = env.get("/v1/vault/keyparams", &dev.token).await;
    assert_eq!(r.status, 200);
    assert_eq!(r.json["version"], 1);
    assert_eq!(r.json["wrapped_mk"], dev.record.wrapped_mk);
    // The record the server hands back unlocks to the same master key
    let got: KeyParams = serde_json::from_value(r.json.clone()).unwrap();
    got.verify_mk(&dev.mk).unwrap();
    assert!(r.headers.get("x-request-id").is_some());
    assert_eq!(r.headers.get("onepaste-proto").unwrap(), "1");
}

#[tokio::test(flavor = "multi_thread")]
async fn new_device_reads_the_record_with_the_registration_credential_then_registers_with_the_proof() {
    let env = Env::new().await;
    let a = first_device(&env).await;
    let reg = invite(&env, &a).await;
    // Before registering, the credential alone is enough to read the record; reading does not consume it
    for _ in 0..2 {
        let r = env.send(Method::GET, "/v1/vault/keyparams", None, None, None, &[("OnePaste-Reg-Token", &reg)]).await;
        assert_eq!(r.status, 200, "{}", r.json);
        let rec: KeyParams = serde_json::from_value(r.json).unwrap();
        let mk = rec.unlock("pw").unwrap();
        rec.verify_mk(&mk).unwrap();
    }
    let b = join_device(&env, &reg, &a).await;
    assert_eq!(env.get("/v1/vault/keyparams", &b.token).await.status, 200);
}

#[tokio::test(flavor = "multi_thread")]
async fn wrong_vault_proof_is_rejected_and_the_credential_still_works() {
    let env = Env::new().await;
    let a = first_device(&env).await;
    let reg = invite(&env, &a).await;
    let id = new_id();
    let wrong = Key::random().unwrap();
    let r = env
        .call(Method::POST, "/v1/devices/register", None, Some(json!({
            "reg_token": reg, "device_id": id, "sealed_name": sealed_name(&a.mk, &id, "x"), "vault_proof": proof_b64(&wrong)
        })))
        .await;
    assert_eq!((r.status, code(&r)), (401, "registration_invalid"));
    // The same credential, with the right proof, is still usable
    join_device(&env, &reg, &a).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_registration_credential_works_once_and_expires() {
    let env = Env::new().await;
    let a = first_device(&env).await;
    let reg = invite(&env, &a).await;
    join_device(&env, &reg, &a).await;
    // Used once
    let id = new_id();
    let body = json!({ "reg_token": reg, "device_id": id, "sealed_name": sealed_name(&a.mk, &id, "x"), "vault_proof": proof_b64(&a.mk) });
    let r = env.call(Method::POST, "/v1/devices/register", None, Some(body.clone())).await;
    assert_eq!((r.status, code(&r)), (401, "registration_invalid"));
    // Expired: move the server clock instead of waiting
    let fresh = invite(&env, &a).await;
    env.server().clock.advance(env.cfg.reg_token_ttl_secs + 1);
    let r = env.send(Method::GET, "/v1/vault/keyparams", None, None, None, &[("OnePaste-Reg-Token", &fresh)]).await;
    assert_eq!((r.status, code(&r)), (401, "registration_invalid"));
    let body = json!({ "reg_token": fresh, "device_id": id, "sealed_name": sealed_name(&a.mk, &id, "x"), "vault_proof": proof_b64(&a.mk) });
    let r = env.call(Method::POST, "/v1/devices/register", None, Some(body)).await;
    assert_eq!((r.status, code(&r)), (401, "registration_invalid"));
}

#[tokio::test(flavor = "multi_thread")]
async fn init_is_required_for_an_empty_vault_and_forbidden_for_an_existing_one() {
    let env = Env::new().await;
    let reg = reg_token_of(env.server().first_connect_code.as_deref().unwrap());
    let (record, mk) = KeyParams::create("pw", KDF).unwrap();
    let id = new_id();
    // Empty vault without init
    let r = env
        .call(Method::POST, "/v1/devices/register", None, Some(json!({
            "reg_token": reg, "device_id": id, "sealed_name": sealed_name(&mk, &id, "x"), "vault_proof": proof_b64(&mk)
        })))
        .await;
    assert_eq!((r.status, code(&r)), (400, "invalid_argument"));
    // Empty vault with both init and a top-level proof
    let r = env
        .call(Method::POST, "/v1/devices/register", None, Some(json!({
            "reg_token": reg, "device_id": id, "sealed_name": sealed_name(&mk, &id, "x"), "vault_proof": proof_b64(&mk),
            "init": { "record": record, "vault_proof": proof_b64(&mk) }
        })))
        .await;
    assert_eq!(r.status, 400);
    // The failed attempts did not consume the credential
    let a = first_device(&env).await;
    // Existing vault with init
    let reg2 = invite(&env, &a).await;
    let id2 = new_id();
    let r = env
        .call(Method::POST, "/v1/devices/register", None, Some(json!({
            "reg_token": reg2, "device_id": id2, "sealed_name": sealed_name(&mk, &id2, "x"),
            "init": { "record": record, "vault_proof": proof_b64(&mk) }
        })))
        .await;
    assert_eq!((r.status, code(&r)), (400, "invalid_argument"));
}

#[tokio::test(flavor = "multi_thread")]
async fn put_keyparams_checks_version_proof_and_creation() {
    let env = Env::new().await;
    let a = first_device(&env).await;
    let next = a.record.change_passphrase("pw", "pw2").unwrap();
    let put = |rec: &KeyParams, base: i64, proof: String| json!({ "record": rec, "base_version": base, "vault_proof": proof });
    // Creation through PUT is never allowed
    let r = env.call(Method::PUT, "/v1/vault/keyparams", Some(&a.token), Some(put(&next, 0, proof_b64(&a.mk)))).await;
    assert_eq!((r.status, code(&r)), (400, "invalid_argument"));
    // Wrong proof
    let r = env.call(Method::PUT, "/v1/vault/keyparams", Some(&a.token), Some(put(&next, 1, proof_b64(&Key::random().unwrap())))).await;
    assert_eq!((r.status, code(&r)), (401, "unauthorized"));
    // Good write
    let r = env.call(Method::PUT, "/v1/vault/keyparams", Some(&a.token), Some(put(&next, 1, proof_b64(&a.mk)))).await;
    assert_eq!((r.status, r.json["version"].as_i64()), (200, Some(2)));
    // Stale version carries the current record
    let r = env.call(Method::PUT, "/v1/vault/keyparams", Some(&a.token), Some(put(&next, 1, proof_b64(&a.mk)))).await;
    assert_eq!((r.status, code(&r)), (409, "stale"));
    assert_eq!(r.json["current_keyparams"]["version"], 2);
    assert_eq!(r.json["current_keyparams"]["salt"], json!(next.salt));
}

#[tokio::test(flavor = "multi_thread")]
async fn items_create_conflict_delete_and_page_in_seq_order() {
    let env = Env::new().await;
    let a = first_device(&env).await;
    let (i1, i2, i3) = (new_id(), new_id(), new_id());
    for (n, id) in [&i1, &i2, &i3].into_iter().enumerate() {
        let r = put_item(&env, &a, id, 0, "text").await;
        assert_eq!((r.status, r.json["seq"].as_i64()), (200, Some(n as i64 + 1)), "{}", r.json);
    }
    let page = env.get("/v1/items?after=0&limit=2", &a.token).await;
    let ids: Vec<&str> = page.json["items"].as_array().unwrap().iter().map(|i| i["id"].as_str().unwrap()).collect();
    assert_eq!(ids, [i1.as_str(), i2.as_str()]);
    assert_eq!((page.json["has_more"].clone(), page.json["next_after"].clone(), page.json["head_seq"].clone()), (json!(true), json!(2), json!(3)));
    let page2 = env.get("/v1/items?after=2&limit=2", &a.token).await;
    assert_eq!(page2.json["items"].as_array().unwrap().len(), 1);
    assert_eq!(page2.json["has_more"], false);
    // A wrong base_seq is stale and attaches the current item
    let r = put_item(&env, &a, &i1, 0, "again").await;
    assert_eq!((r.status, code(&r)), (409, "stale"));
    assert_eq!(r.json["current_item"]["seq"], 1);
    assert!(r.json["current_item"]["envelope"].is_object());
    // Delete leaves a tombstone without an envelope; head_seq follows every write
    let r = env.call(Method::PUT, &format!("/v1/items/{i2}"), Some(&a.token), Some(json!({ "base_seq": 2, "deleted": true }))).await;
    assert_eq!((r.status, r.json["seq"].as_i64()), (200, Some(4)));
    let all = env.get("/v1/items", &a.token).await;
    assert_eq!(all.json["head_seq"], 4);
    let items = all.json["items"].as_array().unwrap();
    let seqs: Vec<i64> = items.iter().map(|i| i["seq"].as_i64().unwrap()).collect();
    assert_eq!(seqs, [1, 3, 4]);
    let tomb = &items[2];
    assert_eq!((tomb["id"].as_str().unwrap(), tomb["deleted"].clone()), (i2.as_str(), json!(true)));
    assert!(tomb.get("envelope").is_none());
    // The envelope id must match the path
    let r = env.call(Method::PUT, &format!("/v1/items/{}", new_id()), Some(&a.token), Some(json!({ "base_seq": 0, "envelope": envelope_json(&a.mk, &i1, "x") }))).await;
    assert_eq!(r.status, 400);
}

#[tokio::test(flavor = "multi_thread")]
async fn idempotency_replays_and_detects_a_different_request() {
    let env = Env::new().await;
    let a = first_device(&env).await;
    let id = new_id();
    let body = json!({ "base_seq": 0, "envelope": envelope_json(&a.mk, &id, "once") }).to_string().into_bytes();
    let path = format!("/v1/items/{id}");
    let r1 = env.send(Method::PUT, &path, Some(&a.token), Some("same-key"), Some(body.clone()), &[]).await;
    let r2 = env.send(Method::PUT, &path, Some(&a.token), Some("same-key"), Some(body), &[]).await;
    assert_eq!((r1.status, r2.status), (200, 200));
    assert_eq!(r1.json, r2.json);
    assert_eq!(env.get("/v1/items", &a.token).await.json["head_seq"], 1, "the replay must not advance seq");
    let other = new_id();
    let body = json!({ "base_seq": 0, "envelope": envelope_json(&a.mk, &other, "different") }).to_string().into_bytes();
    let r = env.send(Method::PUT, &format!("/v1/items/{other}"), Some(&a.token), Some("same-key"), Some(body), &[]).await;
    assert_eq!((r.status, code(&r)), (422, "idempotency_conflict"));
    // A stale answer is remembered too: replaying it is fine, changing the body under the same key is not
    let stale_body = json!({ "base_seq": 0, "envelope": envelope_json(&a.mk, &id, "again") }).to_string().into_bytes();
    let s1 = env.send(Method::PUT, &path, Some(&a.token), Some("stale-key"), Some(stale_body.clone()), &[]).await;
    let s2 = env.send(Method::PUT, &path, Some(&a.token), Some("stale-key"), Some(stale_body), &[]).await;
    assert_eq!((s1.status, s2.status), (409, 409));
    assert_eq!(s1.json["current_item"], s2.json["current_item"]);
    let changed = json!({ "base_seq": 1, "envelope": envelope_json(&a.mk, &id, "merged") }).to_string().into_bytes();
    let s3 = env.send(Method::PUT, &path, Some(&a.token), Some("stale-key"), Some(changed.clone()), &[]).await;
    assert_eq!((s3.status, code(&s3)), (422, "idempotency_conflict"));
    let s4 = env.send(Method::PUT, &path, Some(&a.token), Some("fresh-key"), Some(changed), &[]).await;
    assert_eq!(s4.status, 200, "{}", s4.json);
    // Registration replays the same device token without storing it in the clear
    let reg = invite(&env, &a).await;
    let id = new_id();
    let body = json!({ "reg_token": reg, "device_id": id, "sealed_name": sealed_name(&a.mk, &id, "n"), "vault_proof": proof_b64(&a.mk) }).to_string().into_bytes();
    let g1 = env.send(Method::POST, "/v1/devices/register", None, Some("reg-key"), Some(body.clone()), &[]).await;
    let g2 = env.send(Method::POST, "/v1/devices/register", None, Some("reg-key"), Some(body), &[]).await;
    assert_eq!((g1.status, g2.status), (200, 200));
    assert_eq!(g1.json["device_token"], g2.json["device_token"]);
    let token = g1.json["device_token"].as_str().unwrap();
    let db = std::fs::read(env.cfg.data_dir.join("onepaste.db")).unwrap();
    assert!(!db.windows(token.len()).any(|w| w == token.as_bytes()), "the device token must not be stored in the clear");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_revoked_device_is_rejected_on_its_next_request() {
    let env = Env::new().await;
    let a = first_device(&env).await;
    let reg = invite(&env, &a).await;
    let b = join_device(&env, &reg, &a).await;
    assert_eq!(env.get("/v1/items", &b.token).await.status, 200);
    let r = env.call(Method::DELETE, &format!("/v1/devices/{}", b.id), Some(&a.token), None).await;
    assert_eq!(r.status, 204);
    let r = env.get("/v1/items", &b.token).await;
    assert_eq!((r.status, code(&r)), (401, "unauthorized"));
    let list = env.get("/v1/devices", &a.token).await;
    let revoked: Vec<bool> = list.json.as_array().unwrap().iter().map(|d| d["revoked"].as_bool().unwrap()).collect();
    assert_eq!(revoked, [false, true]);
}

#[tokio::test(flavor = "multi_thread")]
async fn an_old_cursor_gets_410_after_tombstones_are_purged() {
    let env = Env::with(|c| c.tombstone_retention_secs = 10).await;
    let a = first_device(&env).await;
    let (i1, i2) = (new_id(), new_id());
    put_item(&env, &a, &i1, 0, "a").await;
    env.call(Method::PUT, &format!("/v1/items/{i1}"), Some(&a.token), Some(json!({ "base_seq": 1, "deleted": true }))).await;
    put_item(&env, &a, &i2, 0, "b").await;
    env.server().clock.advance(11);
    env.server().cleanup_now();
    let r = env.get("/v1/items?after=1", &a.token).await;
    assert_eq!((r.status, code(&r)), (410, "cursor_expired"));
    // A full pull and a cursor at or past the purge point still work
    let full = env.get("/v1/items?after=0", &a.token).await;
    assert_eq!(full.status, 200);
    assert_eq!(full.json["items"].as_array().unwrap().len(), 1);
    assert_eq!(env.get("/v1/items?after=2", &a.token).await.status, 200);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_write_is_announced_on_the_event_stream_within_two_seconds() {
    let env = Env::new().await;
    let a = first_device(&env).await;
    let mut stream = env.http.get(format!("{}/v1/events", env.base)).header("OnePaste-Proto", "1").header("Authorization", format!("Bearer {}", a.token)).send().await.unwrap();
    assert_eq!(stream.status().as_u16(), 200);
    put_item(&env, &a, &new_id(), 0, "ping").await;
    let got = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            let chunk = stream.chunk().await.unwrap().expect("stream ended");
            let text = String::from_utf8_lossy(&chunk).into_owned();
            if text.contains("data:") {
                return text;
            }
        }
    })
    .await
    .expect("no event within 2 seconds");
    assert!(got.contains("\"seq\":1"), "{got}");
}

#[tokio::test(flavor = "multi_thread")]
async fn too_many_registration_requests_get_429_with_retry_after() {
    let env = Env::with(|c| c.register_per_min = 3).await;
    let mut statuses = Vec::new();
    let mut last = None;
    for _ in 0..5 {
        let r = env.call(Method::POST, "/v1/devices/register", None, Some(json!({}))).await;
        statuses.push(r.status);
        last = Some(r);
    }
    assert_eq!(statuses, [400, 400, 400, 429, 429]);
    let last = last.unwrap();
    assert_eq!(code(&last), "rate_limited");
    let wait: u64 = last.headers.get("retry-after").unwrap().to_str().unwrap().parse().unwrap();
    assert!(wait >= 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_protocol_header_is_required_and_checked() {
    let env = Env::new().await;
    let a = first_device(&env).await;
    for header in [None, Some("2"), Some("")] {
        let mut rb = env.http.get(format!("{}/v1/items", env.base)).header("Authorization", format!("Bearer {}", a.token));
        if let Some(h) = header {
            rb = rb.header("OnePaste-Proto", h);
        }
        let r = rb.send().await.unwrap();
        assert_eq!(r.status().as_u16(), 400);
        let v: Value = serde_json::from_slice(&r.bytes().await.unwrap()).unwrap();
        assert_eq!(v["code"], "unsupported_version");
        assert_eq!((v["min"].clone(), v["max"].clone()), (json!(1), json!(1)));
        assert!(v["request_id"].is_string());
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn oversized_bodies_and_envelopes_get_413() {
    let env = Env::new().await;
    let a = first_device(&env).await;
    let id = new_id();
    // The request body is over 12 MiB
    let big = vec![b'a'; 13 * 1024 * 1024];
    let r = env.send(Method::PUT, &format!("/v1/items/{id}"), Some(&a.token), None, Some(big), &[]).await;
    assert_eq!((r.status, code(&r)), (413, "too_large"));
    // The body is under 12 MiB but the sealed item is over 8 MiB: 413, not 400
    let raw = vec![7u8; 8 * 1024 * 1024 + 1];
    let env_json = json!({ "v": 1, "id": id, "created_at": 1, "updated_at": 1, "size": raw.len(), "sealed": B64.encode(&raw) });
    let r = env.call(Method::PUT, &format!("/v1/items/{id}"), Some(&a.token), Some(json!({ "base_seq": 0, "envelope": env_json }))).await;
    assert_eq!((r.status, code(&r)), (413, "too_large"));
    // Exactly 8 MiB passes the size check (the content is not decryptable, but the server never decrypts)
    let raw = vec![7u8; 8 * 1024 * 1024];
    let env_json = json!({ "v": 1, "id": id, "created_at": 1, "updated_at": 1, "size": raw.len(), "sealed": B64.encode(&raw) });
    let r = env.call(Method::PUT, &format!("/v1/items/{id}"), Some(&a.token), Some(json!({ "base_seq": 0, "envelope": env_json }))).await;
    assert_eq!(r.status, 200, "{}", r.json);
}

/// Deterministic xorshift generator, so a failure can be reproduced.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

fn fuzz_payload(rng: &mut Rng, valid: &[u8]) -> Vec<u8> {
    let shapes: [&[u8]; 9] = [b"", b"null", b"[]", b"\"x\"", b"{}", b"{\"base_seq\":1e999}", b"{\"reg_token\":1,\"device_id\":[],\"sealed_name\":{}}", b"{\"record\":{\"v\":\"1\"},\"base_version\":-5}", b"{\"base_seq\":-1,\"envelope\":{\"v\":300,\"size\":-3}}"];
    match rng.below(5) {
        0 => (0..rng.below(300)).map(|_| rng.next() as u8).collect(),
        // A proper prefix is never a complete JSON document
        1 => valid[..rng.below(valid.len())].to_vec(),
        2 => {
            let mut v = valid.to_vec();
            for _ in 0..1 + rng.below(6) {
                let i = rng.below(v.len());
                v[i] = rng.next() as u8;
            }
            // Trailing garbage guarantees the document is invalid, however the bytes changed
            v.push(0);
            v
        }
        3 => shapes[rng.below(shapes.len())].to_vec(),
        _ => b"[".repeat(1 + rng.below(5000)),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn malformed_input_never_causes_a_5xx_or_a_crash() {
    let env = Env::with(|c| {
        c.register_per_min = 1_000_000;
        c.auth_fail_per_min = 1_000_000;
    })
    .await;
    let a = first_device(&env).await;
    let id = new_id();
    let (record, mk) = KeyParams::create("pw", KDF).unwrap();
    let valid_register = json!({ "reg_token": "x", "device_id": id, "sealed_name": "AAAA", "init": { "record": record, "vault_proof": proof_b64(&mk) } }).to_string().into_bytes();
    let valid_keyparams = json!({ "record": record, "base_version": 1, "vault_proof": proof_b64(&mk) }).to_string().into_bytes();
    let valid_item = json!({ "base_seq": 0, "envelope": envelope_json(&a.mk, &id, "x") }).to_string().into_bytes();
    let targets: [(Method, String, Option<&str>, &[u8]); 3] = [
        (Method::POST, "/v1/devices/register".into(), None, &valid_register),
        (Method::PUT, "/v1/vault/keyparams".into(), Some(&a.token), &valid_keyparams),
        (Method::PUT, format!("/v1/items/{id}"), Some(&a.token), &valid_item),
    ];
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    for (method, path, token, valid) in targets {
        for n in 0..500 {
            let body = fuzz_payload(&mut rng, valid);
            let r = env.send(method.clone(), &path, token, None, Some(body), &[]).await;
            assert!((400..500).contains(&r.status), "{method} {path} payload {n}: status {}", r.status);
        }
    }
    // The server is still healthy
    assert_eq!(env.get("/v1/items", &a.token).await.status, 200);
}

#[tokio::test(flavor = "multi_thread")]
async fn logs_do_not_contain_secrets_or_payloads() {
    let env = Env::new().await;
    let a = first_device(&env).await;
    let reg = invite(&env, &a).await;
    let proof = proof_b64(&a.mk);
    let marker = "ONEPASTE-LEAK-MARKER-7f3a";
    let id = new_id();
    let envelope = envelope_json(&a.mk, &id, marker);
    let sealed = envelope["sealed"].as_str().unwrap().to_string();
    env.call(Method::PUT, &format!("/v1/items/{id}"), Some(&a.token), Some(json!({ "base_seq": 0, "envelope": envelope }))).await;
    env.send(Method::GET, "/v1/vault/keyparams", None, None, None, &[("OnePaste-Reg-Token", &reg)]).await;
    join_device(&env, &reg, &a).await;
    env.call(Method::POST, "/v1/devices/register", None, Some(json!({ "reg_token": reg, "device_id": id, "sealed_name": "AAAA", "vault_proof": proof }))).await;
    let logs = captured_logs();
    assert!(logs.contains("request_id") && logs.contains("latency_ms"), "the access log should exist");
    for secret in [a.token.as_str(), reg.as_str(), proof.as_str(), sealed.as_str(), marker, "Authorization", "Bearer"] {
        assert!(!logs.contains(secret), "the log contains a secret");
    }
    let first_code = env.server().first_connect_code.clone().unwrap();
    assert!(!logs.contains(&first_code), "the connection code must be printed to standard output only");
}

#[tokio::test(flavor = "multi_thread")]
async fn data_certificate_and_sequence_survive_a_restart() {
    let mut env = Env::new().await;
    let a = first_device(&env).await;
    let id = new_id();
    put_item(&env, &a, &id, 0, "kept").await;
    let (fp, before) = (env.fingerprint.clone(), env.get("/v1/items", &a.token).await.json);
    env.restart().await;
    assert_eq!(env.server().fingerprint, fp);
    assert!(env.server().first_connect_code.is_none(), "a vault with a device must not print a new connection code");
    assert_eq!(env.get("/v1/items", &a.token).await.json, before);
    let r = put_item(&env, &a, &new_id(), 0, "next").await;
    assert_eq!(r.json["seq"], 2);
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn the_private_key_file_is_private() {
    use std::os::unix::fs::PermissionsExt;
    let env = Env::new().await;
    let mode = |p: std::path::PathBuf| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(env.cfg.data_dir.join("tls/key.pem")), 0o600);
    assert_eq!(mode(env.cfg.data_dir.join("onepaste.db")), 0o600);
    assert_eq!(mode(env.cfg.data_dir.clone()), 0o700);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_connect_code_subcommand_invalidates_older_credentials() {
    let env = Env::new().await;
    let first = reg_token_of(env.server().first_connect_code.as_deref().unwrap());
    let fresh = onepaste_server::connect_code(&env.cfg).unwrap();
    let r = env.send(Method::GET, "/v1/vault/keyparams", None, None, None, &[("OnePaste-Reg-Token", &first)]).await;
    assert_eq!(r.status, 401, "the older credential must be invalid");
    let r = env.send(Method::GET, "/v1/vault/keyparams", None, None, None, &[("OnePaste-Reg-Token", &reg_token_of(&fresh))]).await;
    assert_eq!((r.status, code(&r)), (404, "vault_empty"), "the new credential is valid and the vault is still empty");
}

#[tokio::test(flavor = "multi_thread")]
async fn only_the_pinned_certificate_is_accepted() {
    let env = Env::new().await;
    let url = format!("{}/v1/items", env.base);
    let get = |client: reqwest::Client| {
        let url = url.clone();
        async move { client.get(url).header("OnePaste-Proto", "1").send().await }
    };
    // The right fingerprint connects (the answer is 401 because there is no token, but the TLS handshake worked)
    assert_eq!(get(pinned_client(&env.fingerprint)).await.unwrap().status().as_u16(), 401);
    // A wrong fingerprint is refused
    assert!(get(pinned_client("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA")).await.is_err());
    // A second server has a fresh certificate and so a different fingerprint: the old fingerprint must not connect to it
    let other = Env::new().await;
    assert_ne!(other.fingerprint, env.fingerprint);
    let other_url = format!("{}/v1/items", other.base);
    let old = pinned_client(&env.fingerprint).get(other_url.clone()).header("OnePaste-Proto", "1").send().await;
    assert!(old.is_err(), "the old fingerprint must not trust the new certificate");
    let new = pinned_client(&other.fingerprint).get(other_url).header("OnePaste-Proto", "1").send().await;
    assert_eq!(new.unwrap().status().as_u16(), 401);
}

#[derive(Debug)]
struct Impostor(std::sync::Arc<rustls::sign::CertifiedKey>);

impl rustls::server::ResolvesServerCert for Impostor {
    fn resolve(&self, _hello: rustls::server::ClientHello<'_>) -> Option<std::sync::Arc<rustls::sign::CertifiedKey>> {
        Some(self.0.clone())
    }
}

/// The certificate is public, so presenting it proves nothing: the handshake signature has to verify as well.
#[tokio::test(flavor = "multi_thread")]
async fn an_impostor_with_the_pinned_certificate_but_another_key_is_refused() {
    use base64::{engine::general_purpose::STANDARD as B64, Engine};
    use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
    use std::sync::Arc;
    let env = Env::new().await;
    let pem = std::fs::read_to_string(env.cfg.data_dir.join("tls/cert.pem")).unwrap();
    let der = B64.decode(pem.lines().filter(|l| !l.starts_with("-----")).collect::<String>()).unwrap();
    let other = rcgen::KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256).unwrap();
    let signing = rustls::crypto::ring::sign::any_ecdsa_type(&PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(other.serialize_der()))).unwrap();
    let certified = rustls::sign::CertifiedKey::new(vec![CertificateDer::from(der)], signing);
    let cfg = rustls::ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_cert_resolver(Arc::new(Impostor(Arc::new(certified))));
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    let app = axum::Router::new().route("/v1/items", axum::routing::get(|| async { "impostor" }));
    let tls = axum_server::tls_rustls::RustlsConfig::from_config(Arc::new(cfg));
    tokio::spawn(async move {
        let _ = axum_server::from_tcp_rustls(listener, tls).unwrap().serve(app.into_make_service()).await;
    });
    let r = pinned_client(&env.fingerprint).get(format!("https://127.0.0.1:{port}/v1/items")).send().await;
    assert!(r.is_err(), "a server that cannot sign with the pinned certificate's key must be refused");
}

/// A client that rebuilds its copy from scratch walks the pages below the purge point. Those cursors were handed out by
/// the server a moment ago and must keep working, while a really old cursor (or another purge in between) must not.
#[tokio::test(flavor = "multi_thread")]
async fn a_full_pull_in_pages_is_not_cut_off_by_the_purge_point() {
    let env = Env::with(|c| c.tombstone_retention_secs = 10).await;
    let a = first_device(&env).await;
    let ids: Vec<String> = (0..5).map(|_| new_id()).collect();
    for id in &ids {
        put_item(&env, &a, id, 0, "x").await;
    }
    for (n, id) in ids.iter().enumerate().take(2) {
        env.call(Method::PUT, &format!("/v1/items/{id}"), Some(&a.token), Some(json!({ "base_seq": n as i64 + 1, "deleted": true }))).await;
    }
    env.server().clock.advance(11);
    env.server().cleanup_now();
    // A cursor from before the purge is expired
    assert_eq!(env.get("/v1/items?after=5", &a.token).await.status, 410);
    // A full pull in pages of two: the second page starts at a cursor below the purge point
    let p1 = env.get("/v1/items?after=0&limit=2", &a.token).await;
    assert_eq!((p1.status, p1.json["has_more"].clone()), (200, json!(true)));
    let next = p1.json["next_after"].as_i64().unwrap();
    let p2 = env.get(&format!("/v1/items?after={next}&limit=2"), &a.token).await;
    assert_eq!(p2.status, 200, "the cursor the server just handed out must work: {}", p2.json);
    // Another device never started a pull, so the same number is just an old cursor for it
    let reg = invite(&env, &a).await;
    let b = join_device(&env, &reg, &a).await;
    assert_eq!(env.get(&format!("/v1/items?after={next}"), &b.token).await.status, 410);
    // A purge after the pull started invalidates the cursor again
    env.call(Method::PUT, &format!("/v1/items/{}", ids[2]), Some(&a.token), Some(json!({ "base_seq": 3, "deleted": true }))).await;
    env.server().clock.advance(11);
    env.server().cleanup_now();
    assert_eq!(env.get(&format!("/v1/items?after={next}&limit=2"), &a.token).await.status, 410);
}

#[tokio::test(flavor = "multi_thread")]
async fn oversized_upload_can_finish_sending_before_reading_its_413_response() {
    let env = Env::new().await;
    let device = first_device(&env).await;
    let address = env.server().addr;
    let fingerprint = env.fingerprint.clone();
    tokio::task::spawn_blocking(move || {
        use std::io::{Read, Write};
        let config = rustls::ClientConfig::builder_with_provider(std::sync::Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions().unwrap().dangerous()
            .with_custom_certificate_verifier(std::sync::Arc::new(onepaste_client::pin::PinVerifier::new(&fingerprint)))
            .with_no_client_auth();
        let connection = rustls::ClientConnection::new(std::sync::Arc::new(config), rustls::pki_types::ServerName::try_from("localhost").unwrap()).unwrap();
        let socket = std::net::TcpStream::connect(address).unwrap();
        socket.set_read_timeout(Some(std::time::Duration::from_secs(10))).unwrap();
        socket.set_write_timeout(Some(std::time::Duration::from_secs(10))).unwrap();
        let mut stream = rustls::StreamOwned::new(connection, socket);
        let headers = format!("PUT /v1/items/{} HTTP/1.1\r\nHost: localhost\r\nOnePaste-Proto: 1\r\nAuthorization: Bearer {}\r\nIdempotency-Key: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",new_id(),device.token,new_id(),13*1024*1024);
        stream.write_all(headers.as_bytes()).unwrap(); stream.flush().unwrap();
        // An early close must not replace the API's size error with a TLS/body write failure.
        for _ in 0..13*64 {
            stream.write_all(&[b'a';16*1024]).unwrap(); stream.flush().unwrap();
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        let mut response=String::new(); stream.read_to_string(&mut response).unwrap();
        assert!(response.starts_with("HTTP/1.1 413")); assert!(response.contains("too_large"));
    }).await.unwrap();
}
