//! Protection and space: which code the account's files use, what each choice lets it store,
//! what it stores and gives now, and moving every file to a new choice (re-coding).

use std::collections::{BTreeMap, HashMap, HashSet};

use anyhow::{Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};
use ursa_core::ObjectId;
use ursa_core::coding::Code;
use ursa_core::head::IndexEntry;
use ursa_core::manifest::{self, FileManifest, ShardSet};
use ursa_proto::Role;

use crate::capacity::{self, Choice, UNKNOWN_UP};
use crate::library::rand_usize;
use crate::objects::{auto_code, codes_for};
use crate::{Session, now_ms, short};

/// Files re-coded between index commits. A commit is what lets the old pieces go.
const RECODE_BATCH: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Shape {
    pub k: u16,
    pub n: u16,
}

impl From<Code> for Shape {
    fn from(c: Code) -> Self {
        Self { k: c.k, n: c.n }
    }
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Kinds {
    pub photos: u64,
    pub videos: u64,
    pub files: u64,
}

/// A person in the pool (you, or a friend), as one failure domain.
#[derive(Clone, Debug, Serialize)]
pub struct Member {
    pub name: String,
    pub own: bool,
    pub nodes: usize,
    pub reachable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub up_30d: Option<f64>,
    /// What this account stores on the member's nodes.
    pub used_bytes: u64,
    /// What the member gives this account (`None` for your own nodes).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gives_you: Option<u64>,
}

/// A friend's use of the space you give them.
#[derive(Clone, Debug, Serialize)]
pub struct Hosted {
    pub name: String,
    pub account: String,
    pub used_bytes: u64,
    pub quota_bytes: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct Disk {
    pub total_bytes: u64,
    pub free_bytes: u64,
    /// This account's pieces.
    pub yours_bytes: u64,
    /// Friends' pieces.
    pub friends_bytes: u64,
    /// Given to friends and not used by them yet.
    pub reserved_bytes: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct Month {
    /// `YYYY-MM`, UTC.
    pub month: String,
    /// Bytes of files stored by the end of the month (files deleted since are not counted).
    pub bytes: u64,
}

/// Everything the Pool and Usage screens show.
#[derive(Clone, Debug, Serialize)]
pub struct StorageReport {
    /// The code new files use: the account's choice, or the automatic one.
    pub protection: Shape,
    pub chosen: bool,
    pub files: usize,
    /// Files not yet in `protection` (a switch not finished, or uploads while nodes were down).
    pub pending_files: usize,
    pub stored_bytes: u64,
    pub kinds: Kinds,
    /// Chunk objects by code, `k-of-n` → count.
    pub codes: BTreeMap<String, usize>,
    pub limit_bytes: u64,
    pub choices: Vec<Choice>,
    /// What you give friends, summed over them.
    pub give_bytes: u64,
    /// Your pieces on your own nodes and on friends'.
    pub home_bytes: u64,
    pub friends_bytes: u64,
    pub members: Vec<Member>,
    pub hosting: Vec<Hosted>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disk: Option<Disk>,
    pub growth: Vec<Month>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct RecodeReport {
    pub files: usize,
    pub recoded_files: usize,
    pub objects_recoded: usize,
    pub shards_deleted: usize,
    pub failures: Vec<String>,
}

fn code_name(c: Code) -> String {
    format!("{}-of-{}", c.k, c.n)
}

impl Session {
    /// Chooses the code for the account's files (`None`: automatic). Files already stored keep
    /// theirs until [`Session::recode`] runs.
    pub async fn set_protection(&self, shape: Option<Shape>) -> Result<()> {
        let code = match shape {
            Some(s) => {
                let c = Code::new(s.k, s.n).map_err(|_| anyhow!("{}-of-{} is not a valid code", s.k, s.n))?;
                if c.n <= c.k {
                    bail!("{}-of-{} has no spare pieces: nothing could go missing", c.k, c.n);
                }
                Some(c)
            }
            None => None,
        };
        self.update(move |s| s.protection = code).await?;
        Ok(())
    }

    /// Gives every friend an equal share of `total_bytes` on every node this account owns.
    pub async fn set_give(&self, total_bytes: u64) -> Result<()> {
        let state = self.state().await;
        if state.friends.is_empty() {
            bail!("add a friend first");
        }
        let each = total_bytes / state.friends.len() as u64;
        let mut errors = Vec::new();
        for n in &state.home_nodes {
            match self.node(n).await {
                Ok(c) => {
                    for f in &state.friends {
                        if let Err(e) = c.grant(f.account, each).await {
                            errors.push(format!("{}: {e:#}", short(&n.id)));
                        }
                    }
                }
                Err(e) => errors.push(format!("{}: {e:#}", short(&n.id))),
            }
        }
        if !errors.is_empty() {
            bail!("could not update every node: {}", errors.join("; "));
        }
        Ok(())
    }

    async fn live_manifests(&self) -> Result<(Vec<(ObjectId, IndexEntry, FileManifest)>, Vec<String>)> {
        let idx = self.index().await?;
        let mut out = Vec::new();
        let mut failed = Vec::new();
        for (id, e) in idx.entries.iter().filter(|(_, e)| !e.deleted && e.at.is_some()) {
            match self.manifest(id, e).await {
                Ok(m) => out.push((*id, e.clone(), m)),
                Err(err) => failed.push(format!("{}: {err:#}", id.to_hex())),
            }
        }
        Ok((out, failed))
    }

    /// The report, with `extra` among the choices too (a shape being considered).
    pub async fn storage(&self, extra: Option<Shape>) -> Result<StorageReport> {
        let state = self.state().await;
        let pool = self.pool().await;

        // Members: one per failure domain, from what the nodes say (or last said).
        let mut by_owner: BTreeMap<String, Vec<&crate::PoolNode>> = BTreeMap::new();
        for p in &pool {
            by_owner.entry(p.owner.clone()).or_default().push(p);
        }
        let mut members = Vec::new();
        let (mut own_used, mut own_cap, mut give, mut hosting, mut disk) = (0u64, None::<u64>, 0u64, Vec::new(), None);
        let mut friend_caps = Vec::new();
        let mut ups = Vec::new();
        for (owner, nodes) in &by_owner {
            let own = owner == "home";
            let infos: Vec<_> = nodes.iter().filter_map(|p| p.info.as_ref()).collect();
            let takes = infos.iter().any(|i| i.role != Role::None);
            if !takes {
                continue;
            }
            let used: u64 = infos.iter().map(|i| i.used_bytes).sum();
            let up = infos.iter().filter_map(|i| i.up_30d).fold(None, |a: Option<f64>, u| Some(a.map_or(u, |a| a.max(u))));
            let gives_you = (!own).then(|| infos.iter().filter_map(|i| i.quota_bytes).max().unwrap_or(0));
            ups.push(up.unwrap_or(UNKNOWN_UP));
            if own {
                own_used = used;
                // Grants and disk from the first home node that reported them.
                if let Some(i) = infos.iter().find(|i| i.role == Role::Owner) {
                    give = i.grants.iter().map(|g| g.quota_bytes).sum();
                    let friends_used: u64 = i.grants.iter().map(|g| g.used_bytes).sum();
                    let reserved: u64 = i.grants.iter().map(|g| g.quota_bytes.saturating_sub(g.used_bytes)).sum();
                    for g in &i.grants {
                        let name = state.friends.iter().find(|f| f.account == g.account).map(|f| f.name.clone()).unwrap_or_else(|| g.account.short());
                        hosting.push(Hosted { name, account: g.account.to_string(), used_bytes: g.used_bytes, quota_bytes: g.quota_bytes });
                    }
                    if let (Some(total), Some(free)) = (i.disk_total_bytes, i.disk_free_bytes) {
                        disk = Some(Disk { total_bytes: total, free_bytes: free, yours_bytes: i.used_bytes, friends_bytes: friends_used, reserved_bytes: reserved });
                        own_cap = Some((free + i.used_bytes).saturating_sub(reserved));
                    }
                }
                if own_cap.is_none() {
                    // A bucket, or a node that has not said: treat as unbounded.
                    own_cap = Some(u64::MAX / 4);
                }
            } else {
                friend_caps.push(gives_you.unwrap_or(0));
            }
            members.push(Member {
                name: if own { state.name.clone() } else { owner.clone() },
                own,
                nodes: nodes.len(),
                reachable: nodes.iter().any(|p| p.reachable),
                up_30d: up,
                used_bytes: used,
                gives_you,
            });
        }
        members.sort_by_key(|m| !m.own);
        let friends_bytes = members.iter().filter(|m| !m.own).map(|m| m.used_bytes).sum();
        let domains = members.len();

        let protection = state.protection.unwrap_or_else(|| auto_code(domains));
        let mut choices = capacity::choices(protection, own_cap, &friend_caps, give, &ups);
        if let Some(c) = extra.and_then(|s| Code::new(s.k, s.n).ok()).filter(|c| c.n > c.k)
            && !choices.iter().any(|x| (x.k, x.n) == (c.k, c.n))
        {
            choices.extend(capacity::choices(c, own_cap, &friend_caps, give, &ups).into_iter().filter(|x| (x.k, x.n) == (c.k, c.n)).map(|mut x| {
                x.recommended = false;
                x
            }));
        }
        let limit_bytes = capacity::limit(protection, own_cap, &friend_caps, give);

        // The library.
        let (files, _) = self.live_manifests().await?;
        let mut kinds = Kinds::default();
        let mut codes: BTreeMap<String, usize> = BTreeMap::new();
        let mut pending_files = 0;
        let mut added: Vec<(i64, u64)> = Vec::new();
        for (_, e, m) in &files {
            if m.mime.starts_with("image/") {
                kinds.photos += m.size;
            } else if m.mime.starts_with("video/") {
                kinds.videos += m.size;
            } else {
                kinds.files += m.size;
            }
            let mut pending = false;
            for c in &m.chunks {
                if let Some(s) = &c.shards {
                    *codes.entry(code_name(s.code)).or_default() += 1;
                    pending |= s.code != protection;
                }
            }
            pending_files += pending as usize;
            added.push((e.added_ms, m.size));
        }
        let stored_bytes = kinds.photos + kinds.videos + kinds.files;

        Ok(StorageReport {
            protection: protection.into(),
            chosen: state.protection.is_some(),
            files: files.len(),
            pending_files,
            stored_bytes,
            kinds,
            codes,
            limit_bytes,
            choices,
            give_bytes: give,
            home_bytes: own_used,
            friends_bytes,
            members,
            hosting,
            disk,
            growth: growth(&added, now_ms(), 12),
        })
    }

    /// Moves every file to the account's protection: re-codes chunks, thumbnails and manifests
    /// that use another code, commits the index in batches, and after each commit deletes the
    /// old pieces nothing references any more. Safe to stop and run again.
    pub async fn recode(&self, progress: impl Fn(u64, u64)) -> Result<RecodeReport> {
        let writable = self.writable().await;
        let state = self.state().await;
        let target = state.protection.unwrap_or_else(|| auto_code(writable.len()));
        if (target.n as usize) > writable.len() {
            bail!("{} needs {} nodes; {} are reachable now", code_name(target), target.n, writable.len());
        }
        let (big, small) = codes_for(Some(target), writable.len())?;
        let (files, failed) = self.live_manifests().await?;
        let mut report = RecodeReport { files: files.len() + failed.len(), failures: failed, ..Default::default() };
        // Without every manifest we cannot know which pieces are shared, so nothing is deleted.
        let may_delete = report.failures.is_empty();

        // Who references each piece (node, shard id) now.
        let mut refs: HashMap<(String, ObjectId), usize> = HashMap::new();
        let pieces = |set: &ShardSet| set.shards.iter().map(|s| (s.node.clone(), s.id)).collect::<Vec<_>>();
        for (_, e, m) in &files {
            for set in m.chunks.iter().chain(m.thumb.iter()).filter_map(|c| c.shards.as_ref()).chain(e.at.iter()) {
                for p in pieces(set) {
                    *refs.entry(p).or_default() += 1;
                }
            }
        }

        let todo: Vec<_> = files
            .into_iter()
            .filter(|(_, e, m)| {
                m.chunks.iter().any(|c| c.shards.as_ref().is_some_and(|s| s.code != big))
                    || m.thumb.as_ref().and_then(|t| t.shards.as_ref()).is_some_and(|s| s.code != small)
                    || e.at.as_ref().is_some_and(|s| s.code != small)
            })
            .collect();
        let total: u64 = todo.iter().map(|(_, _, m)| m.size).sum();
        let mut done = 0u64;
        progress(0, total);
        info!(files = todo.len(), to = %code_name(big), "re-coding");

        let mut recoded: HashMap<ObjectId, ShardSet> = HashMap::new();
        let mut kept: HashSet<(String, ObjectId)> = HashSet::new();
        for batch in todo.chunks(RECODE_BATCH) {
            let mut moved: Vec<(ObjectId, ObjectId, ShardSet, i64)> = Vec::new();
            let mut released: Vec<(String, ObjectId)> = Vec::new();
            for (id, e, m) in batch {
                let mut m = m.clone();
                let r: Result<()> = async {
                    let mut refs_out: Vec<&mut manifest::ChunkRef> = m.chunks.iter_mut().collect();
                    let thumb = m.thumb.as_mut();
                    let n_chunks = refs_out.len();
                    if let Some(t) = thumb {
                        refs_out.push(t);
                    }
                    for (i, c) in refs_out.into_iter().enumerate() {
                        let want = if i < n_chunks { big } else { small };
                        let Some(old) = c.shards.clone() else { continue };
                        if old.code == want {
                            continue;
                        }
                        let new = match recoded.get(&c.id) {
                            Some(s) if s.code == want => s.clone(),
                            _ => {
                                let bytes = self.fetch_object(&c.id, &old).await?;
                                let s = self.store_object(&bytes, want, &writable, rand_usize()).await?;
                                recoded.insert(c.id, s.clone());
                                report.objects_recoded += 1;
                                s
                            }
                        };
                        kept.extend(pieces(&new));
                        released.extend(pieces(&old));
                        c.shards = Some(new);
                        if i < n_chunks {
                            done += c.len;
                            progress(done.min(total), total);
                        }
                    }
                    Ok(())
                }
                .await;
                if let Err(err) = r {
                    warn!(file = %m.name, "re-code failed: {err:#}");
                    report.failures.push(format!("{}: {err:#}", m.name));
                    continue;
                }
                let (nid, sealed) = manifest::seal(self.account.root(), &m);
                let at = self.store_object(&sealed, small, &writable, rand_usize()).await?;
                kept.extend(pieces(&at));
                if let Some(old) = &e.at {
                    released.extend(pieces(old));
                }
                moved.push((*id, nid, at, e.added_ms));
                report.objects_recoded += 1;
                report.recoded_files += 1;
            }
            if moved.is_empty() {
                continue;
            }
            self.change_index(move |idx| {
                for (file, manifest, set, added) in &moved {
                    idx.entries.entry(*file).or_insert_with(|| IndexEntry::new(*added, None)).replace(file, *manifest, set.clone());
                }
            })
            .await?;
            if may_delete {
                report.shards_deleted += self.release(&mut refs, &kept, released).await;
            }
        }
        progress(total, total);
        info!(?report, "re-code finished");
        Ok(report)
    }

    /// Drops one reference to each piece; deletes the pieces nobody references any more (and
    /// that no new set reuses). Returns how many were deleted. A node that cannot be reached
    /// keeps its old pieces until its lease runs out.
    async fn release(&self, refs: &mut HashMap<(String, ObjectId), usize>, kept: &HashSet<(String, ObjectId)>, released: Vec<(String, ObjectId)>) -> usize {
        let mut by_node: HashMap<String, Vec<ObjectId>> = HashMap::new();
        for p in released {
            let r = refs.entry(p.clone()).or_insert(1);
            *r = r.saturating_sub(1);
            if *r == 0 && !kept.contains(&p) {
                refs.remove(&p);
                by_node.entry(p.0).or_default().push(p.1);
            }
        }
        let state = self.state().await;
        let mut deleted = 0;
        for (node, ids) in by_node {
            let r = self.node_ref(&node, &state);
            let n = ids.len();
            match async { self.node(&r).await?.delete(ids).await }.await {
                Ok(()) => deleted += n,
                Err(e) => warn!(node = %short(&node), "could not delete {n} old pieces: {e:#}"),
            }
        }
        deleted
    }
}

/// Bytes stored at the end of each of the last `months` months (oldest first), from when each
/// live file was added.
fn growth(added: &[(i64, u64)], now: i64, months: usize) -> Vec<Month> {
    let (mut y, mut m) = year_month(now);
    let mut ends = Vec::with_capacity(months);
    for _ in 0..months {
        ends.push((y, m));
        (y, m) = if m == 1 { (y - 1, 12) } else { (y, m - 1) };
    }
    ends.reverse();
    ends.into_iter()
        .map(|(y, m)| {
            let (ny, nm) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
            let end = days_from_civil(ny, nm, 1) * 86_400_000;
            Month { month: format!("{y:04}-{m:02}"), bytes: added.iter().filter(|(t, _)| *t < end).map(|(_, s)| s).sum() }
        })
        .collect()
}

fn year_month(ms: i64) -> (i64, u32) {
    let (y, m, _) = civil_from_days(ms.div_euclid(86_400_000));
    (y, m)
}

// Howard Hinnant's civil calendar algorithms.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + (m <= 2) as i64, m, d)
}

fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = (m as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calendar_round_trips() {
        for z in [-1000i64, 0, 19_000, 20_417, 60_000] {
            let (y, m, d) = civil_from_days(z);
            assert_eq!(days_from_civil(y, m, d), z);
        }
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        // 2026-11-25
        assert_eq!(civil_from_days(days_from_civil(2026, 11, 25)), (2026, 11, 25));
    }

    #[test]
    fn growth_is_cumulative_by_month() {
        let day = 86_400_000;
        let now = days_from_civil(2026, 11, 25) * day;
        let added = [(days_from_civil(2026, 9, 3) * day, 10), (days_from_civil(2026, 11, 1) * day, 5), (days_from_civil(2025, 1, 1) * day, 1)];
        let g = growth(&added, now, 3);
        let got: Vec<_> = g.iter().map(|m| (m.month.as_str(), m.bytes)).collect();
        assert_eq!(got, vec![("2026-09", 11), ("2026-10", 11), ("2026-11", 16)]);
    }
}
