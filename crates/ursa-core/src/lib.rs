//! Ursa's pure core: no I/O beyond `Read`, no network.
//!
//! - [`ObjectId`]: every stored object is named by the BLAKE3 hash of its (encrypted) bytes, so
//!   a node can verify what it is given and a client can verify what it gets back.
//! - [`crypto`]: the account key, per-chunk keys (derived from the content and an account
//!   secret, so identical chunks deduplicate within one account only), and AES-256-GCM.
//! - [`chunk`]: content-defined chunking (FastCDC), so an edit changes a few chunks.
//! - [`manifest`]: what a file is made of, sealed under a key only the account can derive.

pub mod chunk;
pub mod crypto;
pub mod manifest;

mod id;

pub use id::ObjectId;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("decryption failed: the data is corrupt, was tampered with, or the key is wrong")]
    Decrypt,
    #[error("object {expected} arrived with the wrong content")]
    WrongContent { expected: ObjectId },
    #[error("malformed {0}")]
    Malformed(&'static str),
    #[error("unsupported manifest version {0}")]
    Version(u32),
}

pub type Result<T> = std::result::Result<T, Error>;
