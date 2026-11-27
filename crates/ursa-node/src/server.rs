//! The node's side of `ursa/node/2`.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use anyhow::{Result, anyhow, bail};
use iroh::endpoint::{Connection, RecvStream, SendStream};
use iroh::protocol::{AcceptError, ProtocolHandler};
use tokio::sync::{Mutex, RwLock};
use tracing::{debug, info, warn};
use ursa_core::account::AccountId;
use ursa_core::head::Head;
use ursa_proto::{GrantInfo, MAX_OBJECT, NodeInfo, Request, Response, Role, read_body, read_header, write_header};

use crate::store::Store;
use crate::uptime::{HEARTBEAT_MS, Uptime};

#[derive(Debug, Clone)]
pub struct Node {
    inner: Arc<Inner>,
}

struct Inner {
    store: Store,
    node_id: String,
    claim_code: String,
    owners: RwLock<Vec<AccountId>>,
    grants: RwLock<BTreeMap<AccountId, u64>>,
    /// Bytes stored per account. Loaded from a listing at start, kept up to date after.
    usage: Mutex<HashMap<AccountId, u64>>,
    /// Serialises head compare-and-set.
    heads: Mutex<()>,
    uptime: Mutex<Uptime>,
}

impl std::fmt::Debug for Inner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Node").field("store", &self.store.describe()).finish_non_exhaustive()
    }
}

