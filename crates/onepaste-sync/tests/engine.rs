//! Engine tests: an in-memory store and simulated server answers. No real server, no network.

use onepaste_core::crypto::Key;
use onepaste_core::envelope::{seal_item, ItemPayload};
use onepaste_sync::*;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};

const T0: u64 = 1_700_000_000_000;

fn text(s: &str) -> ItemPayload {
    ItemPayload { kind: "text".into(), content: Some(s.into()), ..Default::default() }
}

fn key_copy(k: &Key) -> Key {
    Key::from_bytes(*k.as_bytes())
}

fn engine(mk: &Key) -> Engine<MemStore> {
    Engine::new(MemStore::new(), key_copy(mk), "device-token".into())
}

fn resp(status: u16, v: Value) -> HttpResponse {
    HttpResponse { status, headers: Vec::new(), body: v.to_string().into_bytes() }
}

struct Rec {
    seq: i64,
    deleted: bool,
    env: Option<Value>,
}

/// Just enough of the server's behaviour: optimistic `seq`, idempotency replay, paging and `head_seq`.
#[derive(Default)]
struct Fake {
    seq: i64,
    items: BTreeMap<String, Rec>,
    idem: HashMap<String, (Vec<u8>, u16, Value)>,
    page_cap: Option<usize>,
    expire_below: i64,
    /// Another device writes to the item just before every upload is looked at.
    compete: bool,
    puts: Vec<(String, i64)>,
}

fn record(id: &str, r: &Rec) -> Value {
    let mut v = json!({ "id": id, "seq": r.seq, "deleted": r.deleted });
    if let Some(e) = &r.env {
        v["envelope"] = e.clone();
    }
    v
}

impl Fake {
    fn handle(&mut self, req: &HttpRequest) -> HttpResponse {
        if req.method == Method::Put {
            let id = req.path_and_query.rsplit('/').next().unwrap().to_string();
            let key = req.header("Idempotency-Key").unwrap().to_string();
            if let Some((body, status, out)) = self.idem.get(&key) {
                return if *body == req.body { resp(*status, out.clone()) } else { resp(422, json!({ "code": "idempotency_conflict" })) };
            }
            if self.compete {
                if let Some(r) = self.items.get_mut(&id) {
                    self.seq += 1;
                    r.seq = self.seq;
                }
            }
            let body: Value = serde_json::from_slice(&req.body).unwrap();
            let base = body["base_seq"].as_i64().unwrap();
            self.puts.push((key.clone(), base));
            let cur = self.items.get(&id).map(|r| r.seq).unwrap_or(0);
            if base != cur {
                let mut v = json!({ "code": "stale" });
                if let Some(r) = self.items.get(&id) {
                    v["current_item"] = record(&id, r);
                }
                return resp(409, v);
            }
            self.seq += 1;
            let rec = if body["deleted"] == json!(true) {
                Rec { seq: self.seq, deleted: true, env: None }
            } else {
                Rec { seq: self.seq, deleted: false, env: Some(body["envelope"].clone()) }
            };
            self.items.insert(id, rec);
            let out = json!({ "seq": self.seq });
            self.idem.insert(key, (req.body.clone(), 200, out.clone()));
            return resp(200, out);
        }
        let q = req.path_and_query.split_once('?').unwrap().1;
        let get = |name: &str| q.split('&').find_map(|p| p.strip_prefix(&format!("{name}="))).unwrap().parse::<i64>().unwrap();
        let (after, limit) = (get("after"), get("limit") as usize);
        if after > 0 && after < self.expire_below {
            return resp(410, json!({ "code": "cursor_expired" }));
        }
        let limit = self.page_cap.map_or(limit, |c| c.min(limit));
        let mut rows: Vec<(&String, &Rec)> = self.items.iter().filter(|(_, r)| r.seq > after).collect();
        rows.sort_by_key(|(_, r)| r.seq);
        let has_more = rows.len() > limit;
        let page: Vec<Value> = rows.iter().take(limit).map(|(id, r)| record(id, r)).collect();
        let next_after = rows.iter().take(limit).last().map_or(after, |(_, r)| r.seq);
        resp(200, json!({ "items": page, "next_after": next_after, "has_more": has_more, "head_seq": self.seq }))
    }
}

