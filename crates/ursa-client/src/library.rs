//! The library: upload, list, download, delete, repair. Every object goes through the pool as a
//! coded [`ShardSet`]; the library index (which manifests exist, and where they are) is itself a
//! coded object the account head points at.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use futures_util::{StreamExt, TryStreamExt, stream};
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;
use tracing::{info, warn};
use ursa_core::ObjectId;
use ursa_core::chunk;
use ursa_core::crypto::{open_chunk, seal_chunk};
use ursa_core::head::{IndexEntry, LibraryIndex};
use ursa_core::manifest::{self, ChunkRef, FileManifest, ShardSet, StoredRef};

use crate::objects::codes_for;
use crate::stats::{Stage, Stats, mb, rate};
use crate::{Session, now_ms};

/// Chunks in flight per upload or download.
const IN_FLIGHT: usize = 4;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct UploadMeta {
    pub path: PathBuf,
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
    #[serde(default)]
    pub thumb_path: Option<PathBuf>,
    /// The folder to put it in (an id from the tree); `None`: the root.
    #[serde(default)]
    pub dir: Option<String>,
}

/// A file in the library, as a list shows it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Item {
    pub file_id: String,
    pub name: String,
    pub size: u64,
    pub mime: String,
    pub created_ms: i64,
    pub modified_ms: i64,
    pub added_ms: i64,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub has_thumb: bool,
    /// The folder it is in; `None`: the root.
    #[serde(default)]
    pub dir: Option<String>,
    /// Its folder's path, `/`-separated; `""` at the root.
    #[serde(default)]
    pub path: String,
}

impl Item {
    pub(crate) fn of(id: &ObjectId, m: &FileManifest) -> Self {
        Self {
            file_id: id.to_hex(),
            name: m.name.clone(),
            dir: None,
            path: String::new(),
            size: m.size,
            mime: m.mime.clone(),
            created_ms: m.created_ms,
            modified_ms: m.modified_ms,
            added_ms: m.added_ms,
            width: m.width,
            height: m.height,
            has_thumb: m.thumb.is_some(),
        }
    }
}

/// What repair did.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RepairReport {
    pub files: usize,
    pub objects_checked: usize,
    pub objects_rebuilt: usize,
    pub unrecoverable: Vec<String>,
}

/// An uploaded file whose index entry is waiting for the next index change.
#[derive(Clone, Debug)]
pub(crate) struct Staged {
    id: ObjectId,
    added_ms: i64,
    at: ShardSet,
    dir: Option<String>,
}

impl Staged {
    fn apply(&self, idx: &mut LibraryIndex) {
        idx.add(self.id, self.added_ms, Some(self.at.clone()));
        if self.dir.is_some()
            && let Some(e) = idx.entries.get_mut(&self.id)
            && e.placed_ms == 0
        {
            e.dir = self.dir.clone();
            e.placed_ms = self.added_ms;
        }
    }
}

impl Session {
    // -----------------------------------------------------------------------------------------
    // The index

    /// The index as stored, without the uploads still waiting to be written.
    async fn stored_index(&self) -> Result<LibraryIndex> {
        let Some(r) = self.state().await.index else { return Ok(LibraryIndex::default()) };
        let sealed = self.cached_or_fetch(&r.id, &r.shards).await?;
        LibraryIndex::open(&self.account, &r.id, &sealed).map_err(|e| anyhow!("library index: {e}"))
    }

    /// The index as this device sees it: the stored one plus the uploads not written yet, so a
    /// staged file lists, shows its thumbnail and downloads like any other.
    pub(crate) async fn index(&self) -> Result<LibraryIndex> {
        let mut idx = self.stored_index().await?;
        for s in self.pending_uploads() {
            s.apply(&mut idx);
        }
        Ok(idx)
    }

    fn pending_uploads(&self) -> Vec<Staged> {
        self.pending.lock().expect("pending uploads").clone()
    }

