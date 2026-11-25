//! The protocol between Ursa devices and nodes over iroh: `ursa/node/2`.
//!
//! One bidirectional QUIC stream per request. Each side sends a length-prefixed JSON header
//! (`u32` big-endian length, then JSON); a `put` is followed by exactly `len` bytes of object,
//! and a successful `get` response by exactly `len` bytes. Streams are read to an exact length,
//! never "to the end".
//!
//! **Accounts, not devices.** The first request on a connection is `auth` with the device's
//! certificate; the node checks it is signed by the account and names this connection's own
//! endpoint id, and from then on serves that account. Nodes know accounts as
//!
//! - **owners**, who claimed the node with its claim code and may grant others space, and
//! - **granted** accounts (friends), with a byte quota.
//!
//! Objects live in a per-account namespace, named by their BLAKE3 hash and checked on every
//! `put`. Each account also has one **head**, stored with compare-and-set on its sequence number.

use std::str::FromStr;

use anyhow::{Context, Result, anyhow, bail};
use iroh::endpoint::{Connection, RecvStream, SendStream};
use iroh::{Endpoint, EndpointAddr};
use serde::{Deserialize, Serialize};
use ursa_core::ObjectId;
use ursa_core::account::{AccountId, DeviceCert};
use ursa_core::head::Head;

pub mod link;

