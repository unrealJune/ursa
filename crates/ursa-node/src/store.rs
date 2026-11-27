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
const UPTIME_KEY: &str = "node/uptime.json";

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
        Ok(self.stat_raw(key).await?.map(|(size, _)| size))
    }

    /// The object's size and when it was last written (ms since the epoch), if it exists.
    async fn stat_raw(&self, key: &str) -> Result<Option<(u64, i64)>> {
        match self {
            Self::Disk { root } => match tokio::fs::metadata(root.join(key)).await {
                Ok(m) => Ok(Some((m.len(), m.modified().map(ms_of).unwrap_or(0)))),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(e) => Err(e.into()),
            },
            Self::S3 { store, .. } => match store.head(&StorePath::from(key)).await {
                Ok(m) => Ok(Some((m.size, m.last_modified.timestamp_millis()))),
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

    /// Every key under `prefix/`, with its size and when it was last written.
    async fn list_raw(&self, prefix: &str) -> Result<Vec<(String, u64, i64)>> {
        match self {
            Self::Disk { root } => {
                let mut out = Vec::new();
                walk(&root.join(prefix), prefix, &mut out).await?;
                Ok(out)
            }
            Self::S3 { store, .. } => {
                let metas: Vec<_> = store.list(Some(&StorePath::from(prefix))).try_collect().await?;
                Ok(metas
                    .into_iter()
                    .map(|m| (m.location.to_string(), m.size, m.last_modified.timestamp_millis()))
                    .collect())
            }
        }
    }

    // -----------------------------------------------------------------------------------------

    /// Stores an object; returns how many bytes it added (0 if it was already there). Putting
    /// an object that exists marks it as just stored, so a conditional delete keeps it: a
    /// writer that is about to reference it again cannot lose it to a sweep.
    pub async fn put(&self, account: &AccountId, id: &ObjectId, bytes: Vec<u8>) -> Result<u64> {
        let key = object_key(account, id);
        if self.size_raw(&key).await?.is_some() {
            match self {
                Self::Disk { root } => {
                    let path = root.join(&key);
                    tokio::task::spawn_blocking(move || {
                        std::fs::File::options().write(true).open(path)?.set_modified(std::time::SystemTime::now())
                    })
                    .await??;
                }
                // The same bytes again: a bucket has no cheaper way to refresh the time.
                Self::S3 { .. } => self.put_raw(&key, bytes).await?,
            }
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

    /// Deletes an object, or with `older_than_ms` only if it was last stored before then.
    /// Returns the bytes freed, or `None` if it was absent or kept.
    pub async fn delete(&self, account: &AccountId, id: &ObjectId, older_than_ms: Option<i64>) -> Result<Option<u64>> {
        let key = object_key(account, id);
        let Some((size, stored)) = self.stat_raw(&key).await? else { return Ok(None) };
        if older_than_ms.is_some_and(|t| stored >= t) {
            return Ok(None);
        }
        self.delete_raw(&key).await?;
        Ok(Some(size))
    }

    /// The account's objects in id order, after `after`, at most `limit`.
    pub async fn list(
        &self,
        account: &AccountId,
        after: Option<ObjectId>,
        limit: usize,
    ) -> Result<Vec<ursa_proto::StoredObject>> {
        let mut all: Vec<ursa_proto::StoredObject> = self
            .list_raw(&format!("objects/{account}"))
            .await?
            .into_iter()
            .filter_map(|(key, len, stored_ms)| {
                Some(ursa_proto::StoredObject { id: key.rsplit('/').next()?.parse().ok()?, len, stored_ms })
            })
            .filter(|o| after.is_none_or(|a| o.id > a))
            .collect();
        all.sort_by_key(|o| o.id);
        all.truncate(limit);
        Ok(all)
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
        for (key, size, _) in self.list_raw("objects").await? {
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

    pub async fn uptime(&self) -> Result<crate::uptime::Uptime> {
        match self.get_raw(UPTIME_KEY).await? {
            Some(b) => Ok(serde_json::from_slice(&b).unwrap_or_default()),
            None => Ok(Default::default()),
        }
    }

    pub async fn set_uptime(&self, u: &crate::uptime::Uptime) -> Result<()> {
        self.put_raw(UPTIME_KEY, serde_json::to_vec(u)?).await
    }

    /// The disk's size and free space, for the disk backend. A bucket has no fixed size.
    pub fn disk_space(&self) -> Option<(u64, u64)> {
        match self {
            Self::Disk { root } => {
                let _ = std::fs::create_dir_all(root);
                Some((fs2::total_space(root).ok()?, fs2::available_space(root).ok()?))
            }
            Self::S3 { .. } => None,
        }
    }
}

fn ms_of(t: std::time::SystemTime) -> i64 {
    t.duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

fn rand_suffix() -> String {
    let mut b = [0u8; 4];
    getrandom::fill(&mut b).expect("OS randomness");
    hex::encode(b)
}

/// Recursive listing of a directory, keys relative to the store root.
fn walk<'a>(
    dir: &'a Path,
    prefix: &'a str,
    out: &'a mut Vec<(String, u64, i64)>,
) -> futures_util::future::BoxFuture<'a, Result<()>> {
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
                out.push((key, meta.len(), meta.modified().map(ms_of).unwrap_or(0)));
            }
        }
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (Store, PathBuf) {
        let dir = std::env::temp_dir().join(format!("ursa-store-test-{}", rand_suffix()));
        (Store::disk(dir.clone()), dir)
    }

    fn now() -> i64 {
        ms_of(std::time::SystemTime::now())
    }

    #[tokio::test]
    async fn a_conditional_delete_keeps_what_was_stored_since() {
        let (s, dir) = store();
        let a = ursa_core::account::Account::generate().id();
        let (x, y) = (b"old piece".to_vec(), b"another".to_vec());
        let (ix, iy) = (ObjectId::of(&x), ObjectId::of(&y));
        s.put(&a, &ix, x.clone()).await.unwrap();
        s.put(&a, &iy, y).await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let cutoff = now();
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        // Putting x again (a writer re-using it) makes it newer than the cutoff.
        assert_eq!(s.put(&a, &ix, x).await.unwrap(), 0);
        assert_eq!(s.delete(&a, &ix, Some(cutoff)).await.unwrap(), None, "re-put object was deleted");
        assert!(s.has(&a, &ix).await.unwrap());
        // y was not touched: it goes, and its bytes are counted.
        assert_eq!(s.delete(&a, &iy, Some(cutoff)).await.unwrap(), Some(7));
        assert!(!s.has(&a, &iy).await.unwrap());
        // Unconditional deletes still work; absent objects report nothing.
        assert_eq!(s.delete(&a, &ix, None).await.unwrap(), Some(9));
        assert_eq!(s.delete(&a, &ix, None).await.unwrap(), None);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn listing_pages_through_one_account_in_order() {
        let (s, dir) = store();
        let (a, b) = (ursa_core::account::Account::generate().id(), ursa_core::account::Account::generate().id());
        let mut ids = Vec::new();
        for i in 0..7u8 {
            let bytes = vec![i; 10 + i as usize];
            let id = ObjectId::of(&bytes);
            s.put(&a, &id, bytes).await.unwrap();
            ids.push(id);
        }
        s.put(&b, &ObjectId::of(b"not a's"), b"not a's".to_vec()).await.unwrap();
        ids.sort();
        let first = s.list(&a, None, 4).await.unwrap();
        let rest = s.list(&a, first.last().map(|o| o.id), 4).await.unwrap();
        let got: Vec<ObjectId> = first.iter().chain(&rest).map(|o| o.id).collect();
        assert_eq!(got, ids);
        assert!(first.iter().all(|o| o.stored_ms > 0 && o.len >= 10));
        let _ = std::fs::remove_dir_all(dir);
    }
}
