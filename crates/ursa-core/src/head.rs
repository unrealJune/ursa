//! The account head: one small signed record that everything else hangs from.
//!
//! Anyone can check a head's signature and its sequence number (nodes do, to refuse forgeries
//! and stale writes); only the account can open its payload, which says where the account's
//! things are: its devices, home nodes, friends, and the current library index.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::account::{Account, AccountId, AccountPublic, DeviceCert, HybridSig, b64};
use crate::{Error, ObjectId, Result};

const HEAD_CONTEXT: &str = "ursa account head v1";
const HEAD_AAD: &[u8] = b"ursa account head payload v1";
const INDEX_AAD: &[u8] = b"ursa library index v1";

/// A node, as the account knows it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeRef {
    /// Endpoint id, hex.
    pub id: String,
    /// Endpoint ticket (with addresses), when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ticket: Option<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub label: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Friend {
    pub account: AccountId,
    #[serde(default)]
    pub name: String,
    pub nodes: Vec<NodeRef>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceEntry {
    pub device: String,
    pub name: String,
    pub platform: String,
    pub added_ms: i64,
}

/// What only the account can read.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountState {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub devices: Vec<DeviceEntry>,
    /// The account's own nodes: where it contributes space and keeps its head.
    #[serde(default)]
    pub home_nodes: Vec<NodeRef>,
    #[serde(default)]
    pub friends: Vec<Friend>,
    /// The current library index object, if anything has been stored.
    #[serde(default)]
    pub index: Option<ObjectId>,
}

impl AccountState {
    /// Every node the account can store on: its own, then its friends'.
    pub fn pool(&self) -> Vec<NodeRef> {
        let mut out: Vec<NodeRef> = self.home_nodes.clone();
        for f in &self.friends {
            for n in &f.nodes {
                if !out.iter().any(|o| o.id == n.id) {
                    out.push(n.clone());
                }
            }
        }
        out
    }
}

/// The signed, versioned head as nodes store it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Head {
    pub v: u32,
    pub account: AccountPublic,
    pub seq: u64,
    /// The sealed [`AccountState`], base64.
    #[serde(with = "b64")]
    pub payload: Vec<u8>,
    pub sig: HybridSig,
}

#[derive(Serialize)]
struct HeadBody<'a> {
    v: u32,
    account: &'a AccountId,
    seq: u64,
    payload: String,
}

impl Head {
    fn body(&self) -> Vec<u8> {
        serde_json::to_vec(&HeadBody {
            v: self.v,
            account: &self.account.id,
            seq: self.seq,
            payload: blake3::hash(&self.payload).to_hex().to_string(),
        })
        .expect("json")
    }

    pub fn seal(account: &Account, seq: u64, state: &AccountState) -> Self {
        let json = serde_json::to_vec(state).expect("json");
        let mut nonce = [0u8; 16];
        getrandom::fill(&mut nonce).expect("OS randomness");
        let mut material = account.head_key().to_vec();
        material.extend_from_slice(&nonce);
        let key = blake3::derive_key("ursa v1 head payload key", &material);
        let mut payload = nonce.to_vec();
        payload.extend_from_slice(&crate::crypto::seal_with(&key, &json, HEAD_AAD));
        let mut head = Self { v: 1, account: account.public().clone(), seq, payload, sig: HybridSig { ed25519: vec![], ml_dsa: vec![] } };
        head.sig = account.sign(HEAD_CONTEXT, &head.body());
        head
    }

    /// Anyone can check this; nodes do before storing a head.
    pub fn verify(&self) -> bool {
        self.v == 1 && self.account.verify(HEAD_CONTEXT, &self.body(), &self.sig)
    }

    pub fn open(&self, account: &Account) -> Result<AccountState> {
        if self.account.id != account.id() || !self.verify() {
            return Err(Error::Malformed("account head"));
        }
        if self.payload.len() < 16 {
            return Err(Error::Malformed("account head payload"));
        }
        let mut material = account.head_key().to_vec();
        material.extend_from_slice(&self.payload[..16]);
        let key = blake3::derive_key("ursa v1 head payload key", &material);
        let json = crate::crypto::open_with(&key, &self.payload[16..], HEAD_AAD)?;
        serde_json::from_slice(&json).map_err(|_| Error::Malformed("account state"))
    }

