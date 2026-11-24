//! Where a node keeps things. Two backends behind one type: a directory, or an S3-compatible
//! bucket (SeaweedFS on Buttercup). Layout, identical in both:
//!
//! - `objects/<account>/<first two hex>/<id>`: one account's encrypted objects (shards,
//!   manifests, indexes), named by their BLAKE3 hash. One namespace per account keeps quotas and
//!   deletion simple, and means no account can probe for another's objects.
//! - `heads/<account>.json`: the account's signed head.
//! - `node/owners.json`, `node/grants.json`: who may use the node.
//!
//! Everything the node knows lives here, so with the S3 backend the node itself is stateless.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use bytes::Bytes;
use futures_util::TryStreamExt;
use object_store::path::Path as StorePath;
use object_store::{ObjectStore, ObjectStoreExt, PutPayload};
use ursa_core::ObjectId;
use ursa_core::account::AccountId;

#[derive(Clone)]
pub enum Store {
    Disk { root: PathBuf },
    S3 { store: Arc<dyn ObjectStore>, label: String },
}

pub struct S3Config {
    pub endpoint: String,
    pub bucket: String,
    pub region: String,
    /// Keys go under this path, so several nodes can share one bucket. Empty: the bucket root.
    pub prefix: String,
    pub access_key_id: String,
    pub secret_access_key: String,
}

fn object_key(account: &AccountId, id: &ObjectId) -> String {
    let hex = id.to_hex();
    format!("objects/{account}/{}/{hex}", &hex[..2])
}

fn head_key(account: &AccountId) -> String {
    format!("heads/{account}.json")
}

const OWNERS_KEY: &str = "node/owners.json";
const GRANTS_KEY: &str = "node/grants.json";

impl Store {
    pub fn disk(root: PathBuf) -> Self {
        Self::Disk { root }
    }

    pub fn s3(c: S3Config) -> Result<Self> {
        let store = object_store::aws::AmazonS3Builder::new()
            .with_endpoint(&c.endpoint)
            .with_bucket_name(&c.bucket)
            .with_region(&c.region)
            .with_access_key_id(&c.access_key_id)
            .with_secret_access_key(&c.secret_access_key)
            .with_allow_http(c.endpoint.starts_with("http://"))
            // SeaweedFS (and most self-hosted S3) wants path-style requests.
            .with_virtual_hosted_style_request(false)
            .build()
            .context("configure S3")?;
        let prefix = c.prefix.trim_matches('/');
        if prefix.is_empty() {
            return Ok(Self::S3 { store: Arc::new(store), label: format!("s3 {}/{}", c.endpoint, c.bucket) });
        }
        let store = object_store::prefix::PrefixStore::new(store, prefix);
        Ok(Self::S3 { store: Arc::new(store), label: format!("s3 {}/{}/{prefix}", c.endpoint, c.bucket) })
    }

