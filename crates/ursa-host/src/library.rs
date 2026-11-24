//! The library as one device sees it: the node connection, the manifest cache, and the
//! upload and download pipelines.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use futures_util::{StreamExt, TryStreamExt, stream};
use iroh::{Endpoint, EndpointAddr, RelayMap, RelayMode, RelayUrl, SecretKey};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::io::AsyncWriteExt;
use tokio::sync::{Mutex, mpsc};
use tracing::{info, warn};
use ursa_core::crypto::{AccountKey, open_chunk, seal_chunk};
use ursa_core::manifest::{self, ChunkRef, FileManifest};
use ursa_core::{ObjectId, chunk};
use ursa_proto::{ClaimLink, NodeClient, parse_addr, ticket_of};

/// Chunks in flight per upload or download: enough to keep a home link busy (S2 used two
/// requests per node; one node here, so a few more), small enough to bound memory (4 MiB each).
const IN_FLIGHT: usize = 6;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

/// Where responses and progress go.
#[derive(Clone)]
pub struct Out {
    id: u64,
    tx: mpsc::UnboundedSender<Value>,
}

impl Out {
    pub fn new(id: u64, tx: mpsc::UnboundedSender<Value>) -> Self {
        Self { id, tx }
    }

    pub fn send(&self, v: Value) {
        let _ = self.tx.send(v);
    }

    fn progress(&self, done: u64, total: u64) {
        self.send(json!({ "event": "progress", "id": self.id, "done": done, "total": total }));
    }
}

#[derive(Debug, Deserialize)]
pub struct UploadMeta {
    pub path: PathBuf,
    /// Shown name; defaults to the file name.
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub mime: Option<String>,
    #[serde(default)]
    pub created_ms: Option<i64>,
    #[serde(default)]
    pub modified_ms: Option<i64>,
    #[serde(default)]
    pub width: Option<u32>,
    #[serde(default)]
    pub height: Option<u32>,
    /// A small JPEG the app made for the gallery.
    #[serde(default)]
    pub thumb_path: Option<PathBuf>,
}

pub struct Library {
    data_dir: PathBuf,
    account: AccountKey,
    endpoint: Endpoint,
    node: Mutex<Option<EndpointAddr>>,
    client: Mutex<Option<NodeClient>>,
    manifests: Mutex<HashMap<ObjectId, FileManifest>>,
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

impl Library {
    pub async fn open(
        data_dir: PathBuf,
        account_key: &str,
        relays: Vec<String>,
        relay_token: Option<String>,
    ) -> Result<Self> {
        let account = AccountKey::from_hex(account_key).map_err(|e| anyhow!("{e}"))?;
        std::fs::create_dir_all(data_dir.join("cache").join("manifests"))?;
        std::fs::create_dir_all(data_dir.join("cache").join("thumbs"))?;

        // The device's own identity: the node knows it as an owner once it has claimed.
        let key_path = data_dir.join("device.key");
        let secret = match std::fs::read_to_string(&key_path) {
            Ok(s) => {
                let mut k = [0u8; 32];
                hex::decode_to_slice(s.trim(), &mut k).context("device.key is corrupt")?;
                SecretKey::from_bytes(&k)
            }
            Err(_) => {
                let mut k = [0u8; 32];
                getrandom::fill(&mut k).expect("OS randomness");
                std::fs::write(&key_path, hex::encode(k))?;
                SecretKey::from_bytes(&k)
            }
        };

        let mut builder = Endpoint::builder(iroh::endpoint::presets::N0).secret_key(secret);
        if !relays.is_empty() {
            let urls = relays
                .iter()
                .map(|u| u.trim().parse::<RelayUrl>().map_err(|e| anyhow!("bad relay URL {u}: {e}")))
                .collect::<Result<Vec<_>>>()?;
            let mut map = RelayMap::from_iter(urls);
            if let Some(t) = relay_token.filter(|t| !t.trim().is_empty()) {
                map = map.with_auth_token(t.trim().to_owned());
            }
            builder = builder.relay_mode(RelayMode::Custom(map));
        }
        let endpoint = builder.bind().await.context("start the network endpoint")?;

        let node = match std::fs::read_to_string(data_dir.join("node.json")) {
            Ok(s) => {
                let v: Value = serde_json::from_str(&s).context("node.json is corrupt")?;
                v["ticket"].as_str().map(parse_addr).transpose()?
            }
            Err(_) => None,
        };
        info!(device = %endpoint.id(), configured = node.is_some(), "host ready");
        Ok(Self {
            data_dir,
            account,
            endpoint,
            node: Mutex::new(node),
            client: Mutex::new(None),
            manifests: Mutex::new(HashMap::new()),
        })
    }