impl Node {
    pub async fn new(store: Store, node_id: String, claim_code: String) -> Result<Self> {
        let owners = store.owners().await?;
        let grants = store.grants().await?;
        let usage = store.usage().await?;
        info!(owners = owners.len(), grants = grants.len(), accounts_with_data = usage.len(), "store opened");
        let mut uptime = store.uptime().await?;
        uptime.start(now_ms());
        store.set_uptime(&uptime).await?;
        let node = Self {
            inner: Arc::new(Inner {
                store,
                node_id,
                claim_code,
                owners: RwLock::new(owners),
                grants: RwLock::new(grants),
                usage: Mutex::new(usage),
                heads: Mutex::new(()),
                uptime: Mutex::new(uptime),
            }),
        };
        // Extend the current span once a minute, so a stop shows as a gap.
        let weak = Arc::downgrade(&node.inner);
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_millis(HEARTBEAT_MS as u64)).await;
                let Some(inner) = weak.upgrade() else { break };
                let u = {
                    let mut u = inner.uptime.lock().await;
                    u.beat(now_ms());
                    u.clone()
                };
                if let Err(e) = inner.store.set_uptime(&u).await {
                    warn!("save uptime: {e:#}");
                }
            }
        });
        Ok(node)
    }

    pub async fn claimed(&self) -> bool {
        !self.inner.owners.read().await.is_empty()
    }

    async fn role(&self, account: &AccountId) -> (Role, Option<u64>) {
        if self.inner.owners.read().await.contains(account) {
            return (Role::Owner, None);
        }
        match self.inner.grants.read().await.get(account) {
            Some(q) => (Role::Granted, Some(*q)),
            None => (Role::None, None),
        }
    }

    async fn info(&self, account: Option<&AccountId>) -> NodeInfo {
        let (role, quota) = match account {
            Some(a) => self.role(a).await,
            None => (Role::None, None),
        };
        let usage = self.inner.usage.lock().await;
        let used = account.and_then(|a| usage.get(a).copied()).unwrap_or(0);
        let grants = if role == Role::Owner {
            self.inner
                .grants
                .read()
                .await
                .iter()
                .map(|(a, q)| GrantInfo {
                    account: *a,
                    quota_bytes: *q,
                    used_bytes: usage.get(a).copied().unwrap_or(0),
                })
                .collect()
        } else {
            Vec::new()
        };
        let up_30d = {
            let mut u = self.inner.uptime.lock().await;
            u.beat(now_ms());
            u.fraction(now_ms())
        };
        let (disk_total_bytes, disk_free_bytes) =
            match (role == Role::Owner).then(|| self.inner.store.disk_space()).flatten() {
                Some((t, f)) => (Some(t), Some(f)),
                None => (None, None),
            };
        NodeInfo {
            version: env!("CARGO_PKG_VERSION").to_string(),
            backend: self.inner.store.kind().to_string(),
            node_id: self.inner.node_id.clone(),
            claimed: self.claimed().await,
            role,
            used_bytes: used,
            quota_bytes: quota,
            grants,
            up_30d,
            disk_total_bytes,
            disk_free_bytes,
        }
    }

    async fn handle(
        &self,
        who: &str,
        session: &Mutex<Option<AccountId>>,
        send: &mut SendStream,
        recv: &mut RecvStream,
    ) -> Result<()> {
        let req: Request = read_header(recv).await?;
        let store = &self.inner.store;

        // Requests that do not need an account.
        match &req {
            Request::Auth { cert } => {
                let resp = if cert.verify_for(who) {
                    *session.lock().await = Some(cert.account.id);
                    debug!(%who, account = %cert.account.id.short(), "authenticated");
                    Response::ok()
                } else {
                    warn!(%who, "a certificate that does not verify for this connection");
                    Response::err("certificate does not verify for this device")
                };
                return reply(send, &resp).await;
            }
            Request::Info => {
                let account = *session.lock().await;
                return reply(send, &Response { info: Some(self.info(account.as_ref()).await), ..Response::ok() })
                    .await;
            }
            _ => {}
        }
        let Some(account) = *session.lock().await else {
            return reply(send, &Response::err("authenticate first")).await;
        };
        let (role, quota) = self.role(&account).await;

        let resp = match req {
            Request::Auth { .. } | Request::Info => unreachable!(),
            Request::Claim { code } => {
                if !constant_time_eq(code.as_bytes(), self.inner.claim_code.as_bytes()) {
                    warn!(%who, "claim with a wrong code");
                    Response::err("wrong claim code")
                } else {
                    let mut owners = self.inner.owners.write().await;
                    if !owners.contains(&account) {
                        let mut next = owners.clone();
                        next.push(account);
                        store.set_owners(&next).await?;
                        *owners = next;
                        info!(account = %account.short(), "claimed: a new owner");
                    }
                    Response::ok()
                }
            }
            Request::Grant { account: to, quota_bytes } => {
                if role != Role::Owner {
                    Response::err("only an owner can grant space")
                } else {
                    let mut grants = self.inner.grants.write().await;
                    let mut next = grants.clone();
                    next.insert(to, quota_bytes);
                    store.set_grants(&next).await?;
                    *grants = next;
                    info!(to = %to.short(), quota_bytes, "granted");
                    Response::ok()
                }
            }
            Request::Revoke { account: from } => {
                if role != Role::Owner {
                    Response::err("only an owner can revoke space")
                } else {
                    let mut grants = self.inner.grants.write().await;
                    let mut next = grants.clone();
                    next.remove(&from);
                    store.set_grants(&next).await?;
                    *grants = next;
                    Response::ok()
                }
            }
            _ if role == Role::None => Response::err("this node has not granted your account any space"),
            Request::Put { id, len } => {
                if len > MAX_OBJECT {
                    Response::err(format!("object of {len} bytes exceeds {MAX_OBJECT}"))
                } else {
                    // Check the quota before reading the body, and again with the real size.
                    let used = self.inner.usage.lock().await.get(&account).copied().unwrap_or(0);
                    if quota.is_some_and(|q| used + len > q) {
                        // Drain nothing: the client sees the refusal and the stream is reset.
                        Response::err(format!("over quota: {used} + {len} > {} bytes", quota.unwrap()))
                    } else {
                        let body = read_body(recv, len).await?;
                        if !id.matches(&body) {
                            Response::err(format!("bytes do not hash to {id}"))
                        } else {
                            let added = store.put(&account, &id, body).await?;
                            *self.inner.usage.lock().await.entry(account).or_default() += added;
                            Response::ok()
                        }
                    }
                }
            }
            Request::Get { id } => match store.get(&account, &id).await? {
                Some(body) => {
                    write_header(send, &Response { len: Some(body.len() as u64), ..Response::ok() }).await?;
                    send.write_all(&body).await?;
                    send.finish()?;
                    return Ok(());
                }
                None => Response::err(format!("no object {id}")),
            },
            Request::Has { ids } => {
                let mut have = Vec::with_capacity(ids.len());
                for id in &ids {
                    have.push(store.has(&account, id).await?);
                }
                Response { have, ..Response::ok() }
            }
            Request::Delete { ids, older_than_ms } => {
                let (mut freed, mut deleted) = (0, 0);
                for id in &ids {
                    if let Some(f) = store.delete(&account, id, older_than_ms).await? {
                        freed += f;
                        deleted += 1;
                    }
                }
                let mut usage = self.inner.usage.lock().await;
                let u = usage.entry(account).or_default();
                *u = u.saturating_sub(freed);
                Response { deleted: Some(deleted), freed: Some(freed), ..Response::ok() }
            }
            Request::List { after, limit } => {
                let objects = store.list(&account, after, limit.min(ursa_proto::LIST_PAGE) as usize).await?;
                Response { objects, ..Response::ok() }
            }
            Request::HeadGet => {
                let head = match store.head(&account).await? {
                    Some(b) => Some(Box::new(Head::from_json(&b).map_err(|e| anyhow!("stored head: {e}"))?)),
                    None => None,
                };
                Response { head, ..Response::ok() }
            }
            Request::HeadPut { head, expected_seq } => {
                if head.account.id != account || !head.verify() {
                    Response::err("head is not signed by your account")
                } else {
                    let _guard = self.inner.heads.lock().await;
                    let current = match store.head(&account).await? {
                        Some(b) => Some(Head::from_json(&b).map_err(|e| anyhow!("stored head: {e}"))?),
                        None => None,
                    };
                    let current_seq = current.as_ref().map(|h| h.seq);
                    if current_seq != expected_seq || current_seq.is_some_and(|s| head.seq <= s) {
                        Response {
                            conflict: current.map(Box::new),
                            error: Some("head changed".into()),
                            ..Default::default()
                        }
                    } else {
                        store.set_head(&account, head.to_json()).await?;
                        debug!(account = %account.short(), seq = head.seq, "head stored");
                        Response::ok()
                    }
                }
            }
        };
        reply(send, &resp).await
    }
}

