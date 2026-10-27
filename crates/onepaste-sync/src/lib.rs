//! OnePaste sync engine.
//!
//! The engine decides which request to send, how to treat the response and how local state changes. It performs no IO:
//! no network, no database, no clock and no random source of its own. Callers pass the time in, storage goes through the
//! [`Store`] trait, and randomness comes from `onepaste-core`.

mod engine;
mod error;
mod intent;
mod pull;
mod push;
mod record;
mod store;
mod types;

pub use engine::{Action, Engine, Event, ItemSummary, Status};
pub use error::Error;
pub use intent::{Edit, Intent};
pub use store::{MemStore, Store, StoreError, Tx};
pub use types::{HttpRequest, HttpResponse, ItemRow, Method, OpState, OutboxRow, PauseReason, SyncMeta};