    /// How many uploads are stored but not yet in the index.
    pub fn pending_count(&self) -> usize {
        self.pending.lock().expect("pending uploads").len()
    }

    /// Writes every staged upload into the index in one change. Returns how many it wrote.
    pub async fn commit_uploads(&self) -> Result<usize> {
        let n = self.pending_count();
        if n == 0 {
            return Ok(0);
        }
        // change_index writes everything pending along with its own change, here none.
        self.change_index(|_| {}).await?;
        info!(files = n, "uploads committed");
        Ok(n)
    }

    /// Applies `change` to the index and saves it, merging with concurrent changes: if the head
    /// moved under us, the change is re-applied to the newer index.
    ///
    /// Every staged upload is written with it: a change is one index upload and one head save
    /// whatever it carries, so this is where uploads are batched.
    pub(crate) async fn change_index(&self, change: impl Fn(&mut LibraryIndex)) -> Result<()> {
        let started = std::time::Instant::now();
        let staged = self.pending_count();
        let (stats, r) = Stats::default().scope(self.change_index_inner(change)).await;
        let paths = self.paths_now().await;
        let path = |id: &str| paths.get(id).cloned().unwrap_or_else(|| "-".into());
        match &r {
            Ok(index_bytes) => info!(
                staged,
                index = %mb(*index_bytes as u64),
                ms = started.elapsed().as_millis() as u64,
                stages = %stats.stages(),
                nodes = %stats.nodes(path),
                "library change written"
            ),
            Err(e) => warn!(
                staged,
                ms = started.elapsed().as_millis() as u64,
                stages = %stats.stages(),
                nodes = %stats.nodes(path),
                "library change failed: {e:#}"
            ),
        }
        r.map(|_| ())
    }

    /// Returns the sealed index's size.
    async fn change_index_inner(&self, change: impl Fn(&mut LibraryIndex)) -> Result<usize> {
        let _queued = {
            let _t = Stage::start("queue");
            self.index_lock.lock().await
        };
        // Staged from here on are left for the next change; these are written by this one.
        let staged = self.pending_uploads();
        for _ in 0..5 {
            let before = self.state().await.index.as_ref().map(|r| r.id);
            let mut idx = {
                let _t = Stage::start("read");
                self.stored_index().await?
            };
            for s in &staged {
                s.apply(&mut idx);
            }
            change(&mut idx);
            let (id, sealed) = {
                let _t = Stage::start("seal");
                idx.seal(&self.account)
            };
            let writable = {
                let _t = Stage::start("pool");
                self.writable().await
            };
            let set = {
                let _t = Stage::start("store");
                let (_, small) = codes_for(self.state().await.protection, writable.len())?;
                self.store_object(&sealed, small, &writable, rand_usize()).await?
            };
            let new = StoredRef { id, shards: set };
            let _head = Stage::start("head");
            let state = self
                .update(move |s| {
                    // Only replace the index we started from; otherwise update() loaded a newer
                    // head and we must merge again.
                    if s.index.as_ref().map(|r| r.id) == before {
                        s.index = Some(new.clone());
                    }
                })
                .await?;
            if state.index.as_ref().map(|r| r.id) == Some(id) {
                if !staged.is_empty() {
                    let written: std::collections::HashSet<ObjectId> = staged.iter().map(|s| s.id).collect();
                    self.pending.lock().expect("pending uploads").retain(|s| !written.contains(&s.id));
                }
                return Ok(sealed.len());
            }
        }
        bail!("the library kept changing; try again")
    }

    async fn cached_or_fetch(&self, id: &ObjectId, set: &ShardSet) -> Result<Vec<u8>> {
        let path = self.device.data_dir.join("cache").join("objects").join(id.to_hex());
        if let Ok(b) = tokio::fs::read(&path).await
            && id.matches(&b)
        {
            return Ok(b);
        }
        let b = self.fetch_object(id, set).await?;
        let _ = tokio::fs::write(&path, &b).await;
        Ok(b)
    }

