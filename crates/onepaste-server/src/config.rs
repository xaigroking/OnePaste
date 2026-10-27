//! Runtime configuration. `main.rs` parses command line arguments and environment variables; tests build it directly.

use std::net::SocketAddr;
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct Config {
    pub data_dir: PathBuf,
    pub listen: SocketAddr,
    /// The `u` of the connection code; when unset, the first non-loopback interface address plus the listen port is used.
    pub public_url: Option<String>,
    /// Validity of a registration credential, in seconds.
    pub reg_token_ttl_secs: i64,
    /// Tombstone retention, in seconds.
    pub tombstone_retention_secs: i64,
    /// Retention of idempotency records, in seconds.
    pub idempotency_ttl_secs: i64,
    /// Requests per IP per minute for register, invite and key parameter reads with a registration credential.
    pub register_per_min: u32,
    /// Failed authentications per IP per minute.
    pub auth_fail_per_min: u32,
}

impl Config {
    pub fn new(data_dir: PathBuf) -> Config {
        Config {
            data_dir,
            listen: SocketAddr::from(([0, 0, 0, 0], 8443)),
            public_url: None,
            reg_token_ttl_secs: 24 * 3600,
            tombstone_retention_secs: 90 * 24 * 3600,
            idempotency_ttl_secs: 24 * 3600,
            register_per_min: 10,
            auth_fail_per_min: 20,
        }
    }
}
