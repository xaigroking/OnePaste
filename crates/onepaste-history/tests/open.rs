use onepaste_core::crypto::Key;
use onepaste_history::{HistoryError, HistoryStore};

fn temp_dir(name: &str) -> std::path::PathBuf {
    let p = std::env::temp_dir().join(format!("onepaste-history-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    p
}

#[test]
fn the_library_in_use_is_sqlcipher_and_the_wrong_key_does_not_open_the_file() {
    let dir = temp_dir("open");
    let mk = Key::random().unwrap();
    let store = HistoryStore::open(&dir, &mk).unwrap();
    assert!(!store.cipher_version().unwrap().is_empty(), "not a SQLCipher build");
    drop(store);
    assert!(HistoryStore::open(&dir, &mk).is_ok());
    assert!(matches!(HistoryStore::open(&dir, &Key::random().unwrap()), Err(HistoryError::WrongKey)));
    let _ = std::fs::remove_dir_all(&dir);
}
