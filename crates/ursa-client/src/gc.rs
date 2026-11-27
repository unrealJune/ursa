//! Getting rid of pieces nothing uses, without ever losing one that something does.
//!
//! Pieces become garbage when a switch or an upload stops part-way (written, never committed),
//! when a file is deleted, and when the index or a manifest is rewritten. Three layers:
//!
//! - **Fixed placement**: a re-code puts an object's pieces where its id says, so running it
//!   again after a stop re-uses the pieces it already wrote instead of writing new ones.
//! - **A journal** on this device of pieces written and not yet committed. A re-code that
//!   starts again deletes what the last run left that nothing references.
//! - **A sweep** ([`Session::collect_garbage`]) lists every node, and deletes pieces no live
//!   file, manifest or index references and that are older than a grace period.
//!
//! Every delete is conditional on the node: it keeps any piece put since the cutoff. Putting a
//! piece that exists refreshes its time, so a writer that is about to reference a piece (another
//! device's upload, a concurrent switch) cannot lose it to a sweep that read the library before
//! that writer committed.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};
use ursa_core::ObjectId;
use ursa_core::manifest::ShardSet;
use ursa_proto::Role;

use crate::{Session, now_ms, short};

/// A piece: the node that holds it and its id.
pub type Piece = (String, ObjectId);

/// How old an unreferenced piece must be before a sweep deletes it: longer than any upload or
/// switch on another device could take to commit.
pub const DEFAULT_GRACE: Duration = Duration::from_secs(72 * 3600);

/// Deletes go to a node in batches of this many ids.
const DELETE_BATCH: usize = 500;

pub(crate) fn pieces(set: &ShardSet) -> Vec<Piece> {
    set.shards.iter().map(|s| (s.node.clone(), s.id)).collect()
}

/// Where an object's pieces go, from its id: the same every time, so a re-run re-uses them.
pub(crate) fn placement(id: &ObjectId) -> usize {
    u64::from_le_bytes(id.0[..8].try_into().expect("8 bytes")) as usize
}

/// Pieces this device wrote and has not committed yet, kept on disk across restarts.
pub(crate) struct Journal {
    path: PathBuf,
    pub entries: Vec<Piece>,
}