    pub fn kind(&self) -> &'static str {
        match self {
            Self::Disk { .. } => "disk",
            Self::S3 { .. } => "s3",
        }
    }

    pub fn describe(&self) -> String {
        match self {
            Self::Disk { root } => format!("disk {}", root.display()),
            Self::S3 { label, .. } => label.clone(),
        }
    }

    async fn put_raw(&self, key: &str, bytes: Vec<u8>) -> Result<()> {
        match self {
            Self::Disk { root } => {
                let path = root.join(key);
                if let Some(dir) = path.parent() {
                    tokio::fs::create_dir_all(dir).await?;
                }
                // Write beside it and rename, so a reader never sees half an object.
                let tmp = path.with_extension(format!("tmp-{}-{}", std::process::id(), rand_suffix()));
                tokio::fs::write(&tmp, &bytes).await?;
                tokio::fs::rename(&tmp, &path).await?;
                Ok(())
            }
            Self::S3 { store, .. } => {
                store.put(&StorePath::from(key), PutPayload::from(Bytes::from(bytes))).await?;
                Ok(())
            }
        }
    }

    async fn get_raw(&self, key: &str) -> Result<Option<Vec<u8>>> {
        match self {
            Self::Disk { root } => match tokio::fs::read(root.join(key)).await {
                Ok(b) => Ok(Some(b)),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(e) => Err(e.into()),
            },
            Self::S3 { store, .. } => match store.get(&StorePath::from(key)).await {
                Ok(r) => Ok(Some(r.bytes().await?.to_vec())),
                Err(object_store::Error::NotFound { .. }) => Ok(None),
                Err(e) => Err(e.into()),
            },
        }
    }

    /// The object's size, if it exists.
    async fn size_raw(&self, key: &str) -> Result<Option<u64>> {
        match self {
            Self::Disk { root } => match tokio::fs::metadata(root.join(key)).await {
                Ok(m) => Ok(Some(m.len())),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(e) => Err(e.into()),
            },
            Self::S3 { store, .. } => match store.head(&StorePath::from(key)).await {
                Ok(m) => Ok(Some(m.size)),
                Err(object_store::Error::NotFound { .. }) => Ok(None),
                Err(e) => Err(e.into()),
            },
        }
    }

    async fn delete_raw(&self, key: &str) -> Result<()> {
        match self {
            Self::Disk { root } => match tokio::fs::remove_file(root.join(key)).await {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
                _ => Ok(()),
            },
            Self::S3 { store, .. } => match store.delete(&StorePath::from(key)).await {
                Err(object_store::Error::NotFound { .. }) | Ok(()) => Ok(()),
                Err(e) => Err(e.into()),
            },
        }
    }

    /// Every key under `prefix/`, with its size.
    async fn list_raw(&self, prefix: &str) -> Result<Vec<(String, u64)>> {
        match self {
            Self::Disk { root } => {
                let mut out = Vec::new();
                walk(&root.join(prefix), prefix, &mut out).await?;
                Ok(out)
            }
            Self::S3 { store, .. } => {
                let metas: Vec<_> = store.list(Some(&StorePath::from(prefix))).try_collect().await?;
                Ok(metas.into_iter().map(|m| (m.location.to_string(), m.size)).collect())
            }
        }
    }

    // -----------------------------------------------------------------------------------------

    /// Stores an object; returns how many bytes it added (0 if it was already there).
    pub async fn put(&self, account: &AccountId, id: &ObjectId, bytes: Vec<u8>) -> Result<u64> {
        let key = object_key(account, id);
        if self.size_raw(&key).await?.is_some() {
            return Ok(0);
        }
        let len = bytes.len() as u64;
        self.put_raw(&key, bytes).await?;
        Ok(len)
    }

    pub async fn get(&self, account: &AccountId, id: &ObjectId) -> Result<Option<Vec<u8>>> {
        self.get_raw(&object_key(account, id)).await
    }

    pub async fn has(&self, account: &AccountId, id: &ObjectId) -> Result<bool> {
        Ok(self.size_raw(&object_key(account, id)).await?.is_some())
    }

    /// Deletes an object; returns the bytes freed.
    pub async fn delete(&self, account: &AccountId, id: &ObjectId) -> Result<u64> {
        let key = object_key(account, id);
        let Some(size) = self.size_raw(&key).await? else { return Ok(0) };
        self.delete_raw(&key).await?;
        Ok(size)
    }

    pub async fn head(&self, account: &AccountId) -> Result<Option<Vec<u8>>> {
        self.get_raw(&head_key(account)).await
    }

    pub async fn set_head(&self, account: &AccountId, json: Vec<u8>) -> Result<()> {
        self.put_raw(&head_key(account), json).await
    }

    /// Bytes stored per account, from a listing: the node's accounting after a restart.
    pub async fn usage(&self) -> Result<HashMap<AccountId, u64>> {
        let mut out: HashMap<AccountId, u64> = HashMap::new();
        for (key, size) in self.list_raw("objects").await? {
            let mut parts = key.split('/');
            if let (Some("objects"), Some(acct)) = (parts.next(), parts.next())
                && let Ok(a) = acct.parse::<AccountId>()
            {
                *out.entry(a).or_default() += size;
            }
        }
        Ok(out)
    }

    pub async fn owners(&self) -> Result<Vec<AccountId>> {
        match self.get_raw(OWNERS_KEY).await? {
            Some(b) => Ok(serde_json::from_slice(&b).context("read owners")?),
            None => Ok(Vec::new()),
        }
    }

    pub async fn set_owners(&self, owners: &[AccountId]) -> Result<()> {
        self.put_raw(OWNERS_KEY, serde_json::to_vec_pretty(owners)?).await
    }

    pub async fn grants(&self) -> Result<BTreeMap<AccountId, u64>> {
        match self.get_raw(GRANTS_KEY).await? {
            Some(b) => Ok(serde_json::from_slice(&b).context("read grants")?),
            None => Ok(BTreeMap::new()),
        }
    }

    pub async fn set_grants(&self, grants: &BTreeMap<AccountId, u64>) -> Result<()> {
        self.put_raw(GRANTS_KEY, serde_json::to_vec_pretty(grants)?).await
    }
}

fn rand_suffix() -> String {
    let mut b = [0u8; 4];
    getrandom::fill(&mut b).expect("OS randomness");
    hex::encode(b)
}

/// Recursive listing of a directory, keys relative to the store root.
fn walk<'a>(dir: &'a Path, prefix: &'a str, out: &'a mut Vec<(String, u64)>) -> futures_util::future::BoxFuture<'a, Result<()>> {
    Box::pin(async move {
        let mut rd = match tokio::fs::read_dir(dir).await {
            Ok(d) => d,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e.into()),
        };
        while let Some(entry) = rd.next_entry().await? {
            let name = entry.file_name().to_string_lossy().into_owned();
            let key = format!("{prefix}/{name}");
            let meta = entry.metadata().await?;
            if meta.is_dir() {
                walk(&entry.path(), &key, out).await?;
            } else if !name.contains(".tmp-") {
                out.push((key, meta.len()));
            }
        }
        Ok(())
    })
}