/// Runs the engine against the simulated server until it has nothing left to do.
fn pump(e: &mut Engine<MemStore>, s: &mut Fake, now: &mut u64) -> Vec<Event> {
    let mut events = Vec::new();
    for _ in 0..200 {
        match e.next_action(*now).unwrap() {
            Action::Send(req) => {
                let r = s.handle(&req);
                events.extend(e.on_response(*now, r).unwrap());
            }
            Action::Wait { until_ms } => *now = until_ms,
            Action::Idle | Action::Paused(_) => return events,
            Action::InFlight => panic!("a request is still outstanding"),
        }
    }
    panic!("the engine did not settle");
}

fn pull(e: &mut Engine<MemStore>, s: &mut Fake, now: &mut u64) -> Vec<Event> {
    e.request_pull();
    pump(e, s, now)
}

fn payload_of(e: &mut Engine<MemStore>, id: &str) -> ItemPayload {
    e.get_item(id).unwrap()
}

#[test]
fn a_crash_between_enqueue_and_send_resends_with_the_same_idempotency_key() {
    let mk = Key::random().unwrap();
    let mut e = engine(&mk);
    e.enqueue_create(T0, &text("hello")).unwrap();
    let key = e.outbox().unwrap()[0].idem_key.clone();
    // Drop the engine object, as a crash would, and reload from the store
    let store = e.into_store();
    let mut e2 = Engine::new(store, key_copy(&mk), "device-token".into());
    let Action::Send(req) = e2.next_action(T0).unwrap() else { panic!("expected a request") };
    assert_eq!(req.method, Method::Put);
    assert_eq!(req.header("Idempotency-Key"), Some(key.as_str()));
}

#[test]
fn network_errors_and_server_errors_retry_with_the_same_key_and_body_after_a_growing_delay() {
    let mk = Key::random().unwrap();
    let mut e = engine(&mk);
    e.enqueue_create(T0, &text("hello")).unwrap();
    let Action::Send(first) = e.next_action(T0).unwrap() else { panic!() };
    e.on_transport_error(T0).unwrap();
    let Action::Wait { until_ms } = e.next_action(T0).unwrap() else { panic!("must wait") };
    assert_eq!(until_ms - T0, 1000);
    let Action::Send(second) = e.next_action(until_ms).unwrap() else { panic!() };
    assert_eq!(first, second, "same key and byte-identical body");
    // A 503 and then a 429 with Retry-After: still the same request
    e.on_response(until_ms, resp(503, json!({ "code": "internal" }))).unwrap();
    let Action::Wait { until_ms: t2 } = e.next_action(until_ms).unwrap() else { panic!() };
    assert_eq!(t2 - until_ms, 2000);
    let Action::Send(third) = e.next_action(t2).unwrap() else { panic!() };
    assert_eq!(first, third);
    let mut limited = resp(429, json!({ "code": "rate_limited" }));
    limited.headers.push(("Retry-After".into(), "30".into()));
    e.on_response(t2, limited).unwrap();
    assert_eq!(e.next_action(t2).unwrap(), Action::Wait { until_ms: t2 + 30_000 });
    let Action::Send(fourth) = e.next_action(t2 + 30_000).unwrap() else { panic!() };
    assert_eq!(first, fourth);
}

#[test]
fn stale_resends_with_a_new_key_and_three_in_a_row_is_a_conflict_while_the_queue_goes_on() {
    let mk = Key::random().unwrap();
    let (mut a, mut b, mut s, mut now) = (engine(&mk), engine(&mk), Fake::default(), T0);
    let x = b.enqueue_create(now, &text("x")).unwrap();
    pump(&mut b, &mut s, &mut now);
    pull(&mut a, &mut s, &mut now);
    // B changes the item; A, unaware, changes it too
    b.enqueue_edit(now, &x, Edit::Pinned(true)).unwrap();
    pump(&mut b, &mut s, &mut now);
    a.enqueue_edit(now, &x, Edit::Note(Some("n".into()))).unwrap();
    s.puts.clear();
    pump(&mut a, &mut s, &mut now);
    assert_eq!(s.puts.len(), 2, "one stale attempt and one that succeeds");
    assert_ne!(s.puts[0].0, s.puts[1].0, "the body changed, so the key must change");
    assert!(s.puts[1].1 > s.puts[0].1, "retried on top of the server's version");
    // Every upload of this item loses a race: three stale answers in a row park the operation
    let y = b.enqueue_create(now, &text("y")).unwrap();
    pump(&mut b, &mut s, &mut now);
    pull(&mut a, &mut s, &mut now);
    s.compete = true;
    a.enqueue_edit(now, &y, Edit::Pinned(true)).unwrap();
    let z = a.enqueue_create(now, &text("z")).unwrap();
    s.puts.clear();
    let events = pump(&mut a, &mut s, &mut now);
    assert!(events.iter().any(|e| matches!(e, Event::Conflict { item_id, .. } if *item_id == y)), "{events:?}");
    assert_eq!(s.puts.iter().filter(|(_, _)| true).count(), 4, "three stale uploads of y, then z");
    assert!(events.iter().any(|e| matches!(e, Event::Pushed { item_id, .. } if *item_id == z)), "the next operation still goes");
    let st = a.status().unwrap();
    assert_eq!((st.pending, st.conflict), (0, 1));
}

