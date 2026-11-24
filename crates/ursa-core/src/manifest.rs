//! What a file is made of. Stored as its own encrypted object; the library index lists
//! manifests, and a manifest names its chunks and their keys. Only the account can open it.

use serde::{Deserialize, Serialize};

use crate::crypto::{AccountKey, open_manifest_bytes, seal_manifest_bytes};
use crate::id::hex32;
use crate::{Error, ObjectId, Result};

pub const VERSION: u32 = 1;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChunkRef {
    /// The sealed chunk's id (hash of its ciphertext).
    pub id: ObjectId,
    #[serde(with = "hex32")]
    pub key: [u8; 32],
    /// Plaintext length.
    pub len: u64,
    /// Where the coded shards are. Absent: the sealed chunk is stored whole, under `id`, on the
    /// home node (version 0.1 libraries).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shards: Option<ShardSet>,
}

/// A sealed chunk erasure-coded across nodes: any `code.k` of the shards rebuild it.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ShardSet {
    pub code: crate::coding::Code,
    /// Length of the sealed chunk (ciphertext) the shards rebuild.
    pub sealed_len: u64,
    pub shards: Vec<ShardRef>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ShardRef {
    /// Shard index, 0..n (0..k are data).
    pub i: u16,
    pub id: ObjectId,
    /// The node holding it: endpoint id, hex.
    pub node: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileManifest {
    pub v: u32,
    pub name: String,
    pub size: u64,
    pub mime: String,
    /// When the photo was taken or the file created, milliseconds since the Unix epoch.
    pub created_ms: i64,
    pub modified_ms: i64,
    /// When it was added to Ursa.
    pub added_ms: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    pub chunks: Vec<ChunkRef>,
    /// A small JPEG for the gallery, itself a sealed chunk.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thumb: Option<ChunkRef>,
}

/// Seals a manifest; returns its object id and bytes.
pub fn seal(account: &AccountKey, manifest: &FileManifest) -> (ObjectId, Vec<u8>) {
    let json = serde_json::to_vec(manifest).expect("manifest serializes");
    let sealed = seal_manifest_bytes(account, &json);
    (ObjectId::of(&sealed), sealed)
}

/// Checks the bytes are the object asked for, then opens them.
pub fn open(account: &AccountKey, id: &ObjectId, sealed: &[u8]) -> Result<FileManifest> {
    if !id.matches(sealed) {
        return Err(Error::WrongContent { expected: *id });
    }
    let json = open_manifest_bytes(account, sealed)?;
    let m: FileManifest = serde_json::from_slice(&json).map_err(|_| Error::Malformed("manifest"))?;
    if m.v != VERSION {
        return Err(Error::Version(m.v));
    }
    Ok(m)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::seal_chunk;

    fn sample(account: &AccountKey) -> FileManifest {
        let c = seal_chunk(account, b"pixels");
        FileManifest {
            v: VERSION,
            name: "IMG_0001.HEIC".into(),
            size: 6,
            mime: "image/heic".into(),
            created_ms: 1_700_000_000_000,
            modified_ms: 1_700_000_000_000,
            added_ms: 1_700_000_100_000,
            width: Some(4032),
            height: Some(3024),
            chunks: vec![ChunkRef { id: c.id, key: c.key, len: c.plain_len, shards: None }],
            thumb: None,
        }
    }

    #[test]
    fn round_trip() {
        let account = AccountKey::generate();
        let m = sample(&account);
        let (id, sealed) = seal(&account, &m);
        assert_eq!(open(&account, &id, &sealed).unwrap(), m);
    }

    #[test]
    fn another_account_cannot_open_it() {
        let account = AccountKey::generate();
        let (id, sealed) = seal(&account, &sample(&account));
        assert!(matches!(open(&AccountKey::generate(), &id, &sealed), Err(Error::Decrypt)));
    }

    #[test]
    fn sealing_twice_gives_different_objects() {
        let account = AccountKey::generate();
        let m = sample(&account);
        assert_ne!(seal(&account, &m).0, seal(&account, &m).0);
    }
}
