use base64::{engine::general_purpose::STANDARD as B64, Engine};
use onepaste_core::crypto::Key;
use onepaste_core::envelope::{seal_item, ItemPayload};
use onepaste_core::keys::{derive_subkey, vault_proof, PURPOSE_HISTORY_DB};
use onepaste_history::{Filter, HistoryError, HistoryStore, ItemSource};
use onepaste_sync::{ItemRow, MemStore, OpState, OutboxRow, Store};
use rusqlite::Connection;
use std::path::PathBuf;

fn temp_dir(name: &str) -> PathBuf {
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let p = std::env::temp_dir().join(format!("onepaste-history-{name}-{}-{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
    let _ = std::fs::remove_dir_all(&p);
    p
}

fn text(s: &str) -> ItemPayload {
    ItemPayload { kind: "text".into(), content: Some(s.into()), ..Default::default() }
}

/// Puts a live item into the source, as the sync engine would store it.
fn put(src: &mut MemStore, mk: &Key, id: &str, seq: i64, created: i64, p: &ItemPayload) {
    let env = seal_item(mk, id, created, created, None, p).unwrap();
    let row = ItemRow { id: id.into(), seq, deleted: false, envelope_json: Some(env.to_json().unwrap()), mark: 0 };
    src.transact(&mut |tx| tx.put_item(&row)).unwrap();
}

fn tombstone(src: &mut MemStore, id: &str, seq: i64) {
    let row = ItemRow { id: id.into(), seq, deleted: true, envelope_json: None, mark: 0 };
    src.transact(&mut |tx| tx.put_item(&row)).unwrap();
}

fn forget(src: &mut MemStore, id: &str) {
    src.transact(&mut |tx| tx.remove_item(id)).unwrap();
}

fn queue(src: &mut MemStore, id: &str) {
    let op = OutboxRow { op_id: 1, item_id: id.into(), sealed_intent: vec![1], idem_key: "k".into(), attempts: 0, state: OpState::Pending, last_error: None };
    src.transact(&mut |tx| tx.put_outbox(&op)).unwrap();
}

fn ids(h: &HistoryStore, q: &str, f: Filter) -> Vec<String> {
    h.search(q, f, 1000, 0).unwrap().into_iter().map(|x| x.item_id).collect()
}

fn dump(h: &HistoryStore) -> Vec<(String, String, String, bool, bool, String)> {
    h.search("", Filter::All, 100_000, 0).unwrap().into_iter().map(|x| (x.item_id, x.kind, x.text, x.favorite, x.pinned, x.note)).collect()
}

const ZH2: &str = "\u{4e2d}\u{6587}"; // two Chinese characters
const ZH_TEST: &str = "\u{6d4b}\u{8bd5}";

// ---- 1. opening ------------------------------------------------------------------------------------------------

#[test]
fn opening_needs_the_right_key_an_empty_key_is_refused_and_files_are_private() {
    let dir = temp_dir("open");
    let mk = Key::random().unwrap();
    let store = HistoryStore::open(&dir, &mk).unwrap();
    assert!(!store.cipher_version().unwrap().is_empty(), "not a SQLCipher build");
    drop(store);
    assert!(HistoryStore::open(&dir, &mk).is_ok());
    assert!(matches!(HistoryStore::open(&dir, &Key::random().unwrap()), Err(HistoryError::WrongKey)));
    assert!(matches!(HistoryStore::open(&dir, &Key::from_bytes([0u8; 32])), Err(HistoryError::InvalidKey)));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _keep = HistoryStore::open(&dir, &mk).unwrap();
        assert_eq!(std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777, 0o700);
        for entry in std::fs::read_dir(&dir).unwrap() {
            let e = entry.unwrap();
            assert_eq!(e.metadata().unwrap().permissions().mode() & 0o777, 0o600, "{:?}", e.file_name());
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

fn hex_key(k: &Key) -> String {
    k.as_bytes().iter().map(|b| format!("{b:02x}")).collect()
}

/// Whether a raw connection can read the database when keyed with `key`.
fn opens_with(dir: &std::path::Path, key: &Key) -> bool {
    let conn = Connection::open(dir.join("history.db")).unwrap();
    conn.execute_batch(&format!("PRAGMA key = \"x'{}'\";", hex_key(key))).unwrap();
    conn.query_row("SELECT count(*) FROM sqlite_master", [], |r| r.get::<_, i64>(0)).is_ok()
}

#[test]
fn the_database_key_is_derived_for_this_purpose_and_equals_no_other_key() {
    let dir = temp_dir("key");
    let mk = Key::random().unwrap();
    drop(HistoryStore::open(&dir, &mk).unwrap());
    let history_key = derive_subkey(&mk, PURPOSE_HISTORY_DB);
    assert_ne!(history_key.as_bytes(), mk.as_bytes());
    assert!(opens_with(&dir, &history_key), "the derived key must open the database");
    assert!(!opens_with(&dir, &mk), "the master key itself (the key that encrypts items) must not open the database");
    assert!(!opens_with(&dir, &Key::from_bytes(vault_proof(&mk))), "the vault proof must not open it either");
    assert!(!opens_with(&dir, &derive_subkey(&mk, b"onepaste/other/v1")));
    assert!(!format!("{history_key:?}").contains(&hex_key(&history_key)), "the key must not show up in Debug output");
    let _ = std::fs::remove_dir_all(&dir);
}

// ---- 2. nothing readable on disk -------------------------------------------------------------------------------

fn marker_forms(marker: &str) -> Vec<(String, Vec<u8>)> {
    let b = marker.as_bytes();
    let mut forms = vec![("raw".to_string(), b.to_vec()), ("hex-lower".into(), hex_of(b).into_bytes()), ("hex-upper".into(), hex_of(b).to_uppercase().into_bytes())];
    for k in 0..3usize {
        let mut padded = vec![0xabu8; k];
        padded.extend_from_slice(b);
        let enc = B64.encode(&padded);
        let enc = enc.trim_end_matches('=');
        let head = [0usize, 2, 3][k];
        let tail = if padded.len() % 3 == 0 { 0 } else { 1 };
        forms.push((format!("base64+{k}"), enc[head..enc.len() - tail].as_bytes().to_vec()));
    }
    forms
}

fn hex_of(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn scan_dir(dir: &std::path::Path, marker: &str) -> Vec<String> {
    let mut hits = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        let data = std::fs::read(&path).unwrap();
        for (form, needle) in marker_forms(marker) {
            if data.windows(needle.len()).any(|w| w == needle.as_slice()) {
                hits.push(format!("{} as {form}", path.file_name().unwrap().to_string_lossy()));
            }
        }
    }
    hits
}

#[test]
fn the_database_files_hold_no_readable_content() {
    let dir = temp_dir("disk");
    let mk = Key::random().unwrap();
    let marker = format!("LEAK-HIST-{}", hex_of(&Key::random().unwrap().as_bytes()[..8]));
    let mut src = MemStore::new();
    let mut p = text(&format!("body {marker} end"));
    p.note = Some(format!("note {marker}"));
    p.tags = vec![format!("tag-{marker}")];
    put(&mut src, &mk, "a", 1, 10, &p);
    let mut h = HistoryStore::open(&dir, &mk).unwrap();
    h.refresh(&mut src, &mk).unwrap();
    assert_eq!(ids(&h, &marker, Filter::All), vec!["a"], "the content is searchable");
    // The -wal and -shm files exist while the database is open; they are scanned too
    let names: Vec<String> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    assert!(names.iter().any(|n| n.ends_with("-wal")) && names.iter().any(|n| n.ends_with("-shm")), "{names:?}");
    assert_eq!(scan_dir(&dir, &marker), Vec::<String>::new(), "while open");
    drop(h);
    assert_eq!(scan_dir(&dir, &marker), Vec::<String>::new(), "after close");
    let _ = std::fs::remove_dir_all(&dir);
}

// ---- 3. incremental refresh ------------------------------------------------------------------------------------

#[test]
fn refresh_follows_new_items_edits_and_tombstones() {
    let dir = temp_dir("incr");
    let mk = Key::random().unwrap();
    let mut src = MemStore::new();
    let mut h = HistoryStore::open(&dir, &mk).unwrap();
    put(&mut src, &mk, "a", 1, 10, &text("alpha"));
    put(&mut src, &mk, "b", 2, 20, &text("beta"));
    let r = h.refresh(&mut src, &mk).unwrap();
    assert_eq!((r.upserted, r.removed, r.indexed_seq), (2, 0, 2));
    assert_eq!(h.count().unwrap(), 2);
    // Favorite, pin, note and tags change
    let mut p = text("alpha");
    p.favorite = true;
    p.pinned = true;
    p.note = Some("remember".into());
    p.tags = vec!["work".into(), "urgent".into()];
    put(&mut src, &mk, "a", 3, 10, &p);
    h.refresh(&mut src, &mk).unwrap();
    let hit = &h.search("remember", Filter::All, 10, 0).unwrap()[0];
    assert_eq!((hit.item_id.as_str(), hit.favorite, hit.pinned, hit.note.as_str()), ("a", true, true, "remember"));
    assert_eq!(ids(&h, "urgent", Filter::All), vec!["a"], "tags are searchable");
    assert_eq!(ids(&h, "", Filter::Favorites), vec!["a"]);
    // The note is replaced: the old words must be gone from the index
    let mut p2 = p.clone();
    p2.note = Some("different".into());
    put(&mut src, &mk, "a", 4, 10, &p2);
    h.refresh(&mut src, &mk).unwrap();
    assert!(ids(&h, "remember", Filter::All).is_empty());
    assert_eq!(ids(&h, "different", Filter::All), vec!["a"]);
    // A tombstone removes the row and its index entries
    tombstone(&mut src, "b", 5);
    let r = h.refresh(&mut src, &mk).unwrap();
    assert_eq!(r.removed, 1);
    assert!(ids(&h, "beta", Filter::All).is_empty());
    assert_eq!(h.count().unwrap(), 1);
    assert_eq!(h.indexed_seq().unwrap(), 5);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn items_that_disappear_from_the_source_leave_the_history() {
    let dir = temp_dir("gone");
    let mk = Key::random().unwrap();
    let mut src = MemStore::new();
    let mut h = HistoryStore::open(&dir, &mk).unwrap();
    put(&mut src, &mk, "a", 1, 10, &text("alpha"));
    put(&mut src, &mk, "b", 2, 20, &text("bravo"));
    h.refresh(&mut src, &mk).unwrap();
    // A full pull removed "b" from the local copy without leaving a tombstone
    forget(&mut src, "b");
    let r = h.refresh(&mut src, &mk).unwrap();
    assert_eq!(r.removed, 1);
    assert!(ids(&h, "bravo", Filter::All).is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

// ---- 4. rebuild equals the incremental result ------------------------------------------------------------------

#[test]
fn rebuild_gives_exactly_the_result_of_an_unbroken_series_of_refreshes() {
    let (dir_a, dir_b) = (temp_dir("inc"), temp_dir("rebuild"));
    let mk = Key::random().unwrap();
    let mut src = MemStore::new();
    let mut inc = HistoryStore::open(&dir_a, &mk).unwrap();
    let mut seq = 0;
    for round in 0..6 {
        for n in 0..5 {
            seq += 1;
            let mut p = text(&format!("item {n} of round {round} {ZH2}"));
            p.favorite = (n + round) % 3 == 0;
            p.pinned = n == 2;
            p.note = (n % 2 == 0).then(|| format!("note {round}"));
            p.tags = vec![format!("t{n}")];
            put(&mut src, &mk, &format!("id-{n}-{}", round % 2), seq, 100 + seq, &p);
        }
        if round == 3 {
            seq += 1;
            tombstone(&mut src, "id-1-0", seq);
        }
        if round == 4 {
            forget(&mut src, "id-3-1");
        }
        inc.refresh(&mut src, &mk).unwrap();
    }
    let mut rebuilt = HistoryStore::open(&dir_b, &mk).unwrap();
    rebuilt.rebuild(&mut src, &mk).unwrap();
    assert_eq!(dump(&inc), dump(&rebuilt));
    for q in ["item", "round 3", "note 2", "t2", ZH2, "ITEM 1", "zzz"] {
        for f in [Filter::All, Filter::Favorites, Filter::Text] {
            assert_eq!(inc.search(q, f, 50, 0).unwrap(), rebuilt.search(q, f, 50, 0).unwrap(), "query {q:?}");
        }
    }
    // Rebuilding again changes nothing
    rebuilt.rebuild(&mut src, &mk).unwrap();
    assert_eq!(dump(&inc), dump(&rebuilt));
    let _ = (std::fs::remove_dir_all(&dir_a), std::fs::remove_dir_all(&dir_b));
}

// ---- 5. the refresh transaction --------------------------------------------------------------------------------

#[test]
fn a_refresh_that_fails_midway_changes_nothing() {
    let dir = temp_dir("tx");
    let mk = Key::random().unwrap();
    let mut src = MemStore::new();
    put(&mut src, &mk, "a-first", 1, 10, &text("kept"));
    let mut h = HistoryStore::open(&dir, &mk).unwrap();
    h.refresh(&mut src, &mk).unwrap();
    drop(h);
    // A trigger makes the storage refuse one particular row, after other rows have been written in the same transaction
    {
        let conn = Connection::open(dir.join("history.db")).unwrap();
        conn.execute_batch(&format!("PRAGMA key = \"x'{}'\";", hex_key(&derive_subkey(&mk, PURPOSE_HISTORY_DB)))).unwrap();
        conn.execute_batch("CREATE TRIGGER refuse BEFORE INSERT ON history WHEN NEW.item_id = 'z-boom' BEGIN SELECT RAISE(ABORT, 'refused'); END;").unwrap();
    }
    let mut h = HistoryStore::open(&dir, &mk).unwrap();
    let before = (dump(&h), h.indexed_seq().unwrap(), h.count().unwrap());
    put(&mut src, &mk, "b-second", 2, 20, &text("new one"));
    put(&mut src, &mk, "z-boom", 3, 30, &text("refused one"));
    tombstone(&mut src, "a-first", 4);
    assert!(h.refresh(&mut src, &mk).is_err());
    assert_eq!((dump(&h), h.indexed_seq().unwrap(), h.count().unwrap()), before, "rows, index position and count are unchanged");
    assert_eq!(ids(&h, "kept", Filter::All), vec!["a-first"], "the index is unchanged too");
    assert!(ids(&h, "new one", Filter::All).is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

// ---- 6. searching ----------------------------------------------------------------------------------------------

fn search_fixture(name: &str) -> (HistoryStore, PathBuf) {
    let dir = temp_dir(name);
    let mk = Key::random().unwrap();
    let mut src = MemStore::new();
    put(&mut src, &mk, "i1", 1, 100, &text("Hello World from OnePaste"));
    let mut p2 = text("hello again my friend");
    p2.pinned = true;
    put(&mut src, &mk, "i2", 2, 50, &p2);
    put(&mut src, &mk, "i3", 3, 300, &text(&format!("{ZH2}{ZH_TEST}")));
    let mut p4 = text(&format!("mix {ZH2} text ABC"));
    p4.favorite = true;
    p4.note = Some("a note about invoices".into());
    p4.tags = vec!["billing".into()];
    put(&mut src, &mk, "i4", 4, 400, &p4);
    let img = ItemPayload { kind: "image".into(), content: None, note: Some("holiday photo".into()), tags: vec!["travel".into()], ..Default::default() };
    put(&mut src, &mk, "i5", 5, 500, &img);
    let mut h = HistoryStore::open(&dir, &mk).unwrap();
    h.refresh(&mut src, &mk).unwrap();
    (h, dir)
}

#[test]
fn english_search_finds_words_prefixes_and_substrings_and_ignores_case() {
    let (h, dir) = search_fixture("en");
    assert_eq!(ids(&h, "world", Filter::All), vec!["i1"]);
    assert_eq!(ids(&h, "WORLD", Filter::All), vec!["i1"]);
    assert_eq!(ids(&h, "Hel", Filter::All), vec!["i2", "i1"], "a prefix; the pinned item first");
    assert_eq!(ids(&h, "orl", Filter::All), vec!["i1"], "a substring");
    assert_eq!(ids(&h, "invoices", Filter::All), vec!["i4"], "the note is searched");
    assert_eq!(ids(&h, "billing", Filter::All), vec!["i4"], "the tags are searched");
    assert_eq!(ids(&h, "hello friend", Filter::All), vec!["i2"], "several terms must all match");
    assert!(ids(&h, "nothing like this", Filter::All).is_empty());
    let hit = &h.search("world", Filter::All, 10, 0).unwrap()[0];
    assert_eq!((hit.match_start, hit.match_len), (Some(6), 5));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn chinese_search_works_for_two_characters_and_for_longer_words_and_mixed_text() {
    let (h, dir) = search_fixture("zh");
    // Two characters: below the trigram minimum, so the substring fallback answers
    assert_eq!(ids(&h, ZH2, Filter::All), vec!["i4", "i3"]);
    assert_eq!(ids(&h, ZH_TEST, Filter::All), vec!["i3"]);
    // Three characters or more: the full-text index
    assert_eq!(ids(&h, &format!("{ZH2}{}", &ZH_TEST[..3]), Filter::All), vec!["i3"]);
    assert_eq!(ids(&h, &format!("{ZH2}{ZH_TEST}"), Filter::All), vec!["i3"]);
    // Mixed Chinese and English
    assert_eq!(ids(&h, &format!("{ZH2} abc"), Filter::All), vec!["i4"]);
    assert_eq!(ids(&h, "mix", Filter::All), vec!["i4"]);
    // A short English term uses the same fallback and ignores case
    assert_eq!(ids(&h, "ab", Filter::All), vec!["i4"]);
    assert_eq!(ids(&h, "AB", Filter::All), vec!["i4"]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn filters_ordering_limit_and_offset() {
    let (h, dir) = search_fixture("filter");
    assert_eq!(ids(&h, "", Filter::All), vec!["i2", "i5", "i4", "i3", "i1"], "pinned first, then newest first; the empty query lists recent items");
    assert_eq!(ids(&h, "", Filter::Favorites), vec!["i4"]);
    assert_eq!(ids(&h, "", Filter::Text), vec!["i2", "i4", "i3", "i1"]);
    assert_eq!(ids(&h, "", Filter::Images), vec!["i5"]);
    assert_eq!(ids(&h, ZH2, Filter::Favorites), vec!["i4"]);
    let page = |limit, offset| h.search("", Filter::All, limit, offset).unwrap().into_iter().map(|x| x.item_id).collect::<Vec<_>>();
    assert_eq!(page(2, 0), vec!["i2", "i5"]);
    assert_eq!(page(2, 2), vec!["i4", "i3"]);
    assert_eq!(page(2, 4), vec!["i1"]);
    assert!(page(2, 9).is_empty());
    // Long text is cut to its first 200 characters in the result
    let mk = Key::random().unwrap();
    let dir2 = temp_dir("long");
    let mut src = MemStore::new();
    put(&mut src, &mk, "long", 1, 1, &text(&"x".repeat(500)));
    let mut h2 = HistoryStore::open(&dir2, &mk).unwrap();
    h2.refresh(&mut src, &mk).unwrap();
    assert_eq!(h2.search("", Filter::All, 10, 0).unwrap()[0].text.chars().count(), 200);
    let _ = (std::fs::remove_dir_all(&dir), std::fs::remove_dir_all(&dir2));
}

// ---- 7. special characters in the query ------------------------------------------------------------------------

#[test]
fn full_text_syntax_characters_in_the_query_are_plain_text() {
    let dir = temp_dir("syntax");
    let mk = Key::random().unwrap();
    let mut src = MemStore::new();
    put(&mut src, &mk, "q", 1, 1, &text(r#"say "quoted" and a*b and x-y and key:value and (paren) and NEAR and AND OR NOT"#));
    put(&mut src, &mk, "plain", 2, 2, &text("quoted and ab and xy and keyvalue and paren"));
    let mut h = HistoryStore::open(&dir, &mk).unwrap();
    h.refresh(&mut src, &mk).unwrap();
    for (query, want) in [
        (r#""quoted""#, vec!["q"]),
        ("\"", vec!["q"]),
        ("a*b", vec!["q"]),
        ("*", vec!["q"]),
        ("x-y", vec!["q"]),
        ("-", vec!["q"]),
        ("key:value", vec!["q"]),
        (":", vec!["q"]),
        ("(paren)", vec!["q"]),
        ("(", vec!["q"]),
        ("NEAR", vec!["q"]),
        ("AND OR NOT", vec!["q"]),
        // Two quote characters in a row do not occur in the text: no error, simply no hit
        ("\"\" \"", vec![]),
    ] {
        let got = h.search(query, Filter::All, 10, 0);
        assert!(got.is_ok(), "query {query:?} must not be an error: {got:?}");
        assert_eq!(got.unwrap().into_iter().map(|x| x.item_id).collect::<Vec<_>>(), want as Vec<&str>, "query {query:?}");
    }
    // Percent and underscore are not wildcards in the short fallback either
    put(&mut src, &mk, "pct", 3, 3, &text("100% sure and a_b"));
    h.refresh(&mut src, &mk).unwrap();
    assert_eq!(ids(&h, "%", Filter::All), vec!["pct"]);
    assert_eq!(ids(&h, "_", Filter::All), vec!["pct"]);
    assert!(ids(&h, "a%b", Filter::All).is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

// ---- 8. images ---------------------------------------------------------------------------------------------------

#[test]
fn an_image_item_keeps_only_its_metadata() {
    let dir = temp_dir("image");
    let mk = Key::random().unwrap();
    let mut src = MemStore::new();
    let bytes_marker = format!("IMAGE-BYTES-{}", hex_of(&Key::random().unwrap().as_bytes()[..8]));
    let mut img = ItemPayload { kind: "image".into(), content: Some(bytes_marker.clone()), note: Some("sunset".into()), tags: vec!["beach".into()], ..Default::default() };
    img.extra.insert("mime".into(), "image/png".into());
    img.extra.insert("size".into(), 4096.into());
    put(&mut src, &mk, "img", 1, 1, &img);
    let mut h = HistoryStore::open(&dir, &mk).unwrap();
    h.refresh(&mut src, &mk).unwrap();
    assert_eq!(ids(&h, "sunset", Filter::All), vec!["img"], "found by its note");
    assert_eq!(ids(&h, "beach", Filter::Images), vec!["img"], "found by its tag");
    let hit = &h.search("sunset", Filter::All, 5, 0).unwrap()[0];
    assert_eq!((hit.kind.as_str(), hit.text.as_str()), ("image", ""));
    assert!(ids(&h, &bytes_marker, Filter::All).is_empty(), "the image bytes are not in the history");
    drop(h);
    // Open the database with the right key and look at every column of the row
    let conn = Connection::open(dir.join("history.db")).unwrap();
    conn.execute_batch(&format!("PRAGMA key = \"x'{}'\";", hex_key(&derive_subkey(&mk, PURPOSE_HISTORY_DB)))).unwrap();
    let (kind, mime, size, body): (String, String, i64, String) = conn.query_row("SELECT kind, mime, size, text FROM history", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))).unwrap();
    assert_eq!((kind.as_str(), mime.as_str(), size, body.as_str()), ("image", "image/png", 4096, ""));
    let _ = std::fs::remove_dir_all(&dir);
}

// ---- 9. unreadable items -----------------------------------------------------------------------------------------

#[test]
fn an_item_that_cannot_be_decrypted_is_skipped_and_the_rest_still_applies() {
    let dir = temp_dir("skip");
    let mk = Key::random().unwrap();
    let other = Key::random().unwrap();
    let mut src = MemStore::new();
    put(&mut src, &mk, "good-1", 1, 1, &text("first fine"));
    put(&mut src, &other, "bad", 2, 2, &text("sealed under another key"));
    put(&mut src, &mk, "good-2", 3, 3, &text("second fine"));
    src.transact(&mut |tx| {
        tx.put_item(&ItemRow { id: "broken".into(), seq: 4, deleted: false, envelope_json: Some("not json".into()), mark: 0 })
    })
    .unwrap();
    let mut h = HistoryStore::open(&dir, &mk).unwrap();
    let r = h.refresh(&mut src, &mk).unwrap();
    assert_eq!(r.upserted, 2);
    assert_eq!(r.skipped, vec![("bad".to_string(), "decrypt_failed"), ("broken".to_string(), "malformed_envelope")]);
    assert_eq!(ids(&h, "fine", Filter::All).len(), 2);
    assert_eq!(r.indexed_seq, 4, "the skipped items do not hold the index back");
    let _ = std::fs::remove_dir_all(&dir);
}

// ---- 10. items not sent yet ----------------------------------------------------------------------------------------

#[test]
fn an_item_that_has_not_been_sent_yet_is_searchable_at_once() {
    let dir = temp_dir("unsent");
    let mk = Key::random().unwrap();
    let mut src = MemStore::new();
    put(&mut src, &mk, "synced", 7, 1, &text("already synced"));
    let mut h = HistoryStore::open(&dir, &mk).unwrap();
    h.refresh(&mut src, &mk).unwrap();
    // seq 0: created here, still waiting in the outbox
    put(&mut src, &mk, "fresh", 0, 2, &text("just copied"));
    h.refresh(&mut src, &mk).unwrap();
    assert_eq!(ids(&h, "just copied", Filter::All), vec!["fresh"]);
    // An edit that is only queued (seq unchanged, a change waiting) shows up too
    let mut edited = text("already synced");
    edited.note = Some("queued note".into());
    put(&mut src, &mk, "synced", 7, 1, &edited);
    queue(&mut src, "synced");
    h.refresh(&mut src, &mk).unwrap();
    assert_eq!(ids(&h, "queued note", Filter::All), vec!["synced"]);
    // The source type is any sync Store
    fn takes_source(_: &mut dyn ItemSource) {}
    takes_source(&mut src);
    let _ = std::fs::remove_dir_all(&dir);
}