    pub fn to_json(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("json")
    }

    pub fn from_json(b: &[u8]) -> Result<Self> {
        serde_json::from_slice(b).map_err(|_| Error::Malformed("account head"))
    }
}

// ---------------------------------------------------------------------------------------------
// The library index: which manifests make up the library. Merges without losing anyone's
// change: union of entries, and a deletion wins over the same manifest's addition.

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexEntry {
    pub added_ms: i64,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub deleted: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LibraryIndex {
    pub entries: BTreeMap<ObjectId, IndexEntry>,
}

impl LibraryIndex {
    pub fn live(&self) -> impl Iterator<Item = &ObjectId> {
        self.entries.iter().filter(|(_, e)| !e.deleted).map(|(id, _)| id)
    }

    pub fn add(&mut self, id: ObjectId, added_ms: i64) {
        self.entries.entry(id).or_insert(IndexEntry { added_ms, deleted: false });
    }

    pub fn delete(&mut self, id: ObjectId) {
        self.entries.entry(id).or_insert(IndexEntry { added_ms: 0, deleted: false }).deleted = true;
    }

    pub fn merge(&mut self, other: &LibraryIndex) {
        for (id, e) in &other.entries {
            let mine = self.entries.entry(*id).or_insert_with(|| e.clone());
            mine.deleted |= e.deleted;
        }
    }

    pub fn seal(&self, account: &Account) -> (ObjectId, Vec<u8>) {
        let sealed = crate::crypto::seal_manifest_bytes_aad(account.root(), &serde_json::to_vec(self).expect("json"), INDEX_AAD);
        (ObjectId::of(&sealed), sealed)
    }

    pub fn open(account: &Account, id: &ObjectId, sealed: &[u8]) -> Result<Self> {
        if !id.matches(sealed) {
            return Err(Error::WrongContent { expected: *id });
        }
        let json = crate::crypto::open_manifest_bytes_aad(account.root(), sealed, INDEX_AAD)?;
        serde_json::from_slice(&json).map_err(|_| Error::Malformed("library index"))
    }
}

/// A device certificate is also what a device presents to nodes; re-exported for callers.
pub type Cert = DeviceCert;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heads_verify_open_and_reject_tampering() {
        let a = Account::generate();
        let state = AccountState { name: "June".into(), ..Default::default() };
        let head = Head::seal(&a, 7, &state);
        assert!(head.verify());
        assert_eq!(head.open(&a).unwrap(), state);
        let back = Head::from_json(&head.to_json()).unwrap();
        assert!(back.verify());
        let mut bumped = head.clone();
        bumped.seq = 8;
        assert!(!bumped.verify(), "a node must not accept a replayed head with a new seq");
        assert!(head.open(&Account::generate()).is_err());
    }

    #[test]
    fn index_merge_keeps_both_sides_and_deletes_win() {
        let (x, y, z) = (ObjectId::of(b"x"), ObjectId::of(b"y"), ObjectId::of(b"z"));
        let mut a = LibraryIndex::default();
        a.add(x, 1);
        a.add(y, 2);
        let mut b = LibraryIndex::default();
        b.add(x, 1);
        b.delete(y);
        b.add(z, 3);
        a.merge(&b);
        let live: Vec<_> = a.live().copied().collect();
        assert_eq!(live.len(), 2);
        assert!(live.contains(&x) && live.contains(&z));
    }

    #[test]
    fn index_seals_for_the_account_only() {
        let a = Account::generate();
        let mut idx = LibraryIndex::default();
        idx.add(ObjectId::of(b"m"), 5);
        let (id, sealed) = idx.seal(&a);
        assert_eq!(LibraryIndex::open(&a, &id, &sealed).unwrap(), idx);
        assert!(LibraryIndex::open(&Account::generate(), &id, &sealed).is_err());
    }
}
