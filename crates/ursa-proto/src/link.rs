//! Adding a device to an account: `ursa/link/1`. The same in both directions (Windows to
//! Android, Android to Windows): whichever device has the account offers, the other joins.
//!
//! 1. The offering device makes a one-time **link code**: its endpoint ticket and a 32-byte
//!    secret (`ursa-link:<secret>@<ticket>`), shown as text or a QR code.
//! 2. The joining device dials it, proves it knows the secret, and sends a fresh X-Wing public
//!    key, its name and platform.
//! 3. Both compute a 6-digit confirmation code from the secret and the transcript and show it;
//!    the person checks they match and confirms on the offering device.
//! 4. The offering device seals the account root to the joiner's X-Wing key (post-quantum, unlike
//!    iroh's TLS) and signs a certificate for the joiner's endpoint id.
//!
//! The QUIC connection already authenticates the offering device (its id is in the ticket); the
//! secret authenticates the joiner; the confirmation code catches a leaked link code.

use std::sync::Arc;

use anyhow::{Context, Result, anyhow, bail};
use iroh::endpoint::Connection;
use iroh::protocol::{AcceptError, ProtocolHandler};
use iroh::{Endpoint, EndpointAddr};
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, oneshot};
use ursa_core::account::{Account, DeviceCert, open_sealed, seal_to, xwing_public};
use ursa_core::crypto::AccountKey;

use crate::{parse_addr, read_header, ticket_of, write_header};

pub const ALPN: &[u8] = b"ursa/link/1";

#[derive(Clone)]
pub struct LinkCode {
    pub secret: [u8; 32],
    pub addr: EndpointAddr,
}

impl LinkCode {
    pub fn new(addr: EndpointAddr) -> Self {
        let mut secret = [0u8; 32];
        getrandom::fill(&mut secret).expect("OS randomness");
        Self { secret, addr }
    }

    pub fn parse(s: &str) -> Result<Self> {
        let rest = s.trim().strip_prefix("ursa-link:").ok_or_else(|| anyhow!("not an ursa-link: code"))?;
        let (secret, ticket) = rest.split_once('@').ok_or_else(|| anyhow!("link code has no ticket"))?;
        let mut sb = [0u8; 32];
        hex::decode_to_slice(secret, &mut sb).map_err(|_| anyhow!("link code secret is malformed"))?;
        Ok(Self { secret: sb, addr: parse_addr(ticket)? })
    }
}

impl std::fmt::Display for LinkCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ursa-link:{}@{}", hex::encode(self.secret), ticket_of(&self.addr))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Hello {
    /// keyed_hash(secret, "ursa link join" ‖ joiner id): proves the joiner has the code.
    proof: String,
    xwing: String,
    name: String,
    platform: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "snake_case")]
enum Reply {
    Accepted { sealed_root: String, cert: Box<DeviceCert> },
    Rejected { reason: String },
}

/// Who is asking to join, shown with the confirmation code.
#[derive(Debug, Clone)]
pub struct Joiner {
    pub device: String,
    pub name: String,
    pub platform: String,
}

fn proof(secret: &[u8; 32], joiner: &str) -> String {
    let mut h = blake3::Hasher::new_keyed(secret);
    h.update(b"ursa link join\0");
    h.update(joiner.as_bytes());
    h.finalize().to_hex().to_string()
}

/// The 6-digit code both screens show.
fn confirmation(secret: &[u8; 32], offerer: &str, joiner: &str, xwing: &[u8]) -> String {
    let mut h = blake3::Hasher::new_keyed(secret);
    h.update(b"ursa link confirm\0");
    h.update(offerer.as_bytes());
    h.update(joiner.as_bytes());
    h.update(xwing);
    let n = u32::from_le_bytes(h.finalize().as_bytes()[..4].try_into().unwrap()) % 1_000_000;
    format!("{n:06}")
}

fn transcript(offerer: &str, joiner: &str, xwing: &[u8]) -> Vec<u8> {
    let mut t = b"ursa link root v1\0".to_vec();
    t.extend_from_slice(offerer.as_bytes());
    t.extend_from_slice(joiner.as_bytes());
    t.extend_from_slice(blake3::hash(xwing).as_bytes());
    t
}

// ---------------------------------------------------------------------------------------------
// Offering side

/// What the offering device decides once it has shown the confirmation code.
pub type Decision = oneshot::Sender<bool>;

struct Offer {
    code: LinkCode,
    account: Arc<Account>,
    now_ms: i64,
    /// Told who is joining and the code to show; answers with the person's decision.
    prompt: tokio::sync::mpsc::Sender<(Joiner, String, Decision)>,
}

/// The `ursa/link/1` handler on a device that has the account. At most one offer at a time.
#[derive(Clone, Default)]
pub struct LinkHandler {
    offer: Arc<Mutex<Option<Offer>>>,
}

impl std::fmt::Debug for LinkHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("LinkHandler")
    }
}

impl LinkHandler {
    /// Opens an offer and returns its code, plus a channel on which each join attempt arrives
    /// with the confirmation code to show and a sender for the person's decision.
    pub async fn offer(
        &self,
        endpoint: &Endpoint,
        account: Arc<Account>,
        now_ms: i64,
    ) -> (LinkCode, tokio::sync::mpsc::Receiver<(Joiner, String, Decision)>) {
        let code = LinkCode::new(endpoint.addr());
        let (tx, rx) = tokio::sync::mpsc::channel(1);
        *self.offer.lock().await = Some(Offer { code: code.clone(), account, now_ms, prompt: tx });
        (code, rx)
    }