#[test]
fn two_devices_changing_different_fields_end_up_with_both_changes() {
    let mk = Key::random().unwrap();
    let (mut a, mut b, mut s, mut now) = (engine(&mk), engine(&mk), Fake::default(), T0);
    let id = a.enqueue_create(now, &text("shared")).unwrap();
    pump(&mut a, &mut s, &mut now);
    pull(&mut b, &mut s, &mut now);
    a.enqueue_edit(now, &id, Edit::Pinned(true)).unwrap();
    b.enqueue_edit(now, &id, Edit::Note(Some("x".into()))).unwrap();
    pump(&mut a, &mut s, &mut now);
    pump(&mut b, &mut s, &mut now);
    pull(&mut a, &mut s, &mut now);
    for e in [&mut a, &mut b] {
        let p = payload_of(e, &id);
        assert!(p.pinned, "pinned must survive");
        assert_eq!(p.note.as_deref(), Some("x"));
        assert_eq!(p.content.as_deref(), Some("shared"));
    }
}

#[test]
fn deleting_wins_over_editing_whichever_comes_first() {
    let mk = Key::random().unwrap();
    for delete_first in [true, false] {
        let (mut a, mut b, mut s, mut now) = (engine(&mk), engine(&mk), Fake::default(), T0);
        let id = a.enqueue_create(now, &text("doomed")).unwrap();
        pump(&mut a, &mut s, &mut now);
        pull(&mut b, &mut s, &mut now);
        a.enqueue_delete(now, &id).unwrap();
        b.enqueue_edit(now, &id, Edit::Note(Some("late".into()))).unwrap();
        if delete_first {
            pump(&mut a, &mut s, &mut now);
            pump(&mut b, &mut s, &mut now);
        } else {
            pump(&mut b, &mut s, &mut now);
            pump(&mut a, &mut s, &mut now);
        }
        pull(&mut a, &mut s, &mut now);
        pull(&mut b, &mut s, &mut now);
        assert!(s.items[&id].deleted, "the server ends up deleted (delete first: {delete_first})");
        for e in [&mut a, &mut b] {
            assert_eq!(e.get_item(&id), Err(Error::Deleted));
            assert_eq!(e.status().unwrap().pending + e.status().unwrap().conflict, 0);
        }
    }
}

#[test]
fn a_pulled_version_older_than_the_local_one_is_dropped() {
    let mk = Key::random().unwrap();
    let (mut a, mut s, mut now) = (engine(&mk), Fake::default(), T0);
    a.enqueue_create(now, &text("first")).unwrap();
    let id = a.enqueue_create(now, &text("current")).unwrap();
    pump(&mut a, &mut s, &mut now);
    let seq = s.items[&id].seq;
    assert_eq!(seq, 2);
    // The server (wrongly) offers an older version of the same item
    let old = seal_item(&mk, &id, 1, 1, None, &text("older")).unwrap();
    a.request_pull();
    let Action::Send(_) = a.next_action(now).unwrap() else { panic!() };
    let page = json!({ "items": [{ "id": id, "seq": 1, "deleted": false, "envelope": old }], "next_after": 1, "has_more": false, "head_seq": seq });
    let events = a.on_response(now, resp(200, page)).unwrap();
    assert!(matches!(events.last(), Some(Event::Pulled { applied: 0, .. })), "{events:?}");
    assert_eq!(payload_of(&mut a, &id).content.as_deref(), Some("current"));
    let row = a.list_items().unwrap().into_iter().find(|i| i.id == id).unwrap();
    assert_eq!(row.seq, 2);
}