    pub fn device_id(&self) -> String {
        self.endpoint.id().to_string()
    }

    /// The live connection, reconnecting when it has closed.
    async fn client(&self) -> Result<NodeClient> {
        let mut slot = self.client.lock().await;
        if let Some(c) = slot.as_ref().filter(|c| !c.is_closed()) {
            return Ok(c.clone());
        }
        let addr = self.node.lock().await.clone().context("no node yet: connect one in Settings")?;
        let c = tokio::time::timeout(CONNECT_TIMEOUT, NodeClient::connect(&self.endpoint, addr))
            .await
            .map_err(|_| anyhow!("the node did not answer within {}s", CONNECT_TIMEOUT.as_secs()))??;
        *slot = Some(c.clone());
        Ok(c)
    }

    fn path_of(c: &NodeClient) -> String {
        let paths = c.connection().paths();
        match paths.iter().find(|p| p.is_selected()) {
            Some(p) if p.is_ip() => "direct".into(),
            Some(p) if p.is_relay() => "relay".into(),
            _ => "connecting".into(),
        }
    }

    pub async fn status(&self) -> Result<Value> {
        let node = self.node.lock().await.clone();
        let Some(addr) = node else {
            return Ok(json!({ "configured": false, "connected": false, "device_id": self.device_id() }));
        };
        let node_id = addr.id.to_string();
        match self.client().await {
            Ok(c) => match c.info().await {
                Ok(info) => Ok(json!({
                    "configured": true, "connected": true, "device_id": self.device_id(),
                    "node_id": node_id, "path": Self::path_of(&c), "owner": info.owner,
                    "backend": info.backend, "node_version": info.version,
                })),
                Err(e) => Ok(json!({ "configured": true, "connected": false, "device_id": self.device_id(), "node_id": node_id, "error": format!("{e:#}") })),
            },
            Err(e) => Ok(json!({ "configured": true, "connected": false, "device_id": self.device_id(), "node_id": node_id, "error": format!("{e:#}") })),
        }
    }

    /// Claims a node with a link the node printed, and remembers it.
    pub async fn connect(&self, link: &str) -> Result<Value> {
        let link: ClaimLink = link.parse()?;
        let c = tokio::time::timeout(CONNECT_TIMEOUT, NodeClient::connect(&self.endpoint, link.addr.clone()))
            .await
            .map_err(|_| anyhow!("the node did not answer within {}s", CONNECT_TIMEOUT.as_secs()))??;
        c.claim(&link.code).await?;
        let info = c.info().await?;
        if !info.owner {
            bail!("the node accepted the claim but does not list this device as an owner");
        }
        std::fs::write(self.data_dir.join("node.json"), serde_json::to_vec_pretty(&json!({ "ticket": ticket_of(&link.addr) }))?)?;
        *self.node.lock().await = Some(link.addr);
        *self.client.lock().await = Some(c);
        self.status().await
    }

    pub async fn forget(&self) -> Result<Value> {
        let _ = std::fs::remove_file(self.data_dir.join("node.json"));
        *self.node.lock().await = None;
        if let Some(c) = self.client.lock().await.take() {
            c.connection().close(0u32.into(), b"forgotten");
        }
        self.manifests.lock().await.clear();
        Ok(json!({}))
    }

    fn manifest_cache(&self, id: &ObjectId) -> PathBuf {
        self.data_dir.join("cache").join("manifests").join(id.to_hex())
    }

    fn thumb_cache(&self, id: &ObjectId) -> PathBuf {
        self.data_dir.join("cache").join("thumbs").join(format!("{}.jpg", id.to_hex()))
    }