impl Journal {
    pub fn open(path: PathBuf) -> Self {
        let entries = std::fs::read(&path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        Self { path, entries }
    }

    pub fn add(&mut self, p: impl IntoIterator<Item = Piece>) {
        self.entries.extend(p);
        self.save();
    }

    pub fn remove(&mut self, done: &HashSet<Piece>) {
        self.entries.retain(|p| !done.contains(p));
        self.save();
    }

    fn save(&self) {
        let tmp = self.path.with_extension("tmp");
        let ok = serde_json::to_vec(&self.entries).ok().is_some_and(|b| std::fs::write(&tmp, b).is_ok());
        if !ok || std::fs::rename(&tmp, &self.path).is_err() {
            warn!("could not save the journal at {}", self.path.display());
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct GcReport {
    pub nodes_swept: usize,
    /// Nodes that could not be listed (down, or too old to list), left as they are.
    pub nodes_skipped: Vec<String>,
    pub objects: usize,
    pub referenced: usize,
    /// Unreferenced but younger than the grace period.
    pub kept_young: usize,
    pub deleted: u64,
    pub bytes_freed: u64,
}

impl Session {
    pub(crate) fn journal(&self) -> Journal {
        Journal::open(self.device.data_dir.join(format!("journal-{}.json", self.account_id())))
    }

    /// Every piece the library references now: the index, and each live file's manifest,
    /// chunks and thumbnail. `None` if any part cannot be read or accounted for, in which case
    /// nothing may be deleted.
    pub(crate) async fn referenced(&self) -> Result<Option<(Option<ObjectId>, HashSet<Piece>)>> {
        let state = self.state().await;
        let mut refs = HashSet::new();
        let Some(index) = &state.index else { return Ok(Some((None, refs))) };
        refs.extend(pieces(&index.shards));
        let idx = self.index().await?;
        for (id, e) in idx.entries.iter().filter(|(_, e)| !e.deleted) {
            // An entry from before coded storage, or one we cannot open: we cannot tell what
            // it uses.
            let Some(at) = &e.at else { return Ok(None) };
            let Ok(m) = self.manifest(id, e).await else { return Ok(None) };
            refs.extend(pieces(at));
            for c in m.chunks.iter().chain(m.thumb.iter()) {
                let Some(set) = &c.shards else { return Ok(None) };
                refs.extend(pieces(set));
            }
        }
        Ok(Some((Some(index.id), refs)))
    }

    /// Deletes `candidates`, each only if its node has not had it put since `older_than_ms`.
    /// Returns how many went and their bytes, and the pieces whose node answered (deleted, kept
    /// as too new, or already gone).
    pub(crate) async fn delete_pieces(
        &self,
        candidates: impl IntoIterator<Item = Piece>,
        older_than_ms: i64,
    ) -> (u64, u64, HashSet<Piece>) {
        let mut by_node: HashMap<String, Vec<ObjectId>> = HashMap::new();
        for (n, id) in candidates {
            by_node.entry(n).or_default().push(id);
        }
        let state = self.state().await;
        let (mut deleted, mut freed, mut handled) = (0, 0, HashSet::new());
        for (node, ids) in by_node {
            let r = self.node_ref(&node, &state);
            for batch in ids.chunks(DELETE_BATCH) {
                match async { self.node(&r).await?.delete(batch.to_vec(), Some(older_than_ms)).await }.await {
                    Ok((n, bytes)) => {
                        deleted += n;
                        freed += bytes;
                        handled.extend(batch.iter().map(|id| (node.clone(), *id)));
                    }
                    Err(e) => {
                        warn!(node = %short(&node), "could not delete {} pieces: {e:#}", batch.len());
                        break;
                    }
                }
            }
        }
        (deleted, freed, handled)
    }

    /// Reads the newest head the nodes offer, and refuses one older than this device has seen
    /// (the nodes holding the newest are slow or down): a sweep must not judge by an old library.
    async fn fresh_head(&self) -> Result<()> {
        let seen = self.seq().await;
        self.load_head().await?;
        if self.seq().await < seen {
            bail!(
                "the nodes that answered hold an older copy of the account than this device has seen; try again when they are all up"
            );
        }
        Ok(())
    }

    /// Lists every reachable node and deletes this account's pieces that nothing references and
    /// that are older than `grace`. Refuses if any file cannot be accounted for.
    pub async fn collect_garbage(&self, grace: Duration) -> Result<GcReport> {
        self.fresh_head().await?;
        let cutoff = now_ms() - grace.as_millis() as i64;
        let Some((index, mut refs)) = self.referenced().await? else {
            bail!(
                "some files could not be read, so it is not safe to tell what is unused; try again when every node is up"
            );
        };
        let mut report = GcReport::default();

        // List each node that holds this account's data.
        let mut listed: Vec<(String, Vec<ursa_proto::StoredObject>)> = Vec::new();
        for p in self.pool().await {
            if !p.reachable || p.info.as_ref().is_none_or(|i| i.role == Role::None) {
                report.nodes_skipped.push(short(&p.id));
                continue;
            }
            let r = self.node_ref(&p.id, &self.state().await);
            match async { self.node(&r).await?.list().await }.await {
                Ok(objs) => listed.push((p.id.clone(), objs)),
                Err(e) => {
                    warn!(node = %short(&p.id), "cannot list: {e:#}");
                    report.nodes_skipped.push(short(&p.id));
                }
            }
        }

        // The library may have moved on while we listed (another device): read it again and
        // count what it references too. Anything it wrote since is protected by the cutoff.
        self.fresh_head().await?;
        if self.state().await.index.as_ref().map(|r| r.id) != index {
            match self.referenced().await? {
                Some((_, newer)) => refs.extend(newer),
                None => bail!("the library changed and could not be read again; try later"),
            }
        }

        let mut candidates = Vec::new();
        for (node, objs) in listed {
            report.nodes_swept += 1;
            for o in objs {
                report.objects += 1;
                let p = (node.clone(), o.id);
                if refs.contains(&p) {
                    report.referenced += 1;
                } else if o.stored_ms >= cutoff {
                    report.kept_young += 1;
                } else {
                    candidates.push(p);
                }
            }
        }
        let (deleted, freed, _) = self.delete_pieces(candidates, cutoff).await;
        report.deleted = deleted;
        report.bytes_freed = freed;
        info!(?report, "garbage collected");
        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placement_is_fixed_by_id() {
        let a = ObjectId::of(b"one");
        assert_eq!(placement(&a), placement(&ObjectId::of(b"one")));
        assert_ne!(placement(&a), placement(&ObjectId::of(b"two")));
    }

    #[test]
    fn the_journal_survives_a_restart() {
        let path = std::env::temp_dir().join(format!("ursa-journal-test-{}.json", crate::library::rand_usize()));
        let (x, y) = (("n1".to_string(), ObjectId::of(b"x")), ("n2".to_string(), ObjectId::of(b"y")));
        let mut j = Journal::open(path.clone());
        j.add([x.clone(), y.clone()]);
        assert_eq!(Journal::open(path.clone()).entries, vec![x.clone(), y.clone()]);
        j.remove(&HashSet::from([x]));
        assert_eq!(Journal::open(path.clone()).entries, vec![y]);
        let _ = std::fs::remove_file(path);
    }
}