#[test]
fn a_head_seq_below_the_highest_seen_pauses_everything() {
    let mk = Key::random().unwrap();
    let (mut a, mut s, mut now) = (engine(&mk), Fake::default(), T0);
    for n in 0..3 {
        a.enqueue_create(now, &text(&format!("t{n}"))).unwrap();
    }
    pump(&mut a, &mut s, &mut now);
    assert_eq!(a.status().unwrap().max_seen_seq, 3);
    // The server comes back with an older database
    let mut rolled_back = Fake::default();
    let events = pull(&mut a, &mut rolled_back, &mut now);
    assert_eq!(events, vec![Event::Paused(PauseReason::ServerRollback)]);
    assert_eq!(a.status().unwrap().paused, Some(PauseReason::ServerRollback));
    // From now on no request is produced, even with new work queued and across a restart
    a.enqueue_create(now, &text("more")).unwrap();
    assert_eq!(a.next_action(now).unwrap(), Action::Paused(PauseReason::ServerRollback));
    let mut reloaded = Engine::new(a.into_store(), key_copy(&mk), "device-token".into());
    reloaded.request_pull();
    assert_eq!(reloaded.next_action(now).unwrap(), Action::Paused(PauseReason::ServerRollback));
}

#[test]
fn a_full_pull_removes_what_the_server_lost_and_a_crash_halfway_keeps_the_old_cache() {
    let mk = Key::random().unwrap();
    let (mut a, mut s, mut now) = (engine(&mk), Fake::default(), T0);
    let ids: Vec<String> = ["a", "b", "c"].iter().map(|t| a.enqueue_create(now, &text(t)).unwrap()).collect();
    pump(&mut a, &mut s, &mut now);
    pull(&mut a, &mut s, &mut now);
    assert_eq!(a.status().unwrap().cursor, 3);
    // The server forgets c entirely and no longer honours old cursors; pages hold one item
    s.items.remove(&ids[2]);
    s.expire_below = 1000;
    s.page_cap = Some(1);
    a.request_pull();
    // First request: the old cursor is refused
    let Action::Send(req) = a.next_action(now).unwrap() else { panic!() };
    assert!(req.path_and_query.contains("after=3"));
    assert_eq!(a.on_response(now, s.handle(&req)).unwrap(), vec![Event::Refetching]);
    // Second request starts from 0; apply that first of two pages, then crash
    let Action::Send(req) = a.next_action(now).unwrap() else { panic!() };
    assert!(req.path_and_query.contains("after=0"));
    a.on_response(now, s.handle(&req)).unwrap();
    let mut crashed = Engine::new(a.into_store(), key_copy(&mk), "device-token".into());
    let still: Vec<String> = crashed.list_items().unwrap().into_iter().map(|i| i.id).collect();
    assert_eq!(still.len(), 3, "nothing is dropped before the full pull has finished");
    // After the restart the unfinished full pull continues by itself and then cleans up
    s.expire_below = 0;
    pump(&mut crashed, &mut s, &mut now);
    let left: Vec<String> = crashed.list_items().unwrap().into_iter().map(|i| i.id).collect();
    let mut want = vec![ids[0].clone(), ids[1].clone()];
    want.sort();
    assert_eq!(left, want);
    assert!(!crashed.status().unwrap().refetching);
}

#[test]
fn a_401_stops_syncing_as_revoked() {
    let mk = Key::random().unwrap();
    let mut e = engine(&mk);
    e.enqueue_create(T0, &text("x")).unwrap();
    let Action::Send(_) = e.next_action(T0).unwrap() else { panic!() };
    let events = e.on_response(T0, resp(401, json!({ "code": "unauthorized" }))).unwrap();
    assert_eq!(events, vec![Event::Paused(PauseReason::Revoked)]);
    assert_eq!(e.next_action(T0).unwrap(), Action::Paused(PauseReason::Revoked));
    // The queued operation is kept for later
    assert_eq!(e.status().unwrap().pending, 1);
}

