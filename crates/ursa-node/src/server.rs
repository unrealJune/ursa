//! The node's side of `ursa/node/1`.

use std::sync::Arc;

use anyhow::{Result, anyhow, bail};
use iroh::endpoint::{Connection, RecvStream, SendStream};
use iroh::protocol::{AcceptError, ProtocolHandler};
use tokio::sync::RwLock;
use tracing::{debug, info, warn};
use ursa_proto::{MAX_OBJECT, NodeInfo, Request, Response, read_body, read_header, write_header};

use crate::store::Store;

#[derive(Debug, Clone)]
pub struct Node {
    inner: Arc<Inner>,
}

struct Inner {
    store: Store,
    /// Endpoint ids (hex) allowed to use the node. Mirrors `node/owners.json` in the store.
    owners: RwLock<Vec<String>>,
    claim_code: String,
}

impl std::fmt::Debug for Inner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Node").field("store", &self.store.describe()).finish_non_exhaustive()
    }
}

impl Node {
    pub async fn new(store: Store, claim_code: String) -> Result<Self> {
        let owners = store.owners().await?;
        Ok(Self { inner: Arc::new(Inner { store, owners: RwLock::new(owners), claim_code }) })
    }

    pub async fn claimed(&self) -> bool {
        !self.inner.owners.read().await.is_empty()
    }

    async fn is_owner(&self, who: &str) -> bool {
        self.inner.owners.read().await.iter().any(|o| o == who)
    }

    async fn handle(&self, who: &str, send: &mut SendStream, recv: &mut RecvStream) -> Result<()> {
        let req: Request = read_header(recv).await?;
        let owner = self.is_owner(who).await;
        let store = &self.inner.store;
        let resp = match req {
            Request::Info => Response {
                info: Some(NodeInfo {
                    version: env!("CARGO_PKG_VERSION").to_string(),
                    backend: store.kind().to_string(),
                    claimed: self.claimed().await,
                    owner,
                }),
                ..Response::ok()
            },
            Request::Claim { code } => {
                if !constant_time_eq(code.as_bytes(), self.inner.claim_code.as_bytes()) {
                    warn!(%who, "claim with a wrong code");
                    Response::err("wrong claim code")
                } else {
                    let mut owners = self.inner.owners.write().await;
                    if !owners.iter().any(|o| o == who) {
                        let mut next = owners.clone();
                        next.push(who.to_string());
                        store.set_owners(&next).await?;
                        *owners = next;
                        info!(%who, "claimed: a new owner");
                    }
                    Response::ok()
                }
            }
            _ if !owner => Response::err("not an owner of this node"),
            Request::Put { id, len } => {
                if len > MAX_OBJECT {
                    Response::err(format!("object of {len} bytes exceeds {MAX_OBJECT}"))
                } else {
                    let body = read_body(recv, len).await?;
                    if !id.matches(&body) {
                        Response::err(format!("bytes do not hash to {id}"))
                    } else {
                        store.put(&id, body).await?;
                        Response::ok()
                    }
                }
            }
            Request::Get { id } => match store.get(&id).await? {
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
                    have.push(store.has(id).await?);
                }
                Response { have, ..Response::ok() }
            }
            Request::IndexAdd { id } => {
                if !store.has(&id).await? {
                    Response::err(format!("manifest {id} is not stored"))
                } else {
                    store.index_add(&id).await?;
                    Response::ok()
                }
            }
            Request::IndexRemove { id } => {
                store.index_remove(&id).await?;
                Response::ok()
            }
            Request::IndexList => Response { ids: store.index_list().await?, ..Response::ok() },
        };
        write_header(send, &resp).await?;
        send.finish()?;
        Ok(())
    }
}

impl ProtocolHandler for Node {
    async fn accept(&self, conn: Connection) -> Result<(), AcceptError> {
        let who = conn.remote_id().to_string();
        debug!(%who, "connection");
        loop {
            let (mut send, mut recv) = match conn.accept_bi().await {
                Ok(s) => s,
                Err(_) => return Ok(()),
            };
            let node = self.clone();
            let who = who.clone();
            tokio::spawn(async move {
                if let Err(e) = node.handle(&who, &mut send, &mut recv).await {
                    debug!(%who, "request failed: {e:#}");
                    let _ = write_header(&mut send, &Response::err(format!("{e:#}"))).await;
                    let _ = send.finish();
                }
            });
        }
    }
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
        return Err(anyhow!("a claim code cannot contain '@' or spaces"));
    }
    Ok(())
}
