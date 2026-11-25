//! Coded objects: every chunk, thumbnail, manifest and library index is stored as a
//! [`ShardSet`] spread over the pool, and read back from whichever `k` shards answer first.

use anyhow::{Result, anyhow, bail};
use futures_util::StreamExt;
use futures_util::stream::FuturesUnordered;
use tracing::debug;
use ursa_core::ObjectId;
use ursa_core::coding::{self, Code};
use ursa_core::head::NodeRef;
use ursa_core::manifest::{ShardRef, ShardSet};

use crate::{Session, short};

/// How long a fetch waits with nothing arriving before it asks one more node for a spare shard.
const HEDGE_AFTER: std::time::Duration = std::time::Duration::from_secs(2);

/// The code picked when the account has not chosen one: 3-of-5 with five people, fewer
/// pieces in a smaller pool.
pub fn auto_code(domains: usize) -> Code {
    let n = domains.clamp(1, 5);
    Code { k: if n >= 3 { n as u16 - 2 } else { 1 }, n: n as u16 }
}

/// The codes for big objects (chunks) and small ones (manifests, indexes, thumbnails), given the
/// account's choice and how many failure domains are writable now. Small objects are cheap to
/// copy whole: at least three copies, and enough to survive as many nodes down as the chunks.
/// If the pool cannot take the chosen code right now (nodes down), the automatic one is used
/// and a later re-code moves those files to the choice.
pub(crate) fn codes_for(protection: Option<Code>, domains: usize) -> Result<(Code, Code)> {
    if domains == 0 {
        bail!("no node will take this account's data: add a node you own, or a friend's");
    }
    let big = match protection {
        Some(c) if (c.n as usize) <= domains => c,
        Some(c) => {
            debug!(want = %format!("{}-of-{}", c.k, c.n), domains, "not enough nodes for the chosen protection");
            auto_code(domains)
        }
        None => auto_code(domains),
    };
    let copies = (big.n - big.k + 1).max(3).min(domains as u16);
    let small = Code::new(1, copies).map_err(|e| anyhow!("{e}"))?;
    Ok((big, small))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_is_three_of_five_with_five_people() {
        assert_eq!(auto_code(5), Code { k: 3, n: 5 });
        assert_eq!(auto_code(8), Code { k: 3, n: 5 });
        assert_eq!(auto_code(2), Code { k: 1, n: 2 });
    }

    #[test]
    fn a_choice_is_used_when_the_pool_can_take_it() {
        let c = Code { k: 4, n: 6 };
        assert_eq!(codes_for(Some(c), 6).unwrap(), (c, Code { k: 1, n: 3 }));
        // Four copies of small objects survive the same three nodes down as 1-of-4 chunks.
        assert_eq!(codes_for(Some(Code { k: 1, n: 4 }), 6).unwrap().1, Code { k: 1, n: 4 });
        // Too few nodes up: the automatic code.
        assert_eq!(codes_for(Some(c), 5).unwrap().0, Code { k: 3, n: 5 });
        assert!(codes_for(None, 0).is_err());
    }
}

impl Session {
    /// Codes `bytes` and puts shard `i` on `nodes[(rotate + i) % len]`, all in parallel. A node
    /// that refuses is replaced by the next unused one, if any.
    pub(crate) async fn store_object(&self, bytes: &[u8], code: Code, nodes: &[NodeRef], rotate: usize) -> Result<ShardSet> {
        let shards = coding::encode(code, bytes);
        let n = code.n as usize;
        if nodes.len() < n {
            bail!("{} shards need {} nodes; {} are writable", n, n, nodes.len());
        }
        let order: Vec<&NodeRef> = (0..nodes.len()).map(|i| &nodes[(rotate + i) % nodes.len()]).collect();
        let mut spare = order[n..].to_vec();
        let mut placed: Vec<ShardRef> = Vec::with_capacity(n);
        let attempts = futures_util::future::join_all((0..n).map(|i| {
            let node = order[i];
            let shard = &shards[i];
            async move {
                let id = ObjectId::of(shard);
                let r = async { self.node(node).await?.put(id, shard).await }.await;
                (i, id, node, r)
            }
        }))
        .await;
        for (i, id, node, r) in attempts {
            match r {
                Ok(()) => placed.push(ShardRef { i: i as u16, id, node: node.id.clone() }),
                Err(e) => {
                    debug!(node = %short(&node.id), "shard put failed, trying a spare: {e:#}");
                    self.drop_conn(&node.id).await;
                    let mut ok = false;
                    while let Some(alt) = spare.pop() {
                        if async { self.node(alt).await?.put(id, &shards[i]).await }.await.is_ok() {
                            placed.push(ShardRef { i: i as u16, id, node: alt.id.clone() });
                            ok = true;
                            break;
                        }
                    }
                    if !ok {
                        bail!("could not place shard {i}: {e:#}");
                    }
                }
            }
        }
        placed.sort_by_key(|s| s.i);
        Ok(ShardSet { code, sealed_len: bytes.len() as u64, shards: placed })
    }

