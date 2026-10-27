//! OnePaste local encrypted history database.
//!
//! The history is a **cache**, never a source of truth and never synced: it is built from the local items (decrypted
//! with the master key), lives in one SQLCipher file whose key is derived from the master key for this purpose alone,
//! and can be deleted and rebuilt at any time. Search happens here, locally; nothing is uploaded.

mod refresh;
mod search;
mod store;

#[cfg(test)]
mod perf_tests;

pub use refresh::{ItemSource, RefreshReport, SourceItems};
pub use search::{Filter, Hit};
pub use store::{HistoryError, HistoryStore, FILE_NAME};