    /// A manifest from memory, the sealed on-disk cache, or the node, in that order. The disk
    /// cache keeps it sealed: it holds every chunk key of the file.
    async fn manifest(&self, id: &ObjectId) -> Result<FileManifest> {
        if let Some(m) = self.manifests.lock().await.get(id) {
            return Ok(m.clone());
        }
        let sealed = match tokio::fs::read(self.manifest_cache(id)).await {
            Ok(b) if id.matches(&b) => b,
            _ => {
                let b = self.client().await?.get(*id).await?;
                let _ = tokio::fs::write(self.manifest_cache(id), &b).await;
                b
            }
        };
        let m = manifest::open(&self.account, id, &sealed).map_err(|e| anyhow!("manifest {id}: {e}"))?;
        self.manifests.lock().await.insert(*id, m.clone());
        Ok(m)
    }

    fn item(id: &ObjectId, m: &FileManifest) -> Value {
        json!({
            "file_id": id.to_hex(), "name": m.name, "size": m.size, "mime": m.mime,
            "created_ms": m.created_ms, "modified_ms": m.modified_ms, "added_ms": m.added_ms,
            "width": m.width, "height": m.height, "has_thumb": m.thumb.is_some(),
        })
    }

    pub async fn list(&self) -> Result<Value> {
        let ids = self.client().await?.index_list().await?;
        let results: Vec<(ObjectId, Result<FileManifest>)> = stream::iter(ids)
            .map(|id| async move { (id, self.manifest(&id).await) })
            .buffer_unordered(8)
            .collect()
            .await;
        let mut items = Vec::new();
        let mut unreadable = 0;
        for (id, r) in results {
            match r {
                Ok(m) => items.push((m.created_ms, Self::item(&id, &m))),
                Err(e) => {
                    warn!("skipping {id}: {e:#}");
                    unreadable += 1;
                }
            }
        }
        items.sort_by_key(|i| std::cmp::Reverse(i.0));
        Ok(json!({ "items": items.into_iter().map(|(_, v)| v).collect::<Vec<_>>(), "unreadable": unreadable }))
    }

    pub async fn upload(&self, meta: UploadMeta, out: &Out) -> Result<Value> {
        let client = self.client().await?;
        let file_meta = tokio::fs::metadata(&meta.path).await.with_context(|| format!("read {}", meta.path.display()))?;
        let total = file_meta.len();
        let name = meta
            .name
            .clone()
            .or_else(|| meta.path.file_name().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_else(|| "untitled".into());
        let modified_ms = meta.modified_ms.unwrap_or_else(|| {
            file_meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as i64)
                .unwrap_or_else(now_ms)
        });

        // Chunking and sealing are CPU work: a blocking thread feeds sealed chunks to the
        // network side through a small queue, so memory stays bounded.
        let (tx, mut rx) = mpsc::channel(IN_FLIGHT);
        let account = self.account.clone();
        let path = meta.path.clone();
        let reader = tokio::task::spawn_blocking(move || -> Result<()> {
            let f = std::io::BufReader::with_capacity(1 << 20, std::fs::File::open(&path)?);
            for piece in chunk::chunks(f) {
                let sealed = seal_chunk(&account, &piece?);
                if tx.blocking_send(sealed).is_err() {
                    break;
                }
            }
            Ok(())
        });
        let sealed_stream = stream::poll_fn(move |cx| rx.poll_recv(cx));

        let done = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let mut last_report = Instant::now();
        out.progress(0, total);
        let mut chunks: Vec<ChunkRef> = Vec::new();
        let mut uploads = sealed_stream
            .map(|s| {
                let client = client.clone();
                async move {
                    if !client.has(vec![s.id]).await?.first().copied().unwrap_or(false) {
                        client.put(s.id, &s.ciphertext).await?;
                    }
                    Ok::<_, anyhow::Error>(ChunkRef { id: s.id, key: s.key, len: s.plain_len })
                }
            })
            .buffered(IN_FLIGHT);
        while let Some(r) = uploads.next().await {
            let c = r?;
            let d = done.fetch_add(c.len, std::sync::atomic::Ordering::Relaxed) + c.len;
            chunks.push(c);
            if last_report.elapsed() > Duration::from_millis(100) {
                out.progress(d, total);
                last_report = Instant::now();
            }
        }
        reader.await??;

        let thumb = match &meta.thumb_path {
            Some(p) => {
                let bytes = tokio::fs::read(p).await.with_context(|| format!("read thumbnail {}", p.display()))?;
                let s = seal_chunk(&self.account, &bytes);
                client.put(s.id, &s.ciphertext).await?;
                Some(ChunkRef { id: s.id, key: s.key, len: s.plain_len })
            }
            None => None,
        };

        let m = FileManifest {
            v: manifest::VERSION,
            name,
            size: total,
            mime: meta.mime.clone().unwrap_or_else(|| "application/octet-stream".into()),
            created_ms: meta.created_ms.unwrap_or(modified_ms),
            modified_ms,
            added_ms: now_ms(),
            width: meta.width,
            height: meta.height,
            chunks,
            thumb,
        };
        let (id, sealed) = manifest::seal(&self.account, &m);
        client.put(id, &sealed).await?;
        client.index_add(id).await?;
        let _ = tokio::fs::write(self.manifest_cache(&id), &sealed).await;
        if let (Some(p), Some(_)) = (&meta.thumb_path, &m.thumb) {
            let _ = tokio::fs::copy(p, self.thumb_cache(&id)).await;
        }
        self.manifests.lock().await.insert(id, m.clone());
        out.progress(total, total);
        Ok(json!({ "item": Self::item(&id, &m) }))
    }