    pub(crate) fn node_ref(&self, id: &str, state: &ursa_core::head::AccountState) -> NodeRef {
        state.pool().into_iter().find(|n| n.id == id).unwrap_or(NodeRef { id: id.to_string(), ticket: None, label: String::new() })
    }

    /// Reads `k` shards (data shards first, so a healthy read needs no decoding), asking more
    /// nodes as others fail, then rebuilds and checks the object.
    pub(crate) async fn fetch_object(&self, id: &ObjectId, set: &ShardSet) -> Result<Vec<u8>> {
        let state = self.state().await;
        let k = set.code.k as usize;
        let mut queue: Vec<&ShardRef> = set.shards.iter().collect();
        // Data shards first (no decoding needed), and nodes known to be down last.
        queue.sort_by_key(|s| (self.is_down(&s.node), s.i));
        queue.reverse(); // pop() takes the first
        let mut got: Vec<(usize, Vec<u8>)> = Vec::with_capacity(k);
        let mut inflight = FuturesUnordered::new();
        let mut errors = Vec::new();
        let start = |s: &ShardRef| {
            let node = self.node_ref(&s.node, &state);
            let (i, sid) = (s.i as usize, s.id);
            async move { (i, node.id.clone(), async { self.node(&node).await?.get(sid).await }.await) }
        };
        while inflight.len() < k {
            match queue.pop() {
                Some(s) => inflight.push(start(s)),
                None => break,
            }
        }
        loop {
            // A shard that is slow to come (a node that is off, or slow) does not hold the object
            // up: after HEDGE_AFTER with nothing back, one more shard is asked for, and the first
            // k to arrive win.
            let next = match tokio::time::timeout(HEDGE_AFTER, inflight.next()).await {
                Ok(Some(next)) => next,
                Ok(None) => break,
                Err(_) => {
                    if let Some(s) = queue.pop() {
                        inflight.push(start(s));
                    }
                    continue;
                }
            };
            let (i, node, r) = next;
            match r {
                Ok(bytes) => {
                    got.push((i, bytes));
                    if got.len() == k {
                        break;
                    }
                }
                Err(e) => {
                    self.drop_conn(&node).await;
                    errors.push(format!("{}: {e:#}", short(&node)));
                    if let Some(s) = queue.pop() {
                        inflight.push(start(s));
                    }
                }
            }
        }
        if got.len() < k {
            bail!("only {} of the {} shards needed answered ({})", got.len(), k, errors.join("; "));
        }
        let avail: Vec<(usize, &[u8])> = got.iter().map(|(i, b)| (*i, b.as_slice())).collect();
        let bytes = coding::decode(set.code, set.sealed_len as usize, &avail).map_err(|e| anyhow!("{e}"))?;
        if !id.matches(&bytes) {
            bail!("rebuilt object does not match {id}");
        }
        Ok(bytes)
    }

    /// For each shard: does its node still have it? Unreachable counts as no.
    pub(crate) async fn shard_health(&self, set: &ShardSet) -> Vec<bool> {
        let state = self.state().await;
        futures_util::future::join_all(set.shards.iter().map(|s| {
            let node = self.node_ref(&s.node, &state);
            async move {
                match async { self.node(&node).await?.has(vec![s.id]).await }.await {
                    Ok(v) => v.first().copied().unwrap_or(false),
                    Err(_) => false,
                }
            }
        }))
        .await
    }

    /// Rebuilds the shards that are missing onto writable nodes not already holding one of this
    /// set. Returns the repaired set, or `None` if nothing was missing.
    pub(crate) async fn repair_set(&self, id: &ObjectId, set: &ShardSet, writable: &[NodeRef]) -> Result<Option<ShardSet>> {
        let health = self.shard_health(set).await;
        if health.iter().all(|h| *h) {
            return Ok(None);
        }
        let bytes = self.fetch_object(id, set).await?;
        let shards = coding::encode(set.code, &bytes);
        let mut out = set.clone();
        let mut used: Vec<String> = set.shards.iter().zip(&health).filter(|(_, h)| **h).map(|(s, _)| s.node.clone()).collect();
        for (pos, (s, ok)) in set.shards.iter().zip(&health).enumerate() {
            if *ok {
                continue;
            }
            let shard = &shards[s.i as usize];
            let mut placed = false;
            for cand in writable.iter().filter(|n| !used.contains(&n.id)) {
                if async { self.node(cand).await?.put(s.id, shard).await }.await.is_ok() {
                    out.shards[pos].node = cand.id.clone();
                    used.push(cand.id.clone());
                    placed = true;
                    break;
                }
            }
            if !placed {
                bail!("no node left to hold shard {} of {}", s.i, short(&id.to_hex()));
            }
        }
        Ok(Some(out))
    }
}