    pub(crate) async fn manifest(&self, id: &ObjectId, entry: &IndexEntry) -> Result<FileManifest> {
        let set = entry.at.as_ref().context("the index does not say where this file's manifest is")?;
        let mid = entry.manifest_id(id);
        let sealed = self.cached_or_fetch(&mid, set).await?;
        manifest::open(self.account.root(), &mid, &sealed).map_err(|e| anyhow!("manifest {mid}: {e}"))
    }

    async fn entry(&self, file_id: &str) -> Result<(ObjectId, IndexEntry, FileManifest)> {
        let id: ObjectId = file_id.parse().map_err(|e| anyhow!("{e}"))?;
        let idx = self.index().await?;
        let e = idx.entries.get(&id).filter(|e| !e.deleted).cloned().context("no such file in the library")?;
        let m = self.manifest(&id, &e).await?;
        Ok((id, e, m))
    }

    // -----------------------------------------------------------------------------------------

    /// Every file, newest first, with its folder and its name as the tree shows it.
    pub async fn list(&self) -> Result<Vec<Item>> {
        let mut items = self.tree().await?.files;
        items.sort_by_key(|i| std::cmp::Reverse(i.created_ms));
        Ok(items)
    }

    /// The live files' manifests (those that open), with the index they came from.
    pub(crate) async fn live_files(&self) -> Result<(LibraryIndex, Vec<(ObjectId, FileManifest)>)> {
        let idx = self.index().await?;
        let live: Vec<(ObjectId, IndexEntry)> = idx.entries.iter().filter(|(_, e)| !e.deleted).map(|(i, e)| (*i, e.clone())).collect();
        let files = stream::iter(live)
            .map(|(id, e)| async move { self.manifest(&id, &e).await.map(|m| (id, m)) })
            .buffer_unordered(8)
            .filter_map(|r| async move {
                match r {
                    Ok(i) => Some(i),
                    Err(e) => {
                        warn!("skipping a file: {e:#}");
                        None
                    }
                }
            })
            .collect()
            .await;
        Ok((idx, files))
    }

    /// Uploads a file and writes it into the library index.
    pub async fn upload(&self, meta: UploadMeta, progress: impl Fn(u64, u64)) -> Result<Item> {
        let item = self.upload_staged(meta, progress).await?;
        self.commit_uploads().await?;
        Ok(item)
    }

    /// Uploads a file (its chunks, thumbnail and manifest) and stages its index entry instead
    /// of writing it: `commit_uploads` writes every staged entry in one index change. Writing the
    /// index once per file re-uploaded the whole index per photo during a backup, which grows with
    /// the library and left every replaced copy on the nodes. A staged file is lost from the
    /// library (its objects orphaned) if the process ends before a commit, so the caller only
    /// counts a file as kept once a commit after it has succeeded.
    pub async fn upload_staged(&self, meta: UploadMeta, progress: impl Fn(u64, u64)) -> Result<Item> {
        let started = std::time::Instant::now();
        let bytes = tokio::fs::metadata(&meta.path).await.map(|m| m.len()).unwrap_or(0);
        let (stats, r) = Stats::default().scope(self.upload_staged_inner(meta, progress)).await;
        let took = started.elapsed();
        let paths = self.paths_now().await;
        let path = |id: &str| paths.get(id).cloned().unwrap_or_else(|| "-".into());
        match &r {
            Ok(_) => info!(
                size = %mb(bytes),
                ms = took.as_millis() as u64,
                rate = %rate(bytes, took),
                stages = %stats.stages(),
                nodes = %stats.nodes(path),
                "upload done"
            ),
            Err(e) => warn!(
                size = %mb(bytes),
                ms = took.as_millis() as u64,
                stages = %stats.stages(),
                nodes = %stats.nodes(path),
                "upload failed: {e:#}"
            ),
        }
        r
    }

