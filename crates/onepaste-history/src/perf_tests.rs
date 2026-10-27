//! Measures the cost of encryption: the same data and the same code on an encrypted and on a plain SQLite file.
//! Run with `cargo test -p onepaste-history --release perf -- --nocapture` to see the numbers.

use crate::store::SCHEMA;
use crate::{Filter, HistoryStore};
use onepaste_core::crypto::Key;
use onepaste_core::envelope::{seal_item, ItemPayload};
use onepaste_sync::{ItemRow, MemStore, Store};
use rusqlite::Connection;
use std::time::{Duration, Instant};

const ITEMS: usize = 10_000;
/// Every single search must finish within this, however it is stored.
const LIMIT: Duration = Duration::from_secs(1);

fn text_for(n: usize) -> String {
    // About 200 characters of English and Chinese mixed, different for every item
    let zh = ["\u{526a}\u{8d34}\u{677f}", "\u{4e2d}\u{6587}", "\u{5386}\u{53f2}\u{8bb0}\u{5f55}", "\u{540c}\u{6b65}\u{6d4b}\u{8bd5}", "\u{641c}\u{7d22}\u{529f}\u{80fd}"];
    let en = ["clipboard", "history", "invoice", "meeting notes", "password reset", "shopping list", "release plan"];
    let mut s = String::new();
    let mut i = n;
    while s.chars().count() < 200 {
        s.push_str(en[i % en.len()]);
        s.push(' ');
        s.push_str(zh[i % zh.len()]);
        s.push_str(&format!(" #{} ", i * 7 + n));
        i = i.wrapping_mul(31).wrapping_add(17);
    }
    s
}

fn source(mk: &Key) -> MemStore {
    let mut src = MemStore::new();
    let mut rows = Vec::new();
    for n in 0..ITEMS {
        let p = ItemPayload { kind: "text".into(), content: Some(text_for(n)), ..Default::default() };
        let env = seal_item(mk, &format!("item-{n:05}"), n as i64, n as i64, None, &p).unwrap();
        rows.push(ItemRow { id: format!("item-{n:05}"), seq: n as i64 + 1, deleted: false, envelope_json: Some(env.to_json().unwrap()), mark: 0 });
    }
    src.transact(&mut |tx| {
        for r in &rows {
            tx.put_item(r)?;
        }
        Ok(())
    })
    .unwrap();
    src
}

fn timed<R>(f: impl FnOnce() -> R) -> (R, Duration) {
    let t = Instant::now();
    let r = f();
    (r, t.elapsed())
}

struct Numbers {
    rebuild: Duration,
    refresh_one: Duration,
    word: Duration,
    two_chars: Duration,
    recent: Duration,
    word_no_hit: Duration,
    two_chars_no_hit: Duration,
}

fn measure(h: &mut HistoryStore, src: &mut MemStore, mk: &Key) -> Numbers {
    let (_, rebuild) = timed(|| h.rebuild(src, mk).unwrap());
    assert_eq!(h.count().unwrap(), ITEMS as i64);
    // One changed item
    let p = ItemPayload { kind: "text".into(), content: Some("changed item text".into()), favorite: true, ..Default::default() };
    let env = seal_item(mk, "item-00042", 42, 42, None, &p).unwrap();
    let row = ItemRow { id: "item-00042".into(), seq: ITEMS as i64 + 1, deleted: false, envelope_json: Some(env.to_json().unwrap()), mark: 0 };
    src.transact(&mut |tx| tx.put_item(&row)).unwrap();
    let (_, refresh_one) = timed(|| h.refresh(src, mk).unwrap());
    // Searches: the best of three runs, to keep a cold cache from deciding the number
    let best = |query: &str, filter: Filter| (0..3).map(|_| timed(|| h.search(query, filter, 50, 0).unwrap()).1).min().unwrap();
    let word = best("meeting notes", Filter::All);
    let two_chars = best("\u{4e2d}\u{6587}", Filter::All);
    let recent = best("", Filter::All);
    // Terms that occur nowhere: the whole table has to be looked at
    let word_no_hit = best("zzqxv", Filter::All);
    let two_chars_no_hit = best("\u{9f98}\u{9f99}", Filter::All);
    assert!(h.search("\u{9f98}\u{9f99}", Filter::All, 50, 0).unwrap().is_empty());
    assert!(!h.search("meeting notes", Filter::All, 50, 0).unwrap().is_empty());
    assert!(!h.search("\u{4e2d}\u{6587}", Filter::All, 50, 0).unwrap().is_empty());
    for (name, d) in [("word", word), ("two chars", two_chars), ("recent", recent), ("word, no hit", word_no_hit), ("two chars, no hit", two_chars_no_hit)] {
        assert!(d < LIMIT, "the search '{name}' took {d:?}");
    }
    Numbers { rebuild, refresh_one, word, two_chars, recent, word_no_hit, two_chars_no_hit }
}

#[test]
fn perf_encrypted_against_plain_sqlite_with_ten_thousand_items() {
    let mk = Key::random().unwrap();
    let mut src = source(&mk);
    let base = std::env::temp_dir().join(format!("onepaste-history-perf-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let mut enc = HistoryStore::open(&base.join("enc"), &mk).unwrap();
    let e = measure(&mut enc, &mut src, &mk);

    std::fs::create_dir_all(base.join("plain")).unwrap();
    let conn = Connection::open(base.join("plain").join("history.db")).unwrap();
    conn.pragma_update(None, "journal_mode", "WAL").unwrap();
    conn.pragma_update(None, "synchronous", "NORMAL").unwrap();
    conn.execute_batch(SCHEMA).unwrap();
    let mut plain = HistoryStore { conn };
    let p = measure(&mut plain, &mut src, &mk);

    let ms = |d: Duration| format!("{:>9.2} ms", d.as_secs_f64() * 1000.0);
    let ratio = |a: Duration, b: Duration| format!("{:>6.2}x", a.as_secs_f64() / b.as_secs_f64().max(1e-9));
    eprintln!("\n10000 items of about 200 characters, mixed Chinese and English");
    eprintln!("{:<42} {:>12} {:>12} {:>8}", "operation", "encrypted", "plain", "ratio");
    for (name, a, b) in [
        ("full rebuild (decrypt + index 10000 items)", e.rebuild, p.rebuild),
        ("refresh after 1 changed item", e.refresh_one, p.refresh_one),
        ("search, 3+ character term (full-text)", e.word, p.word),
        ("search, 2-character Chinese term (substring)", e.two_chars, p.two_chars),
        ("empty query, 50 most recent", e.recent, p.recent),
        ("search, 3+ character term, no hit", e.word_no_hit, p.word_no_hit),
        ("search, 2-character term, no hit (full scan)", e.two_chars_no_hit, p.two_chars_no_hit),
    ] {
        eprintln!("{:<42} {} {} {}", name, ms(a), ms(b), ratio(a, b));
    }
    let size = |d: &str| std::fs::metadata(base.join(d).join("history.db")).map(|m| m.len()).unwrap_or(0);
    eprintln!("{:<42} {:>9} KiB {:>9} KiB", "database file size", size("enc") / 1024, size("plain") / 1024);
    drop((enc, plain));
    let _ = std::fs::remove_dir_all(&base);
}