pub const ALPN: &[u8] = b"ursa/node/2";
/// Headers are small (a head with its keys is ~10 KB); this bounds what a peer can make us
/// allocate before we have checked anything.
pub const MAX_HEADER: usize = 1 << 20;
/// The largest object a node accepts: a coded shard of a 4 MiB chunk, a manifest, an index.
pub const MAX_OBJECT: u64 = 32 << 20;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Request {
    Auth { cert: Box<DeviceCert> },
    Info,
    Claim { code: String },
    Grant { account: AccountId, quota_bytes: u64 },
    Revoke { account: AccountId },
    Put { id: ObjectId, len: u64 },
    Get { id: ObjectId },
    Has { ids: Vec<ObjectId> },
    Delete { ids: Vec<ObjectId> },
    HeadGet,
    /// Stores `head` if the node's current head for the account has `expected_seq` (or there is
    /// none and `expected_seq` is `None`), and `head.seq` is greater.
    HeadPut { head: Box<Head>, expected_seq: Option<u64> },
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Response {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Why a `head_put` lost: the node's current head, so the caller can merge and retry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conflict: Option<Box<Head>>,
    /// For `get`: the length of the object that follows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub len: Option<u64>,
    /// For `has`, in request order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub have: Vec<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub info: Option<NodeInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head: Option<Box<Head>>,
}

impl Response {
    pub fn ok() -> Self {
        Self { ok: true, ..Default::default() }
    }

    pub fn err(e: impl std::fmt::Display) -> Self {
        Self { ok: false, error: Some(e.to_string()), ..Default::default() }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeInfo {
    pub version: String,
    /// Where the node keeps objects: `disk` or `s3`.
    pub backend: String,
    pub node_id: String,
    pub claimed: bool,
    /// The caller's standing with this node.
    pub role: Role,
    /// Bytes the caller's account stores here, and its quota (`None`: unlimited, owners).
    pub used_bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quota_bytes: Option<u64>,
    /// Owners only: every account granted space, with use and quota.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub grants: Vec<GrantInfo>,
    /// Fraction of the last 30 days (or since it first ran) the node was up.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub up_30d: Option<f64>,
    /// Owners only, disk backend: the disk's size and free space.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disk_total_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disk_free_bytes: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    None,
    Owner,
    Granted,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GrantInfo {
    pub account: AccountId,
    pub quota_bytes: u64,
    pub used_bytes: u64,
}

pub async fn write_header<T: Serialize>(send: &mut SendStream, value: &T) -> Result<()> {
    let json = serde_json::to_vec(value)?;
    send.write_all(&(json.len() as u32).to_be_bytes()).await?;
    send.write_all(&json).await?;
    Ok(())
}

pub async fn read_header<T: for<'de> Deserialize<'de>>(recv: &mut RecvStream) -> Result<T> {
    let mut len = [0u8; 4];
    recv.read_exact(&mut len).await.context("read header length")?;
    let len = u32::from_be_bytes(len) as usize;
    if len > MAX_HEADER {
        bail!("header of {len} bytes exceeds {MAX_HEADER}");
    }
    let mut buf = vec![0u8; len];
    recv.read_exact(&mut buf).await.context("read header")?;
    Ok(serde_json::from_slice(&buf)?)
}

pub async fn read_body(recv: &mut RecvStream, len: u64) -> Result<Vec<u8>> {
    if len > MAX_OBJECT {
        bail!("object of {len} bytes exceeds {MAX_OBJECT}");
    }
    let mut buf = vec![0u8; len as usize];
    recv.read_exact(&mut buf).await.context("read object")?;
    Ok(buf)
}

// ---------------------------------------------------------------------------------------------
// Claim links and addresses

/// What a node prints for its owner: `ursa-claim:<code>@<endpoint ticket>`.
#[derive(Debug, Clone)]
pub struct ClaimLink {
    pub code: String,
    pub addr: EndpointAddr,
}

impl ClaimLink {
    pub fn new(code: String, addr: EndpointAddr) -> Self {
        Self { code, addr }
    }
}

impl std::fmt::Display for ClaimLink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ursa-claim:{}@{}", self.code, ticket_of(&self.addr))
    }
}

impl FromStr for ClaimLink {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        let rest = s.trim().strip_prefix("ursa-claim:").ok_or_else(|| anyhow!("not an ursa-claim: link"))?;
        let (code, ticket) = rest.split_once('@').ok_or_else(|| anyhow!("claim link has no ticket"))?;
        Ok(Self { code: code.to_string(), addr: parse_addr(ticket)? })
    }
}

/// An endpoint ticket, or a bare endpoint id (found through discovery).
pub fn parse_addr(s: &str) -> Result<EndpointAddr> {
    let s = s.trim();
    if let Ok(t) = s.parse::<iroh_tickets::endpoint::EndpointTicket>() {
        return Ok(t.endpoint_addr().clone());
    }
    let id: iroh::EndpointId = s.parse().map_err(|e| anyhow!("not a ticket or endpoint id: {e}"))?;
    Ok(EndpointAddr::new(id))
}

pub fn ticket_of(addr: &EndpointAddr) -> String {
    iroh_tickets::endpoint::EndpointTicket::new(addr.clone()).to_string()
}

// ---------------------------------------------------------------------------------------------
// Client

/// An authenticated connection to one node, on behalf of one account.
#[derive(Clone)]
pub struct NodeClient {
    conn: Connection,
}

/// Outcome of a head write.
pub enum HeadPut {
    Stored,
    /// Someone else wrote first; this is the node's current head.
    Conflict(Box<Head>),
}

impl NodeClient {
    /// Connects and authenticates with the device's certificate.
    pub async fn connect(endpoint: &Endpoint, addr: EndpointAddr, cert: &DeviceCert) -> Result<Self> {
        let conn = endpoint.connect(addr, ALPN).await.context("connect to node")?;
        let c = Self { conn };
        c.call(&Request::Auth { cert: Box::new(cert.clone()) }, None).await.context("authenticate")?;
        Ok(c)
    }

    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    pub fn node_id(&self) -> String {
        self.conn.remote_id().to_string()
    }

    pub fn is_closed(&self) -> bool {
        self.conn.close_reason().is_some()
    }

    async fn call(&self, req: &Request, body: Option<&[u8]>) -> Result<(Response, RecvStream)> {
        let (mut send, mut recv) = self.conn.open_bi().await?;
        write_header(&mut send, req).await?;
        if let Some(b) = body {
            send.write_all(b).await?;
        }
        send.finish()?;
        let resp: Response = read_header(&mut recv).await?;
        if !resp.ok && resp.conflict.is_none() {
            bail!("node refused: {}", resp.error.as_deref().unwrap_or("no reason given"));
        }
        Ok((resp, recv))
    }

    pub async fn info(&self) -> Result<NodeInfo> {
        let (r, _) = self.call(&Request::Info, None).await?;
        r.info.ok_or_else(|| anyhow!("node sent no info"))
    }

    pub async fn claim(&self, code: &str) -> Result<()> {
        self.call(&Request::Claim { code: code.to_string() }, None).await.map(|_| ())
    }

    pub async fn grant(&self, account: AccountId, quota_bytes: u64) -> Result<()> {
        self.call(&Request::Grant { account, quota_bytes }, None).await.map(|_| ())
    }

    pub async fn revoke(&self, account: AccountId) -> Result<()> {
        self.call(&Request::Revoke { account }, None).await.map(|_| ())
    }

    pub async fn put(&self, id: ObjectId, bytes: &[u8]) -> Result<()> {
        self.call(&Request::Put { id, len: bytes.len() as u64 }, Some(bytes)).await.map(|_| ())
    }

    /// Fetches an object and checks it is the one asked for.
    pub async fn get(&self, id: ObjectId) -> Result<Vec<u8>> {
        let (r, mut recv) = self.call(&Request::Get { id }, None).await?;
        let len = r.len.ok_or_else(|| anyhow!("node sent no length"))?;
        let body = read_body(&mut recv, len).await?;
        if !id.matches(&body) {
            bail!("node returned the wrong bytes for {id}");
        }
        Ok(body)
    }

    pub async fn has(&self, ids: Vec<ObjectId>) -> Result<Vec<bool>> {
        let n = ids.len();
        let (r, _) = self.call(&Request::Has { ids }, None).await?;
        if r.have.len() != n {
            bail!("node answered {} of {n} has-queries", r.have.len());
        }
        Ok(r.have)
    }

    pub async fn delete(&self, ids: Vec<ObjectId>) -> Result<()> {
        self.call(&Request::Delete { ids }, None).await.map(|_| ())
    }

    pub async fn head_get(&self) -> Result<Option<Head>> {
        Ok(self.call(&Request::HeadGet, None).await?.0.head.map(|h| *h))
    }

    pub async fn head_put(&self, head: &Head, expected_seq: Option<u64>) -> Result<HeadPut> {
        let (r, _) = self.call(&Request::HeadPut { head: Box::new(head.clone()), expected_seq }, None).await?;
        Ok(match r.conflict {
            Some(h) => HeadPut::Conflict(h),
            None => HeadPut::Stored,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_serialize_with_an_op_tag() {
        let id = ObjectId::of(b"x");
        let json = serde_json::to_string(&Request::Get { id }).unwrap();
        assert_eq!(json, format!(r#"{{"op":"get","id":"{id}"}}"#));
        let back: Request = serde_json::from_str(&json).unwrap();
        assert!(matches!(back, Request::Get { id: i } if i == id));
    }

    #[test]
    fn claim_link_round_trips() {
        let id = iroh::SecretKey::from_bytes(&[7u8; 32]).public();
        let link = ClaimLink::new("abc123".into(), EndpointAddr::new(id));
        let parsed: ClaimLink = link.to_string().parse().unwrap();
        assert_eq!(parsed.code, "abc123");
        assert_eq!(parsed.addr.id, id);
    }
}