    async fn upload_staged_inner(&self, meta: UploadMeta, progress: impl Fn(u64, u64)) -> Result<Item> {
        let writable = {
            let _t = Stage::start("pool");
            self.writable().await
        };
        let (big, small) = codes_for(self.state().await.protection, writable.len())?;
        let fmeta = tokio::fs::metadata(&meta.path).await.with_context(|| format!("read {}", meta.path.display()))?;
        let total = fmeta.len();
        let name = meta
            .name
            .clone()
            .or_else(|| meta.path.file_name().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_else(|| "untitled".into());
        let modified_ms = meta.modified_ms.unwrap_or_else(|| {
            fmeta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_millis() as i64).unwrap_or_else(now_ms)
        });

        // Chunk and seal on a blocking thread; code and place on the network side.
        let (tx, mut rx) = tokio::sync::mpsc::channel(IN_FLIGHT);
        let account = self.account.clone();
        let path = meta.path.clone();
        // Returns the time spent reading and sealing (not waiting for the network to take more).
        let reader = tokio::task::spawn_blocking(move || -> Result<std::time::Duration> {
            let f = std::io::BufReader::with_capacity(1 << 20, std::fs::File::open(&path)?);
            let mut sealing = std::time::Duration::ZERO;
            let mut pieces = chunk::chunks(f);
            loop {
                let t = std::time::Instant::now();
                let Some(piece) = pieces.next() else { break };
                let sealed = seal_chunk(account.root(), &piece?);
                sealing += t.elapsed();
                if tx.blocking_send(sealed).is_err() {
                    break;
                }
            }
            Ok(sealing)
        });
        let rotate = rand_usize();
        let mut done = 0u64;
        progress(0, total);
        let mut chunks = Vec::new();
        let placing = Stage::start("place");
        let mut uploads = stream::poll_fn(move |cx| rx.poll_recv(cx))
            .enumerate()
            .map(|(i, s)| {
                let writable = &writable;
                async move {
                    let set = self.store_object(&s.ciphertext, big, writable, rotate + i).await?;
                    Ok::<_, anyhow::Error>(ChunkRef { id: s.id, key: s.key, len: s.plain_len, shards: Some(set) })
                }
            })
            .buffered(IN_FLIGHT);
        while let Some(c) = uploads.next().await {
            let c = c?;
            done += c.len;
            progress(done, total);
            chunks.push(c);
        }
        drop(uploads);
        drop(placing);
        Stats::stage("read+seal", reader.await??);

        let thumb_stage = Stage::start("thumb");
        let thumb = match &meta.thumb_path {
            Some(p) => {
                let bytes = tokio::fs::read(p).await.with_context(|| format!("read thumbnail {}", p.display()))?;
                let s = seal_chunk(self.account.root(), &bytes);
                let set = self.store_object(&s.ciphertext, small, &writable, rotate).await?;
                let _ = tokio::fs::write(self.thumb_cache(&s.id), &bytes).await;
                Some(ChunkRef { id: s.id, key: s.key, len: s.plain_len, shards: Some(set) })
            }
            None => None,
        };
        drop(thumb_stage);
        let _manifest = Stage::start("manifest");
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
        let (id, sealed) = manifest::seal(self.account.root(), &m);
        let at = self.store_object(&sealed, small, &writable, rotate).await?;
        self.pending.lock().expect("pending uploads").push(Staged { id, added_ms: m.added_ms, at, dir: meta.dir.clone() });
        progress(total, total);
        Ok(Item { dir: meta.dir.clone(), ..Item::of(&id, &m) })
    }

    /// A live file's manifest: its size and chunks, for reading parts of it.
    pub async fn file_manifest(&self, file_id: &str) -> Result<FileManifest> {
        Ok(self.entry(file_id).await?.2)
    }