    pub async fn thumb(&self, file_id: &str) -> Result<Value> {
        let id: ObjectId = file_id.parse().map_err(|e| anyhow!("{e}"))?;
        let path = self.thumb_cache(&id);
        if tokio::fs::try_exists(&path).await.unwrap_or(false) {
            return Ok(json!({ "path": path }));
        }
        let m = self.manifest(&id).await?;
        let t = m.thumb.context("this file has no thumbnail")?;
        let ct = self.client().await?.get(t.id).await?;
        let jpeg = open_chunk(&t.id, &t.key, &ct).map_err(|e| anyhow!("{e}"))?;
        let tmp = path.with_extension("part");
        tokio::fs::write(&tmp, &jpeg).await?;
        tokio::fs::rename(&tmp, &path).await?;
        Ok(json!({ "path": path }))
    }

    pub async fn download(&self, file_id: &str, out_path: &Path, out: &Out) -> Result<Value> {
        let id: ObjectId = file_id.parse().map_err(|e| anyhow!("{e}"))?;
        let m = self.manifest(&id).await?;
        let client = self.client().await?;
        let tmp = out_path.with_extension(format!(
            "{}ursa-part",
            out_path.extension().map(|e| format!("{}.", e.to_string_lossy())).unwrap_or_default()
        ));
        let mut file = tokio::fs::File::create(&tmp).await.with_context(|| format!("create {}", tmp.display()))?;
        let total = m.size;
        let mut done = 0u64;
        let mut last_report = Instant::now();
        out.progress(0, total);
        let result: Result<()> = async {
            let mut pieces = stream::iter(m.chunks.clone())
                .map(|c| {
                    let client = client.clone();
                    async move {
                        let ct = client.get(c.id).await?;
                        open_chunk(&c.id, &c.key, &ct).map_err(|e| anyhow!("{e}"))
                    }
                })
                .buffered(IN_FLIGHT);
            while let Some(plain) = pieces.try_next().await? {
                file.write_all(&plain).await?;
                done += plain.len() as u64;
                if last_report.elapsed() > Duration::from_millis(100) {
                    out.progress(done, total);
                    last_report = Instant::now();
                }
            }
            file.flush().await?;
            file.sync_all().await?;
            Ok(())
        }
        .await;
        drop(file);
        if let Err(e) = result {
            let _ = tokio::fs::remove_file(&tmp).await;
            return Err(e);
        }
        if done != total {
            let _ = tokio::fs::remove_file(&tmp).await;
            bail!("got {done} of {total} bytes");
        }
        let _ = tokio::fs::remove_file(out_path).await;
        tokio::fs::rename(&tmp, out_path).await?;
        out.progress(total, total);
        Ok(json!({ "path": out_path }))
    }

    /// Removes the file from the library. Its chunks stay on the node until compaction exists.
    pub async fn delete(&self, file_id: &str) -> Result<Value> {
        let id: ObjectId = file_id.parse().map_err(|e| anyhow!("{e}"))?;
        self.client().await?.index_remove(id).await?;
        self.manifests.lock().await.remove(&id);
        let _ = tokio::fs::remove_file(self.manifest_cache(&id)).await;
        let _ = tokio::fs::remove_file(self.thumb_cache(&id)).await;
        Ok(json!({}))
    }
}
