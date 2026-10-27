//! OnePaste native client layer: everything the sync engine leaves to the platform. HTTPS with certificate pinning,
//! SQLite storage, the change-notification listener and the retry timers.

mod conn;
mod crash;
mod error;
mod harden;
mod join;
pub mod pin;
mod session;
mod sqlite;
mod transport;

pub use conn::ConnectionCode;
pub use crash::crash_point;
pub use error::ClientError;
pub use harden::disable_core_dumps;
pub use join::{join, prepare_new_vault, probe, JoinOptions, JoinResult, NewVault, VaultState};
pub use session::{db_path, now_ms, ApiClient, Info, RunEnd, RunReport, Session, DB_FILE};
pub use sqlite::SqliteStore;
pub use transport::{event_loop, parse_event_seqs, requests_started, Transport};
