//! The encrypted database file and its schema.

use onepaste_core::crypto::Key;
use onepaste_core::keys::{derive_subkey, PURPOSE_HISTORY_DB};
use rusqlite::Connection;
use std::path::Path;
use thiserror::Error;
use zeroize::Zeroize;

pub const FILE_NAME: &str = "history.db";

#[derive(Debug, Error)]
pub enum HistoryError {
    /// The key does not open this database (or the file is not a database).
    #[error("the history database cannot be opened with this key")]
    WrongKey,
    #[error("the key is not valid")]
    InvalidKey,
    #[error("history storage failed: {0}")]
    Storage(String),
}

impl From<rusqlite::Error> for HistoryError {
    fn from(e: rusqlite::Error) -> Self {
        HistoryError::Storage(e.to_string())
    }
}

impl From<std::io::Error> for HistoryError {
    fn from(e: std::io::Error) -> Self {
        HistoryError::Storage(e.to_string())
    }
}

/// Table `history` holds one row per item; `history_fts` is the full-text index over its text, note and tags, kept in
/// step by triggers. The trigram tokenizer finds any substring of three or more characters and ignores ASCII case.
pub(crate) const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS state (key TEXT PRIMARY KEY, value INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS history (
  item_id TEXT PRIMARY KEY,
  seq INTEGER NOT NULL,
  kind TEXT NOT NULL,
  text TEXT NOT NULL DEFAULT '',
  mime TEXT NOT NULL DEFAULT '',
  size INTEGER NOT NULL DEFAULT 0,
  favorite INTEGER NOT NULL DEFAULT 0,
  pinned INTEGER NOT NULL DEFAULT 0,
  note TEXT NOT NULL DEFAULT '',
  tags TEXT NOT NULL DEFAULT '',
  source_device TEXT NOT NULL DEFAULT '',
  created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS history_order ON history (pinned DESC, created_at DESC);
CREATE VIRTUAL TABLE IF NOT EXISTS history_fts USING fts5(text, note, tags, content='history', content_rowid='rowid', tokenize='trigram');
CREATE TRIGGER IF NOT EXISTS history_ai AFTER INSERT ON history BEGIN
  INSERT INTO history_fts(rowid, text, note, tags) VALUES (new.rowid, new.text, new.note, new.tags);
END;
CREATE TRIGGER IF NOT EXISTS history_ad AFTER DELETE ON history BEGIN
  INSERT INTO history_fts(history_fts, rowid, text, note, tags) VALUES ('delete', old.rowid, old.text, old.note, old.tags);
END;
CREATE TRIGGER IF NOT EXISTS history_au AFTER UPDATE ON history BEGIN
  INSERT INTO history_fts(history_fts, rowid, text, note, tags) VALUES ('delete', old.rowid, old.text, old.note, old.tags);
  INSERT INTO history_fts(rowid, text, note, tags) VALUES (new.rowid, new.text, new.note, new.tags);
END;
";

pub struct HistoryStore {
    pub(crate) conn: Connection,
}

/// The SQLCipher raw key statement: the 32 key bytes in hex, which skips the passphrase derivation inside SQLCipher.
fn key_statement(key: &Key) -> String {
    let mut hex: String = key.as_bytes().iter().map(|b| format!("{b:02x}")).collect();
    let stmt = format!("PRAGMA key = \"x'{hex}'\";");
    hex.zeroize();
    stmt
}

impl HistoryStore {
    /// Opens (or creates) `<dir>/history.db`, encrypted with a key derived from the master key. The directory gets
    /// mode 0700 and the database files 0600 on Unix. A wrong key is `WrongKey`.
    pub fn open(dir: &Path, mk: &Key) -> Result<HistoryStore, HistoryError> {
        if mk.as_bytes().iter().all(|b| *b == 0) {
            return Err(HistoryError::InvalidKey);
        }
        std::fs::create_dir_all(dir)?;
        let path = dir.join(FILE_NAME);
        #[cfg(unix)]
        {
            use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
            std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
            // Create the file with the right mode first; the -wal and -shm files copy it
            std::fs::OpenOptions::new().write(true).create(true).truncate(false).mode(0o600).open(&path)?;
        }
        let conn = Connection::open(&path)?;
        let mut stmt = key_statement(&derive_subkey(mk, PURPOSE_HISTORY_DB));
        let keyed = conn.execute_batch(&stmt);
        stmt.zeroize();
        keyed?;
        // A wrong key shows up on the first read
        conn.query_row("SELECT count(*) FROM sqlite_master", [], |r| r.get::<_, i64>(0)).map_err(|_| HistoryError::WrongKey)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        // An empty file has no header yet and would open with any key, so the schema is written right away
        conn.execute_batch(SCHEMA)?;
        Ok(HistoryStore { conn })
    }

    /// The SQLCipher version, which proves the library in use is SQLCipher and not plain SQLite.
    pub fn cipher_version(&self) -> Result<String, HistoryError> {
        Ok(self.conn.query_row("PRAGMA cipher_version", [], |r| r.get(0))?)
    }

    /// The highest item sequence number that has been processed.
    pub fn indexed_seq(&self) -> Result<i64, HistoryError> {
        let v: Option<i64> = self
            .conn
            .query_row("SELECT value FROM state WHERE key = 'indexed_seq'", [], |r| r.get(0))
            .map(Some)
            .or_else(|e| if matches!(e, rusqlite::Error::QueryReturnedNoRows) { Ok(None) } else { Err(e) })?;
        Ok(v.unwrap_or(0))
    }

    /// Number of items in the history.
    pub fn count(&self) -> Result<i64, HistoryError> {
        Ok(self.conn.query_row("SELECT count(*) FROM history", [], |r| r.get(0))?)
    }
}
