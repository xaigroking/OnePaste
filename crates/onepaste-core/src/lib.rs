//! OnePaste core library.
//!
//! Boundary: this crate does **not** depend on database drivers, network stacks, UI or platform SDKs;
//! it only handles keys, encryption and item envelopes, so the same code compiles for desktop and WASM.
mod base32;
pub mod crypto;
pub mod envelope;
pub mod error;
pub mod id;
pub mod keys;
pub mod passphrase;
pub mod pin;
pub mod recovery;

pub use error::Error;
