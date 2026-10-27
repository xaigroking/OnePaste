//! The encrypted database file.

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

const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS state (key TEXT PRIMARY KEY, value INTEGER NOT NULL);";

pub struct HistoryStore {
    conn: Connection,
}

/// The SQLCipher raw key statement: the 32 key bytes in hex, which skips the passphrase derivation inside SQLCipher.
fn key_statement(key: &Key) -> String {
    let mut hex: String = key.as_bytes().iter().map(|b| format!("{b:02x}")).collect();
    let stmt = format!("PRAGMA key = \"x'{hex}'\";");
    hex.zeroize();
    stmt
}

impl HistoryStore {
    /// Opens (or creates) `<dir>/history.db`, encrypted with a key derived from the master key.
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
        // An empty file has no header yet and would open with any key, so the schema is written right away
        conn.execute_batch(SCHEMA).map_err(|_| HistoryError::WrongKey)?;
        Ok(HistoryStore { conn })
    }

    /// The SQLCipher version, which proves the library in use is SQLCipher and not plain SQLite.
    pub fn cipher_version(&self) -> Result<String, HistoryError> {
        Ok(self.conn.query_row("PRAGMA cipher_version", [], |r| r.get(0))?)
    }
}