    /// One chunk of a file, decrypted.
    pub async fn read_chunk(&self, c: &ChunkRef) -> Result<Vec<u8>> {
        let set = c.shards.as_ref().context("chunk has no shards (a 0.1 library)")?;
        let ct = self.fetch_object(&c.id, set).await?;
        open_chunk(&c.id, &c.key, &ct).map_err(|e| anyhow!("{e}"))
    }

    fn thumb_cache(&self, chunk: &ObjectId) -> PathBuf {
        self.device.data_dir.join("cache").join("thumbs").join(format!("{}.jpg", chunk.to_hex()))
    }

    /// The decrypted thumbnail, cached on this device.
    pub async fn thumb(&self, file_id: &str) -> Result<PathBuf> {
        let (_, _, m) = self.entry(file_id).await?;
        let t = m.thumb.context("this file has no thumbnail")?;
        let path = self.thumb_cache(&t.id);
        if tokio::fs::try_exists(&path).await.unwrap_or(false) {
            return Ok(path);
        }
        let set = t.shards.as_ref().context("thumbnail has no shards")?;
        let ct = self.fetch_object(&t.id, set).await?;
        let jpeg = open_chunk(&t.id, &t.key, &ct).map_err(|e| anyhow!("{e}"))?;
        tokio::fs::write(&path, &jpeg).await?;
        Ok(path)
    }

    pub async fn download(&self, file_id: &str, out_path: &Path, progress: impl Fn(u64, u64)) -> Result<()> {
        let started = std::time::Instant::now();
        let (stats, r) = Stats::default().scope(self.download_inner(file_id, out_path, progress)).await;
        let took = started.elapsed();
        let paths = self.paths_now().await;
        let path = |id: &str| paths.get(id).cloned().unwrap_or_else(|| "-".into());
        match &r {
            Ok(bytes) => info!(
                size = %mb(*bytes),
                ms = took.as_millis() as u64,
                rate = %rate(*bytes, took),
                stages = %stats.stages(),
                hedges = stats.hedges(),
                decoded = stats.decodes(),
                nodes = %stats.nodes(path),
                "download done"
            ),
            Err(e) => warn!(
                ms = took.as_millis() as u64,
                stages = %stats.stages(),
                hedges = stats.hedges(),
                nodes = %stats.nodes(path),
                "download failed: {e:#}"
            ),
        }
        r.map(|_| ())
    }

    /// Returns the file's size.
    async fn download_inner(&self, file_id: &str, out_path: &Path, progress: impl Fn(u64, u64)) -> Result<u64> {
        let (_, _, m) = {
            let _t = Stage::start("manifest");
            self.entry(file_id).await?
        };
        let tmp = out_path.with_file_name(format!(
            "{}.ursa-part",
            out_path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
        ));
        let mut file = tokio::fs::File::create(&tmp).await.with_context(|| format!("create {}", tmp.display()))?;
        let total = m.size;
        let mut done = 0u64;
        progress(0, total);
        let fetching = Stage::start("fetch");
        let result: Result<()> = async {
            let mut pieces = stream::iter(m.chunks.clone())
                .map(|c| async move {
                    let set = c.shards.as_ref().context("chunk has no shards (a 0.1 library)")?;
                    let ct = self.fetch_object(&c.id, set).await?;
                    open_chunk(&c.id, &c.key, &ct).map_err(|e| anyhow!("{e}"))
                })
                .buffered(IN_FLIGHT);
            while let Some(plain) = pieces.try_next().await? {
                file.write_all(&plain).await?;
                done += plain.len() as u64;
                progress(done, total);
            }
            file.flush().await?;
            file.sync_all().await?;
            Ok(())
        }
        .await;
        drop(fetching);
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
        Ok(total)
    }

    /// Removes a file from the library. Its shards stay until compaction exists.
    pub async fn delete(&self, file_id: &str) -> Result<()> {
        self.delete_many(&[file_id]).await
    }

