//! Keys and authenticated encryption.
//!
//! Everything stored is AES-256-GCM (post-quantum as it stands: a 256-bit key keeps ~128 bits
//! against Grover). Every key encrypts exactly one plaintext, so the nonce is fixed at zero:
//!
//! - A **chunk** key is a keyed BLAKE3 hash of the chunk's plaintext under the account's dedup
//!   secret. The same chunk stored twice by one account is the same object, but nobody without
//!   the account key can tell which plaintext a chunk holds, or match chunks across accounts.
//! - A **manifest** key is derived from the account key and a random 16-byte nonce stored in
//!   front of the ciphertext.
//!
//! ring's AES-GCM for now (S3 chose aws-lc-rs for bulk speed; ring is within 0-35% of it and
//! builds everywhere without CMake or NASM). The swap is local to this module.

use ring::aead::{AES_256_GCM, Aad, LessSafeKey, Nonce, UnboundKey};

use crate::{Error, ObjectId, Result};

const CHUNK_AAD: &[u8] = b"ursa chunk v1";
const MANIFEST_AAD: &[u8] = b"ursa manifest v1";
pub const TAG_LEN: usize = 16;

/// The account's root secret. Never leaves the account's own devices.
#[derive(Clone)]
pub struct AccountKey([u8; 32]);

impl AccountKey {
    pub fn generate() -> Self {
        let mut k = [0u8; 32];
        getrandom::fill(&mut k).expect("OS randomness");
        Self(k)
    }

    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub fn from_hex(s: &str) -> Result<Self> {
        let mut k = [0u8; 32];
        hex::decode_to_slice(s.trim(), &mut k).map_err(|_| Error::Malformed("account key"))?;
        Ok(Self(k))
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    fn dedup_secret(&self) -> [u8; 32] {
        blake3::derive_key("ursa v1 chunk dedup secret", &self.0)
    }

    pub(crate) fn manifest_key(&self, nonce: &[u8; 16]) -> [u8; 32] {
        let mut material = [0u8; 48];
        material[..32].copy_from_slice(&self.0);
        material[32..].copy_from_slice(nonce);
        blake3::derive_key("ursa v1 manifest key", &material)
    }
}

impl std::fmt::Debug for AccountKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AccountKey(..)")
    }
}

/// A chunk ready to store: its id (hash of the ciphertext), its key and the ciphertext.
pub struct SealedChunk {
    pub id: ObjectId,
    pub key: [u8; 32],
    pub ciphertext: Vec<u8>,
    pub plain_len: u64,
}

pub fn seal_chunk(account: &AccountKey, plaintext: &[u8]) -> SealedChunk {
    let key = *blake3::keyed_hash(&account.dedup_secret(), plaintext).as_bytes();
    let ciphertext = seal(&key, plaintext, CHUNK_AAD);
    SealedChunk { id: ObjectId::of(&ciphertext), key, ciphertext, plain_len: plaintext.len() as u64 }
}

/// Checks the bytes are the object asked for, then decrypts them.
pub fn open_chunk(id: &ObjectId, key: &[u8; 32], ciphertext: &[u8]) -> Result<Vec<u8>> {
    if !id.matches(ciphertext) {
        return Err(Error::WrongContent { expected: *id });
    }
    open(key, ciphertext, CHUNK_AAD)
}

pub(crate) fn seal_manifest_bytes(account: &AccountKey, plaintext: &[u8]) -> Vec<u8> {
    seal_manifest_bytes_aad(account, plaintext, MANIFEST_AAD)
}

/// Seals under a fresh key derived from the account key and a random nonce stored in front.
pub(crate) fn seal_manifest_bytes_aad(account: &AccountKey, plaintext: &[u8], aad: &[u8]) -> Vec<u8> {
    let mut nonce = [0u8; 16];
    getrandom::fill(&mut nonce).expect("OS randomness");
    let key = account.manifest_key(&nonce);
    let mut out = nonce.to_vec();
    out.extend_from_slice(&seal(&key, plaintext, aad));
    out
}

pub(crate) fn open_manifest_bytes(account: &AccountKey, sealed: &[u8]) -> Result<Vec<u8>> {
    open_manifest_bytes_aad(account, sealed, MANIFEST_AAD)
}

pub(crate) fn open_manifest_bytes_aad(account: &AccountKey, sealed: &[u8], aad: &[u8]) -> Result<Vec<u8>> {
    if sealed.len() < 16 + TAG_LEN {
        return Err(Error::Malformed("sealed object"));
    }
    let nonce: [u8; 16] = sealed[..16].try_into().unwrap();
    open(&account.manifest_key(&nonce), &sealed[16..], aad)
}

/// AES-256-GCM under a key used for exactly one plaintext (fixed nonce).
pub(crate) fn seal_with(key: &[u8; 32], plaintext: &[u8], aad: &[u8]) -> Vec<u8> {
    seal(key, plaintext, aad)
}

pub(crate) fn open_with(key: &[u8; 32], ciphertext: &[u8], aad: &[u8]) -> Result<Vec<u8>> {
    open(key, ciphertext, aad)
}

fn cipher(key: &[u8; 32]) -> LessSafeKey {
    LessSafeKey::new(UnboundKey::new(&AES_256_GCM, key).expect("32-byte key"))
}

/// The zero nonce is safe only because every key here encrypts exactly one plaintext.
fn seal(key: &[u8; 32], plaintext: &[u8], aad: &[u8]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(plaintext.len() + TAG_LEN);
    buf.extend_from_slice(plaintext);
    cipher(key)
        .seal_in_place_append_tag(Nonce::assume_unique_for_key([0u8; 12]), Aad::from(aad), &mut buf)
        .expect("seal");
    buf
}

fn open(key: &[u8; 32], ciphertext: &[u8], aad: &[u8]) -> Result<Vec<u8>> {
    let mut buf = ciphertext.to_vec();
    let len = cipher(key)
        .open_in_place(Nonce::assume_unique_for_key([0u8; 12]), Aad::from(aad), &mut buf)
        .map_err(|_| Error::Decrypt)?
        .len();
    buf.truncate(len);
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunk_round_trip() {
        let account = AccountKey::generate();
        let sealed = seal_chunk(&account, b"hello ursa");
        assert_eq!(open_chunk(&sealed.id, &sealed.key, &sealed.ciphertext).unwrap(), b"hello ursa");
    }

    #[test]
    fn same_chunk_same_account_deduplicates() {
        let account = AccountKey::generate();
        assert_eq!(seal_chunk(&account, b"x").id, seal_chunk(&account, b"x").id);
    }

    #[test]
    fn same_chunk_other_account_does_not() {
        let a = seal_chunk(&AccountKey::generate(), b"x");
        let b = seal_chunk(&AccountKey::generate(), b"x");
        assert_ne!(a.id, b.id);
        assert_ne!(a.key, b.key);
    }

    #[test]
    fn tampering_is_caught() {
        let account = AccountKey::generate();
        let mut sealed = seal_chunk(&account, b"hello ursa");
        sealed.ciphertext[0] ^= 1;
        assert!(matches!(
            open_chunk(&sealed.id, &sealed.key, &sealed.ciphertext),
            Err(Error::WrongContent { .. })
        ));
        // Right id for the tampered bytes, so only the AEAD can catch it.
        let id = ObjectId::of(&sealed.ciphertext);
        assert!(matches!(open_chunk(&id, &sealed.key, &sealed.ciphertext), Err(Error::Decrypt)));
    }
}
