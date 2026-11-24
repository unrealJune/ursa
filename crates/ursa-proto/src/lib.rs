//! The protocol between an Ursa client and a node, over iroh.
//!
//! One bidirectional QUIC stream per request. Each side sends a length-prefixed JSON header
//! (`u32` big-endian length, then JSON); a `put` request is followed by exactly `len` bytes of
//! object, and a successful `get` response by exactly `len` bytes. Streams are read to an exact
//! length, never "to the end" (see Street Cryptid's noq `read_to_end` patch).
//!
//! A node stores opaque objects named by their BLAKE3 hash and checks every `put` against it. It
//! keeps one index: the set of manifest ids that make up its owner's library. Every request but
//! `info` and `claim` requires the caller to be an owner; `claim` with the node's claim code
//! makes the caller one.

use std::str::FromStr;

use anyhow::{Context, Result, anyhow, bail};
use iroh::endpoint::{Connection, RecvStream, SendStream};
use iroh::{Endpoint, EndpointAddr};
use serde::{Deserialize, Serialize};
use ursa_core::ObjectId;

pub const ALPN: &[u8] = b"ursa/node/1";
/// Headers are small; this bounds what a peer can make us allocate before we have checked it.
pub const MAX_HEADER: usize = 1 << 20;
/// The largest object a node accepts: a 4 MiB chunk plus its tag, or a manifest, with room.
pub const MAX_OBJECT: u64 = 32 << 20;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Request {
    Info,
    Claim { code: String },
    Put { id: ObjectId, len: u64 },
    Get { id: ObjectId },
    Has { ids: Vec<ObjectId> },
    IndexAdd { id: ObjectId },
    IndexRemove { id: ObjectId },
    IndexList,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Response {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// For `get`: the length of the object that follows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub len: Option<u64>,
    /// For `index_list`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ids: Vec<ObjectId>,
    /// For `has`, in request order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub have: Vec<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub info: Option<NodeInfo>,
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
    pub claimed: bool,
    /// Whether the caller is one of the owners.
    pub owner: bool,
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
// Claim links

/// What a node prints for its owner's first device: `ursa-claim:<code>@<endpoint ticket>`.
#[derive(Debug, Clone)]
pub struct ClaimLink {
    pub code: String,
    pub addr: EndpointAddr,
}

impl ClaimLink {
    pub fn new(code: String, addr: EndpointAddr) -> Self {
        Self { code, addr }
    }

    pub fn ticket(&self) -> String {
        iroh_tickets::endpoint::EndpointTicket::new(self.addr.clone()).to_string()
    }
}

impl std::fmt::Display for ClaimLink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ursa-claim:{}@{}", self.code, self.ticket())
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

/// A connection to one node.
#[derive(Clone)]
pub struct NodeClient {
    conn: Connection,
}

impl NodeClient {
    pub async fn connect(endpoint: &Endpoint, addr: EndpointAddr) -> Result<Self> {
        let conn = endpoint.connect(addr, ALPN).await.context("connect to node")?;
        Ok(Self { conn })
    }

    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    /// `true` once the connection has closed; the caller should reconnect.
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
        if !resp.ok {
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

    pub async fn index_add(&self, id: ObjectId) -> Result<()> {
        self.call(&Request::IndexAdd { id }, None).await.map(|_| ())
    }

    pub async fn index_remove(&self, id: ObjectId) -> Result<()> {
        self.call(&Request::IndexRemove { id }, None).await.map(|_| ())
    }

    pub async fn index_list(&self) -> Result<Vec<ObjectId>> {
        Ok(self.call(&Request::IndexList, None).await?.0.ids)
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