    /// Removes several files in one index change: one index upload and one head save however
    /// many there are, where deleting them one by one wrote the whole index once per file.
    pub async fn delete_many(&self, file_ids: &[&str]) -> Result<()> {
        let ids = file_ids
            .iter()
            .map(|f| f.parse::<ObjectId>().map_err(|e| anyhow!("{f}: {e}")))
            .collect::<Result<Vec<_>>>()?;
        if ids.is_empty() {
            return Ok(());
        }
        self.change_index(move |idx| {
            for id in &ids {
                idx.delete(*id);
            }
        })
        .await
    }

    /// Checks every object of every file, rebuilds missing shards onto live nodes, rewrites the
    /// manifests and the index that moved. This is how a lost node is replaced and how a new
    /// home node gets filled.
    pub async fn repair(&self, progress: impl Fn(usize, usize)) -> Result<RepairReport> {
        let writable = self.writable().await;
        let mut report = RepairReport::default();
        let idx = self.index().await?;
        let live: Vec<(ObjectId, IndexEntry)> = idx.entries.iter().filter(|(_, e)| !e.deleted).map(|(i, e)| (*i, e.clone())).collect();
        report.files = live.len();
        let mut moved: Vec<(ObjectId, ObjectId, ShardSet, i64)> = Vec::new();
        for (n, (id, entry)) in live.iter().enumerate() {
            progress(n, live.len());
            let Some(at) = &entry.at else { continue };
            let mut m = match self.manifest(id, entry).await {
                Ok(m) => m,
                Err(e) => {
                    report.unrecoverable.push(format!("{}: {e:#}", id.to_hex()));
                    continue;
                }
            };
            let mut changed = false;
            let mut refs: Vec<&mut ChunkRef> = m.chunks.iter_mut().collect();
            if let Some(t) = m.thumb.as_mut() {
                refs.push(t);
            }
            for c in refs {
                let Some(set) = c.shards.clone() else { continue };
                report.objects_checked += 1;
                match self.repair_set(&c.id, &set, &writable).await {
                    Ok(Some(new)) => {
                        c.shards = Some(new);
                        changed = true;
                        report.objects_rebuilt += 1;
                    }
                    Ok(None) => {}
                    Err(e) => report.unrecoverable.push(format!("{} chunk {}: {e:#}", m.name, c.id.to_hex())),
                }
            }
            report.objects_checked += 1;
            if changed {
                // New placement: a new manifest object replaces the old one in the index.
                let (nid, sealed) = manifest::seal(self.account.root(), &m);
                let set = self.store_object(&sealed, at.code, &writable, rand_usize()).await?;
                moved.push((*id, nid, set, entry.added_ms));
                report.objects_rebuilt += 1;
            } else if let Some(new) = self.repair_set(&entry.manifest_id(id), at, &writable).await? {
                moved.push((*id, entry.manifest_id(id), new, entry.added_ms));
                report.objects_rebuilt += 1;
            }
        }
        progress(live.len(), live.len());
        if !moved.is_empty() {
            // The file keeps its id; its entry points at the manifest's new place.
            self.change_index(move |idx| {
                for (file, manifest, set, added) in &moved {
                    idx.entries.entry(*file).or_insert_with(|| IndexEntry::new(*added, None)).replace(file, *manifest, set.clone());
                }
            })
            .await?;
        } else if let Some(r) = self.state().await.index {
            // The index object itself may have lost copies.
            if let Some(new) = self.repair_set(&r.id, &r.shards, &writable).await? {
                report.objects_rebuilt += 1;
                self.update(move |s| s.index = Some(StoredRef { id: r.id, shards: new.clone() })).await?;
            }
        }
        // Bring every node in the pool up to the current head.
        self.update(|_| {}).await?;
        info!(?report, "repair finished");
        Ok(report)
    }
}

pub(crate) fn rand_usize() -> usize {
    let mut b = [0u8; 8];
    getrandom::fill(&mut b).expect("OS randomness");
    usize::from_le_bytes(b)
}