#[test]
fn bad_records_are_skipped_and_the_rest_of_the_page_still_applies() {
    let mk = Key::random().unwrap();
    let mut e = engine(&mk);
    e.request_pull();
    let Action::Send(_) = e.next_action(T0).unwrap() else { panic!() };
    let good = seal_item(&mk, "good", 1, 1, None, &text("fine")).unwrap();
    let other = seal_item(&mk, "someone-else", 1, 1, None, &text("moved")).unwrap();
    let mut bad_size = serde_json::to_value(seal_item(&mk, "wrong-size", 1, 1, None, &text("x")).unwrap()).unwrap();
    bad_size["size"] = json!(3);
    let page = json!({
        "items": [
            { "id": "mismatch", "seq": 1, "deleted": false, "envelope": other },
            { "id": "wrong-size", "seq": 2, "deleted": false, "envelope": bad_size },
            { "id": "good", "seq": 3, "deleted": false, "envelope": good },
        ],
        "next_after": 3, "has_more": false, "head_seq": 3
    });
    let events = e.on_response(T0, resp(200, page)).unwrap();
    let skipped: Vec<(&str, &str)> = events.iter().filter_map(|ev| match ev {
        Event::SkippedRecord { id, code } => Some((id.as_str(), *code)),
        _ => None,
    }).collect();
    assert_eq!(skipped, vec![("mismatch", "id_mismatch"), ("wrong-size", "malformed_envelope")]);
    assert_eq!(e.get_item("good").unwrap().content.as_deref(), Some("fine"));
    assert_eq!(e.status().unwrap().cursor, 3, "the cursor still advances past the bad records");
    assert!(matches!(e.get_item("mismatch"), Err(Error::NotFound)));
}

#[test]
fn the_body_of_an_item_cannot_be_changed() {
    let mk = Key::random().unwrap();
    let mut e = engine(&mk);
    let id = e.enqueue_create(T0, &text("fixed")).unwrap();
    assert_eq!(e.enqueue_edit(T0, &id, Edit::Content(Some("changed".into()))), Err(Error::BodyImmutable));
    assert_eq!(e.enqueue_edit(T0, &id, Edit::Kind("image".into())), Err(Error::BodyImmutable));
    assert_eq!(e.outbox().unwrap().len(), 1, "only the creation is queued");
    assert_eq!(e.get_item(&id).unwrap().content.as_deref(), Some("fixed"));
}

#[test]
fn a_storage_transaction_that_fails_midway_leaves_nothing_behind() {
    let mk = Key::random().unwrap();
    let mut e = engine(&mk);
    for fail_at in 0..3 {
        e.store_mut().fail_at_write = Some(fail_at);
        assert!(matches!(e.enqueue_create(T0, &text("x")), Err(Error::Store(_))), "write {fail_at}");
        e.store_mut().fail_at_write = None;
        assert!(e.outbox().unwrap().is_empty(), "no queued operation after a failure at write {fail_at}");
        assert!(e.list_items().unwrap().is_empty(), "no item after a failure at write {fail_at}");
    }
    let id = e.enqueue_create(T0, &text("now it works")).unwrap();
    assert_eq!(e.outbox().unwrap()[0].op_id, 0, "the failed attempts did not consume an operation number");
    assert!(e.get_item(&id).is_ok());
}

#[test]
fn deleting_an_item_that_never_left_the_device_just_forgets_it() {
    let mk = Key::random().unwrap();
    let mut e = engine(&mk);
    let id = e.enqueue_create(T0, &text("scratch")).unwrap();
    e.enqueue_delete(T0, &id).unwrap();
    assert!(e.outbox().unwrap().is_empty());
    assert!(e.list_items().unwrap().is_empty());
}

#[test]
fn request_and_response_debug_output_hides_the_token_and_the_body() {
    let mk = Key::random().unwrap();
    let mut e = engine(&mk);
    e.enqueue_create(T0, &text("TOPSECRET-MARKER")).unwrap();
    let Action::Send(req) = e.next_action(T0).unwrap() else { panic!() };
    let shown = format!("{req:?}");
    assert!(!shown.contains("device-token") && !shown.contains("Bearer"), "{shown}");
    assert!(req.header("Authorization").unwrap().contains("device-token"));
    let body = String::from_utf8_lossy(&req.body).into_owned();
    assert!(!body.contains("TOPSECRET-MARKER"), "the body carries only ciphertext");
}
