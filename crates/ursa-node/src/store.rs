//! Where a node keeps things. Two backends behind one type: a directory, or an S3-compatible
//! bucket (SeaweedFS on Buttercup). Layout, identical in both:
//!
//! - `objects/<first two hex>/<id>`: encrypted objects, named by their BLAKE3 hash.
//! - `index/<id>`: an empty marker per manifest in the owner's library.
//! - `node/owners.json`: the endpoint ids allowed to use the node.
//!
//! Everything the node knows lives here, so with the S3 backend the node itself is stateless.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use bytes::Bytes;
use futures_util::TryStreamExt;
use object_store::path::Path as StorePath;
use object_store::{ObjectStore, ObjectStoreExt, PutPayload};
use ursa_core::ObjectId;

#[derive(Clone)]
pub enum Store {
    Disk { root: PathBuf },
    S3 { store: Arc<dyn ObjectStore>, label: String },
}

pub struct S3Config {
    pub endpoint: String,
    pub bucket: String,
    pub region: String,
    pub access_key_id: String,
    pub secret_access_key: String,
}

fn object_key(id: &ObjectId) -> String {
    let hex = id.to_hex();
    format!("objects/{}/{hex}", &hex[..2])
}

fn index_key(id: &ObjectId) -> String {
    format!("index/{}", id.to_hex())
}

const OWNERS_KEY: &str = "node/owners.json";

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
        Ok(Self::S3 { store: Arc::new(store), label: format!("s3 {}/{}", c.endpoint, c.bucket) })
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
                let tmp = path.with_extension(format!("tmp-{}", std::process::id()));
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

    async fn exists_raw(&self, key: &str) -> Result<bool> {
        match self {
            Self::Disk { root } => Ok(tokio::fs::try_exists(root.join(key)).await?),
            Self::S3 { store, .. } => match store.head(&StorePath::from(key)).await {
                Ok(_) => Ok(true),
                Err(object_store::Error::NotFound { .. }) => Ok(false),
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

    /// The last path component of every key under `prefix/`.
    async fn list_raw(&self, prefix: &str) -> Result<Vec<String>> {
        match self {
            Self::Disk { root } => {
                let mut out = Vec::new();
                let mut dir = match tokio::fs::read_dir(root.join(prefix)).await {
                    Ok(d) => d,
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
                    Err(e) => return Err(e.into()),
                };
                while let Some(entry) = dir.next_entry().await? {
                    out.push(entry.file_name().to_string_lossy().into_owned());
                }
                Ok(out)
            }
            Self::S3 { store, .. } => {
                let metas: Vec<_> = store.list(Some(&StorePath::from(prefix))).try_collect().await?;
                Ok(metas.into_iter().filter_map(|m| m.location.filename().map(str::to_string)).collect())
            }
        }
    }

    pub async fn put(&self, id: &ObjectId, bytes: Vec<u8>) -> Result<()> {
        self.put_raw(&object_key(id), bytes).await
    }

    pub async fn get(&self, id: &ObjectId) -> Result<Option<Vec<u8>>> {
        self.get_raw(&object_key(id)).await
    }

    pub async fn has(&self, id: &ObjectId) -> Result<bool> {
        self.exists_raw(&object_key(id)).await
    }

    pub async fn index_add(&self, id: &ObjectId) -> Result<()> {
        self.put_raw(&index_key(id), Vec::new()).await
    }

    pub async fn index_remove(&self, id: &ObjectId) -> Result<()> {
        self.delete_raw(&index_key(id)).await
    }

    pub async fn index_list(&self) -> Result<Vec<ObjectId>> {
        Ok(self.list_raw("index").await?.into_iter().filter_map(|n| n.parse().ok()).collect())
    }

    pub async fn owners(&self) -> Result<Vec<String>> {
        match self.get_raw(OWNERS_KEY).await? {
            Some(b) => Ok(serde_json::from_slice(&b).context("read owners")?),
            None => Ok(Vec::new()),
        }
    }

    pub async fn set_owners(&self, owners: &[String]) -> Result<()> {
        self.put_raw(OWNERS_KEY, serde_json::to_vec_pretty(owners)?).await
    }
}