    pub async fn cancel(&self) {
        *self.offer.lock().await = None;
    }

    async fn handle(&self, conn: Connection) -> Result<()> {
        let joiner = conn.remote_id().to_string();
        let (mut send, mut recv) = conn.accept_bi().await?;
        let hello: Hello = read_header(&mut recv).await?;
        let (code, account, now_ms, prompt) = {
            let guard = self.offer.lock().await;
            let Some(o) = guard.as_ref() else {
                write_header(&mut send, &Reply::Rejected { reason: "no link offer is open on that device".into() }).await?;
                send.finish()?;
                return Ok(());
            };
            (o.code.clone(), o.account.clone(), o.now_ms, o.prompt.clone())
        };
        // This device's own id, as the joiner saw it in the ticket.
        let offerer = code.addr.id.to_string();
        if !constant_eq(hello.proof.as_bytes(), proof(&code.secret, &joiner).as_bytes()) {
            write_header(&mut send, &Reply::Rejected { reason: "wrong link code".into() }).await?;
            send.finish()?;
            return Ok(());
        }
        let xwing = base64_decode(&hello.xwing)?;
        let shown = confirmation(&code.secret, &offerer, &joiner, &xwing);
        let (decide, decision) = oneshot::channel();
        let who = Joiner { device: joiner.clone(), name: hello.name.clone(), platform: hello.platform.clone() };
        prompt.send((who, shown, decide)).await.map_err(|_| anyhow!("nobody is watching the offer"))?;
        let accepted = tokio::time::timeout(std::time::Duration::from_secs(300), decision).await.ok().and_then(|r| r.ok()).unwrap_or(false);
        let reply = if accepted {
            // One device per code.
            *self.offer.lock().await = None;
            let sealed = seal_to(&xwing, account.root().as_bytes(), &transcript(&offerer, &joiner, &xwing)).map_err(|e| anyhow!("{e}"))?;
            let cert = DeviceCert::issue(&account, &joiner, &hello.name, &hello.platform, now_ms);
            Reply::Accepted { sealed_root: base64_encode(&sealed), cert: Box::new(cert) }
        } else {
            Reply::Rejected { reason: "not confirmed on the other device".into() }
        };
        write_header(&mut send, &reply).await?;
        send.finish()?;
        // Let the reply drain before the connection drops.
        let _ = tokio::time::timeout(std::time::Duration::from_secs(5), conn.closed()).await;
        Ok(())
    }
}

impl ProtocolHandler for LinkHandler {
    async fn accept(&self, conn: Connection) -> Result<(), AcceptError> {
        if let Err(e) = self.handle(conn).await {
            tracing::debug!("link attempt failed: {e:#}");
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------------------------
// Joining side

/// Joins the account behind `code`. `on_confirmation` is called with the 6-digit code to show
/// while the person confirms on the other device. Returns the account root and this device's
/// certificate.
pub async fn join(
    endpoint: &Endpoint,
    code: &str,
    name: &str,
    platform: &str,
    on_confirmation: impl FnOnce(String),
) -> Result<(AccountKey, DeviceCert)> {
    let code = LinkCode::parse(code)?;
    let me = endpoint.id().to_string();
    let offerer = code.addr.id.to_string();
    let mut seed = [0u8; 32];
    getrandom::fill(&mut seed).expect("OS randomness");
    let xwing = xwing_public(&seed);
    let conn = endpoint.connect(code.addr.clone(), ALPN).await.context("reach the other device")?;
    let (mut send, mut recv) = conn.open_bi().await?;
    write_header(
        &mut send,
        &Hello { proof: proof(&code.secret, &me), xwing: base64_encode(&xwing), name: name.into(), platform: platform.into() },
    )
    .await?;
    send.finish()?;
    on_confirmation(confirmation(&code.secret, &offerer, &me, &xwing));
    let reply: Reply = tokio::time::timeout(std::time::Duration::from_secs(320), read_header(&mut recv))
        .await
        .map_err(|_| anyhow!("the other device did not answer in time"))??;
    conn.close(0u32.into(), b"done");
    match reply {
        Reply::Rejected { reason } => bail!("{reason}"),
        Reply::Accepted { sealed_root, cert } => {
            let root = open_sealed(&seed, &base64_decode(&sealed_root)?, &transcript(&offerer, &me, &xwing))
                .map_err(|e| anyhow!("could not open the account key: {e}"))?;
            let root: [u8; 32] = root.as_slice().try_into().map_err(|_| anyhow!("account key has the wrong length"))?;
            let root = AccountKey::from_bytes(root);
            let account = Account::from_root(root.clone());
            if cert.account.id != account.id() || !cert.verify_for(&me) {
                bail!("the other device sent a certificate that does not match its account");
            }
            Ok((root, *cert))
        }
    }
}

fn constant_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn base64_encode(b: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(b)
}

fn base64_decode(s: &str) -> Result<Vec<u8>> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.decode(s).map_err(|_| anyhow!("malformed base64"))
}
