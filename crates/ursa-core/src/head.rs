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
    pub index: Option<crate::manifest::StoredRef>,
    /// The code the account chose for its files (any `k` of `n` pieces rebuild one). `None`:
    /// picked from the pool's size.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protection: Option<crate::coding::Code>,
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
    /// Where the manifest's shards are.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<crate::manifest::ShardSet>,
    /// The manifest object, when it is no longer the one the file was added as. The key stays
    /// the file's id for good; a repair that moves chunks writes a new manifest and points here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manifest: Option<ObjectId>,
    /// Bumped by every re-placement, so the newest placement wins a merge.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub rev: u32,
    /// The folder the file is in; `None` (and every file from before folders): the root.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dir: Option<crate::folders::FolderId>,
    /// The file's name once renamed; until then, its manifest's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// When `dir` or `name` last changed, ms; the newest wins a merge.
    #[serde(default, skip_serializing_if = "is_zero_i64")]
    pub placed_ms: i64,
    /// Fields from a newer version, kept so writing the index here does not drop them.
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

fn is_zero(v: &u32) -> bool {
    *v == 0
}

fn is_zero_i64(v: &i64) -> bool {
    *v == 0
}

impl IndexEntry {
    pub fn new(added_ms: i64, at: Option<crate::manifest::ShardSet>) -> Self {
        Self { added_ms, deleted: false, at, manifest: None, rev: 0, dir: None, name: None, placed_ms: 0, extra: BTreeMap::new() }
    }

    /// The object holding this file's manifest.
    pub fn manifest_id(&self, file: &ObjectId) -> ObjectId {
        self.manifest.unwrap_or(*file)
    }

    /// Records a new placement of the manifest (after a repair).
    pub fn replace(&mut self, file: &ObjectId, manifest: ObjectId, at: crate::manifest::ShardSet) {
        self.manifest = (manifest != *file).then_some(manifest);
        self.at = Some(at);
        self.rev += 1;
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LibraryIndex {
    pub entries: BTreeMap<ObjectId, IndexEntry>,
    /// The folder tree; see [`crate::folders`].
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub folders: BTreeMap<crate::folders::FolderId, crate::folders::Folder>,
    /// Fields from a newer version, kept so writing the index here does not drop them.
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

impl LibraryIndex {
    pub fn live(&self) -> impl Iterator<Item = &ObjectId> {
        self.entries.iter().filter(|(_, e)| !e.deleted).map(|(id, _)| id)
    }

    pub fn add(&mut self, id: ObjectId, added_ms: i64, at: Option<crate::manifest::ShardSet>) {
        let e = self.entries.entry(id).or_insert(IndexEntry::new(added_ms, None));
        if at.is_some() {
            e.at = at;
        }
    }

    pub fn delete(&mut self, id: ObjectId) {
        self.entries.entry(id).or_insert(IndexEntry::new(0, None)).deleted = true;
    }

    /// Union of both sides; a deletion wins; the newest placement wins, and a known location
    /// wins over none. Folders and a file's folder and name: the newest change wins.
    pub fn merge(&mut self, other: &LibraryIndex) {
        for (id, e) in &other.entries {
            let mine = self.entries.entry(*id).or_insert_with(|| e.clone());
            mine.deleted |= e.deleted;
            if e.rev > mine.rev || (mine.at.is_none() && e.rev == mine.rev) {
                mine.at = e.at.clone();
                mine.manifest = e.manifest;
                mine.rev = e.rev;
            }
            if (e.placed_ms, &e.dir, &e.name) > (mine.placed_ms, &mine.dir, &mine.name) {
                mine.dir = e.dir.clone();
                mine.name = e.name.clone();
                mine.placed_ms = e.placed_ms;
            }
        }
        for (id, f) in &other.folders {
            self.folders.entry(id.clone()).or_insert_with(|| f.clone()).merge(f);
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
        a.add(x, 1, None);
        a.add(y, 2, None);
        let mut b = LibraryIndex::default();
        b.add(x, 1, None);
        b.delete(y);
        b.add(z, 3, None);
        a.merge(&b);
        let live: Vec<_> = a.live().copied().collect();
        assert_eq!(live.len(), 2);
        assert!(live.contains(&x) && live.contains(&z));
    }

    #[test]
    fn a_repaired_file_keeps_its_id_and_the_newest_placement_wins() {
        let file = ObjectId::of(b"file");
        let set = |len| crate::manifest::ShardSet { code: crate::coding::Code::new(1, 3).unwrap(), sealed_len: len, shards: vec![] };
        let mut stale = LibraryIndex::default();
        stale.add(file, 1, Some(set(10)));
        let mut repaired = stale.clone();
        let moved = ObjectId::of(b"new manifest");
        repaired.entries.get_mut(&file).unwrap().replace(&file, moved, set(11));
        // Either order of merging ends on the repaired placement, under the same file id.
        for (mut a, b) in [(stale.clone(), repaired.clone()), (repaired.clone(), stale.clone())] {
            a.merge(&b);
            let e = &a.entries[&file];
            assert_eq!((e.manifest_id(&file), e.at.as_ref().unwrap().sealed_len, e.rev), (moved, 11, 1));
        }
    }

    #[test]
    fn index_seals_for_the_account_only() {
        let a = Account::generate();
        let mut idx = LibraryIndex::default();
        idx.add(ObjectId::of(b"m"), 5, None);
        let (id, sealed) = idx.seal(&a);
        assert_eq!(LibraryIndex::open(&a, &id, &sealed).unwrap(), idx);
        assert!(LibraryIndex::open(&Account::generate(), &id, &sealed).is_err());
    }
}