async fn reply(send: &mut SendStream, resp: &Response) -> Result<()> {
    write_header(send, resp).await?;
    send.finish()?;
    Ok(())
}

impl ProtocolHandler for Node {
    async fn accept(&self, conn: Connection) -> Result<(), AcceptError> {
        let who = conn.remote_id().to_string();
        debug!(%who, "connection");
        let session: Arc<Mutex<Option<AccountId>>> = Arc::new(Mutex::new(None));
        loop {
            let (mut send, mut recv) = match conn.accept_bi().await {
                Ok(s) => s,
                Err(_) => return Ok(()),
            };
            let node = self.clone();
            let who = who.clone();
            let session = session.clone();
            tokio::spawn(async move {
                if let Err(e) = node.handle(&who, &session, &mut send, &mut recv).await {
                    debug!(%who, "request failed: {e:#}");
                    let _ = write_header(&mut send, &Response::err(format!("{e:#}"))).await;
                    let _ = send.finish();
                }
            });
        }
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// A claim code: 20 random characters from an unambiguous alphabet (~100 bits).
pub fn new_claim_code() -> String {
    const ALPHABET: &[u8] = b"23456789abcdefghjkmnpqrstuvwxyz";
    let mut bytes = [0u8; 20];
    getrandom::fill(&mut bytes).expect("OS randomness");
    bytes.iter().map(|b| ALPHABET[*b as usize % ALPHABET.len()] as char).collect()
}

pub fn check_claim_code(code: &str) -> Result<()> {
    if code.len() < 16 {
        bail!("a claim code must be at least 16 characters");
    }
    if code.contains('@') || code.contains(char::is_whitespace) {
        bail!("a claim code cannot contain '@' or spaces");
    }
    Ok(())
}
