//! OnePaste 核心库。
//!
//! 边界：本 crate **不依赖**数据库驱动、网络栈、UI、平台 SDK；
//! 只负责密钥、加解密与条目信封，因此同一份代码可编译为桌面库与 WASM。
mod base32;
pub mod crypto;
pub mod envelope;
pub mod error;
pub mod id;
pub mod keys;
pub mod passphrase;
pub mod recovery;

pub use error::Error;
