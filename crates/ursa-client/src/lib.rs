//! An Ursa device: what the desktop host, the CLI and (next) the Android app all run.
//!
//! - [`Device`]: this machine's iroh endpoint and identity, before it has an account. It can
//!   create an account, recover one from its root, or join one by link code.
//! - [`Session`]: a device with an account. It knows the account's head (devices, home nodes,
//!   friends, library index), keeps connections to the pool, stores and fetches coded objects,
//!   and offers link codes to new devices.

pub mod capacity;
pub mod library;
pub mod locator;
mod objects;
pub mod storage;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use iroh::protocol::Router;
use iroh::{Endpoint, EndpointAddr, RelayMap, RelayMode, RelayUrl, SecretKey};
use serde::{Deserialize, Serialize};
use futures_util::StreamExt;
use tokio::sync::Mutex;
use tracing::{info, warn};
use ursa_core::account::{Account, AccountId, DeviceCert};
use ursa_core::crypto::AccountKey;
use ursa_core::head::{AccountState, DeviceEntry, Friend, Head, NodeRef};
use ursa_proto::link::{LinkCode, LinkHandler};
use ursa_proto::{ClaimLink, HeadPut, NodeClient, NodeInfo, Role, parse_addr, ticket_of};

pub use library::{Item, UploadMeta};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
/// After a node fails to answer, calls to it fail at once for this long instead of each paying
/// the full dial timeout: a repair or a download touches a dead node once per chunk.
const DOWN_BACKOFF: Duration = Duration::from_secs(60);
/// After the first node hands over the account head, how long the others get to offer a newer one.
const HEAD_GRACE: Duration = Duration::from_secs(3);

pub fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

/// How a device reaches the world.
#[derive(Clone, Debug, Default)]
pub struct NetConfig {
    /// iroh relays; empty means n0's public ones.
    pub relays: Vec<String>,
    pub relay_token: Option<String>,
    /// pkarr relays for the account locator; empty means iroh's (`dns.iroh.link`).
    pub pkarr: Vec<String>,
}

/// This machine, before or without an account.
pub struct Device {
    data_dir: PathBuf,
    endpoint: Endpoint,
    router: Router,
    link: LinkHandler,
    pkarr: Vec<url::Url>,
}

impl Device {
    pub async fn start(data_dir: &Path, net: &NetConfig) -> Result<Arc<Self>> {
        std::fs::create_dir_all(data_dir.join("cache").join("objects"))?;
        std::fs::create_dir_all(data_dir.join("cache").join("thumbs"))?;
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
        if !net.relays.is_empty() {
            let urls = net
                .relays
                .iter()
                .map(|u| u.trim().parse::<RelayUrl>().map_err(|e| anyhow!("bad relay URL {u}: {e}")))
                .collect::<Result<Vec<_>>>()?;
            let mut map = RelayMap::from_iter(urls);
            if let Some(t) = net.relay_token.as_ref().filter(|t| !t.trim().is_empty()) {
                map = map.with_auth_token(t.trim().to_owned());
            }
            builder = builder.relay_mode(RelayMode::Custom(map));
        }
        let endpoint = builder.bind().await.context("start the network endpoint")?;
        let link = LinkHandler::default();
        let router = Router::builder(endpoint.clone()).accept(ursa_proto::link::ALPN, link.clone()).spawn();
        let pkarr = if net.pkarr.is_empty() {
            locator::default_relays()
        } else {
            net.pkarr.iter().map(|u| u.parse().map_err(|e| anyhow!("bad pkarr relay {u}: {e}"))).collect::<Result<_>>()?
        };
        Ok(Arc::new(Self { data_dir: data_dir.to_path_buf(), endpoint, router, link, pkarr }))
    }

    pub fn id(&self) -> String {
        self.endpoint.id().to_string()
    }

    pub fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub async fn shutdown(&self) {
        let _ = self.router.shutdown().await;
    }

    /// A brand-new account, with this device as its first.
    pub async fn create_account(self: &Arc<Self>, name: &str, device_name: &str, platform: &str) -> Result<Arc<Session>> {
        let account = Account::generate();
        let session = Session::new(self.clone(), account, device_name, platform);
        let state = AccountState {
            name: name.to_string(),
            devices: vec![session.device_entry()],
            ..Default::default()
        };
        session.save_local(&state);
        *session.head.lock().await = Some((None, state));
        Ok(session)
    }

    /// Opens an account whose root this device already holds (it created it, linked to it, or
    /// is recovering it). Finds the head: on the nodes this device remembers, else through the
    /// account's pkarr locator.
    pub async fn open(self: &Arc<Self>, root: AccountKey, device_name: &str, platform: &str) -> Result<Arc<Session>> {
        let session = Session::new(self.clone(), Account::from_root(root), device_name, platform);
        if let Err(e) = session.load_head().await {
            // An account with no node yet lives only on the device that created it.
            match session.load_local() {
                Some(state) => *session.head.lock().await = Some((None, state)),
                None => return Err(e).context("find the account"),
            }
        }
        // Make sure this device is listed (it is new on a recovery).
        if !session.state().await.devices.iter().any(|d| d.device == session.device.id()) {
            let entry = session.device_entry();
            session.update(move |s| s.devices.push(entry.clone())).await?;
        }
        Ok(session)
    }

    /// Joins the account behind a link code; `on_confirmation` gets the 6-digit code to show.
    pub async fn join(
        self: &Arc<Self>,
        code: &str,
        device_name: &str,
        platform: &str,
        on_confirmation: impl FnOnce(String),
    ) -> Result<(AccountKey, Arc<Session>)> {
        let (root, _cert) = ursa_proto::link::join(&self.endpoint, code, device_name, platform, on_confirmation).await?;
        // The offering device knows where the account lives; it sent no head, so look it up.
        let session = self.open(root.clone(), device_name, platform).await?;
        Ok((root, session))
    }
}

/// A device with an account.
pub struct Session {
    pub device: Arc<Device>,
    pub account: Arc<Account>,
    pub cert: DeviceCert,
    device_name: String,
    platform: String,
    conns: Mutex<HashMap<String, NodeClient>>,
    /// Nodes that last failed to answer, and when.
    down: std::sync::Mutex<HashMap<String, Instant>>,
    /// The newest head seen (None before the first save) and its opened state.
    head: Mutex<Option<(Option<Head>, AccountState)>>,
}

/// A node in the pool, as seen right now.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PoolNode {
    pub id: String,
    pub label: String,
    /// `home` (the account's own) or the friend's name.
    pub owner: String,
    pub reachable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// What the node said now or, if it is unreachable, the last time it answered.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub info: Option<NodeInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// What a friend needs to add you: your account id, name and nodes.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Invite {
    pub account: AccountId,
    pub name: String,
    pub nodes: Vec<NodeRef>,
}

impl Invite {
    pub fn encode(&self) -> String {
        use base64::Engine;
        format!("ursa-invite:{}", base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(serde_json::to_vec(self).expect("json")))
    }

    pub fn decode(s: &str) -> Result<Self> {
        use base64::Engine;
        let b = s.trim().strip_prefix("ursa-invite:").ok_or_else(|| anyhow!("not an ursa-invite: code"))?;
        let json = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(b).map_err(|_| anyhow!("malformed invite"))?;
        serde_json::from_slice(&json).map_err(|_| anyhow!("malformed invite"))
    }
}

impl Session {
    fn new(device: Arc<Device>, account: Account, device_name: &str, platform: &str) -> Arc<Self> {
        let cert = DeviceCert::issue(&account, &device.id(), device_name, platform, now_ms());
        Arc::new(Self {
            device,
            account: Arc::new(account),
            cert,
            device_name: device_name.to_string(),
            platform: platform.to_string(),
            conns: Mutex::new(HashMap::new()),
            down: std::sync::Mutex::new(HashMap::new()),
            head: Mutex::new(None),
        })
    }

    fn device_entry(&self) -> DeviceEntry {
        DeviceEntry { device: self.device.id(), name: self.device_name.clone(), platform: self.platform.clone(), added_ms: now_ms() }
    }

    pub fn account_id(&self) -> AccountId {
        self.account.id()
    }

    pub async fn state(&self) -> AccountState {
        self.head.lock().await.as_ref().map(|(_, s)| s.clone()).unwrap_or_default()
    }

    pub async fn seq(&self) -> Option<u64> {
        self.head.lock().await.as_ref().and_then(|(h, _)| h.as_ref().map(|h| h.seq))
    }

    // -----------------------------------------------------------------------------------------
    // Connections

    fn addr_of(r: &NodeRef) -> Result<EndpointAddr> {
        match &r.ticket {
            Some(t) => parse_addr(t),
            None => parse_addr(&r.id),
        }
    }

    /// An authenticated connection to a node, reusing a live one.
    pub async fn node(&self, r: &NodeRef) -> Result<NodeClient> {
        if let Some(c) = self.conns.lock().await.get(&r.id).filter(|c| !c.is_closed()) {
            return Ok(c.clone());
        }
        if let Some(at) = self.down.lock().unwrap().get(&r.id)
            && at.elapsed() < DOWN_BACKOFF
        {
            bail!("node {} did not answer {}s ago", short(&r.id), at.elapsed().as_secs());
        }
        let addr = Self::addr_of(r)?;
        let dialled = tokio::time::timeout(CONNECT_TIMEOUT, NodeClient::connect(&self.device.endpoint, addr, &self.cert))
            .await
            .map_err(|_| anyhow!("node {} did not answer", short(&r.id)))
            .and_then(|r| r);
        match dialled {
            Ok(c) => {
                self.down.lock().unwrap().remove(&r.id);
                self.conns.lock().await.insert(r.id.clone(), c.clone());
                Ok(c)
            }
            Err(e) => {
                self.down.lock().unwrap().insert(r.id.clone(), Instant::now());
                Err(e)
            }
        }
    }

    /// Whether a dial to this node failed within the last [`DOWN_BACKOFF`].
    pub(crate) fn is_down(&self, id: &str) -> bool {
        self.down.lock().unwrap().get(id).is_some_and(|at| at.elapsed() < DOWN_BACKOFF)
    }

    fn drop_conn(&self, id: &str) -> impl std::future::Future<Output = ()> + '_ {
        let id = id.to_string();
        async move {
            self.conns.lock().await.remove(&id);
        }
    }

    pub fn path_of(c: &NodeClient) -> String {
        let paths = c.connection().paths();
        match paths.iter().find(|p| p.is_selected()) {
            Some(p) if p.is_ip() => "direct".into(),
            Some(p) if p.is_relay() => "relay".into(),
            _ => "connecting".into(),
        }
    }

    // -----------------------------------------------------------------------------------------
    // The head

    /// Every node the head should live on: home nodes, then friends' nodes.
    fn head_nodes(state: &AccountState) -> Vec<NodeRef> {
        state.pool()
    }

    /// Reads the newest head from every node this device can think of: those in the current
    /// state, those it remembered on disk, and those the pkarr locator lists.
    pub async fn load_head(&self) -> Result<()> {
        let mut candidates: Vec<NodeRef> = Self::head_nodes(&self.state().await);
        if let Ok(b) = std::fs::read(self.device.data_dir.join("known-nodes.json"))
            && let Ok(known) = serde_json::from_slice::<Vec<NodeRef>>(&b)
        {
            for n in known {
                if !candidates.iter().any(|c| c.id == n.id) {
                    candidates.push(n);
                }
            }
        }
        if candidates.is_empty() {
            match locator::resolve(&self.device.endpoint, &self.device.pkarr, &self.account).await {
                Ok(ids) => {
                    info!(nodes = ids.len(), "found the account through its locator");
                    candidates.extend(ids.into_iter().map(|id| NodeRef { id, ticket: None, label: String::new() }));
                }
                Err(e) => warn!("locator: {e:#}"),
            }
        }
        if candidates.is_empty() {
            bail!("no node of this account is known, and its locator record was not found");
        }
        // Every node holds the head (writes go to all of them, compare-and-swap), so once one has
        // answered the rest get HEAD_GRACE to offer a newer one: a node that is off must not hold
        // every start up for the full dial timeout.
        let mut pending: futures_util::stream::FuturesUnordered<_> = candidates
            .iter()
            .map(|n| async move {
                let c = self.node(n).await?;
                c.head_get().await
            })
            .collect();
        let mut best: Option<Head> = None;
        let mut reached = 0;
        let mut deadline: Option<tokio::time::Instant> = None;
        loop {
            let next = match deadline {
                None => pending.next().await,
                Some(d) => match tokio::time::timeout_at(d, pending.next()).await {
                    Ok(n) => n,
                    Err(_) => {
                        info!(waiting = pending.len(), "head: not waiting longer for slow nodes");
                        break;
                    }
                },
            };
            let Some(r) = next else { break };
            let Ok(h) = r else { continue };
            reached += 1;
            if let Some(h) = h
                && h.verify()
                && h.account.id == self.account.id()
                && best.as_ref().is_none_or(|b| h.seq > b.seq)
            {
                best = Some(h);
                deadline.get_or_insert_with(|| tokio::time::Instant::now() + HEAD_GRACE);
            }
        }
        let Some(head) = best else {
            if reached == 0 {
                bail!("none of the account's {} known nodes answered", candidates.len());
            }
            bail!("the account's nodes answered but none holds its head");
        };
        let state = head.open(&self.account).map_err(|e| anyhow!("open the account head: {e}"))?;
        self.remember(&state);
        *self.head.lock().await = Some((Some(head), state));
        Ok(())
    }

    fn local_head_path(&self) -> PathBuf {
        self.device.data_dir.join(format!("local-head-{}.json", self.account.id().short()))
    }

    /// Keeps a head that has no node to live on yet (sealed and signed like any other).
    fn save_local(&self, state: &AccountState) {
        let head = Head::seal(&self.account, 0, state);
        let _ = std::fs::write(self.local_head_path(), head.to_json());
    }

    fn load_local(&self) -> Option<AccountState> {
        let b = std::fs::read(self.local_head_path()).ok()?;
        Head::from_json(&b).ok()?.open(&self.account).ok()
    }

    fn remember(&self, state: &AccountState) {
        let _ = std::fs::write(
            self.device.data_dir.join("known-nodes.json"),
            serde_json::to_vec_pretty(&Self::head_nodes(state)).unwrap_or_default(),
        );
    }

    /// Changes the account state and saves a new head everywhere it lives, merging with anyone
    /// who wrote in between: `change` is re-applied to the newer state on a conflict.
    pub async fn update(&self, change: impl Fn(&mut AccountState)) -> Result<AccountState> {
        for _attempt in 0..5 {
            let (base_seq, mut state) = {
                let g = self.head.lock().await;
                let (h, s) = g.as_ref().cloned().unwrap_or((None, AccountState::default()));
                (h.map(|h| h.seq), s)
            };
            change(&mut state);
            let seq = base_seq.map_or(1, |s| s + 1);
            let head = Head::seal(&self.account, seq, &state);
            let targets = Self::head_nodes(&state);
            if targets.is_empty() {
                // No node yet: keep it locally until there is one to save to.
                self.save_local(&state);
                *self.head.lock().await = Some((None, state.clone()));
                return Ok(state);
            }
            let results = futures_util::future::join_all(targets.iter().map(|n| {
                let head = head.clone();
                async move {
                    let c = self.node(n).await?;
                    let current = c.head_get().await?;
                    let theirs = current.as_ref().map(|h| h.seq);
                    // A newer head than the one we built on: someone else saved in between.
                    if theirs.unwrap_or(0) > base_seq.unwrap_or(0) {
                        return Ok::<_, anyhow::Error>(current);
                    }
                    match c.head_put(&head, theirs).await? {
                        HeadPut::Stored => Ok(None),
                        HeadPut::Conflict(h) => Ok(Some(*h)),
                    }
                }
            }))
            .await;
            let mut stored = 0;
            let mut newer: Option<Head> = None;
            for r in results {
                match r {
                    Ok(None) => stored += 1,
                    Ok(Some(h)) => {
                        if h.verify() && h.account.id == self.account.id() && h.seq >= seq && newer.as_ref().is_none_or(|n| h.seq > n.seq) {
                            newer = Some(h);
                        }
                    }
                    Err(e) => warn!("head save: {e:#}"),
                }
            }
            if let Some(h) = newer {
                // Someone else saved first: take theirs and try again on top of it.
                let s = h.open(&self.account).map_err(|e| anyhow!("{e}"))?;
                *self.head.lock().await = Some((Some(h), s));
                continue;
            }
            if stored == 0 {
                bail!("no node accepted the account head");
            }
            self.remember(&state);
            *self.head.lock().await = Some((Some(head), state.clone()));
            let ids: Vec<String> = targets.iter().map(|n| n.id.clone()).collect();
            let relays = self.device.pkarr.clone();
            let account = self.account.clone();
            let endpoint = self.device.endpoint.clone();
            tokio::spawn(async move {
                if let Err(e) = locator::publish(&endpoint, &relays, &account, &ids).await {
                    warn!("locator publish: {e:#}");
                }
            });
            return Ok(state);
        }
        bail!("the account head kept changing; try again")
    }

    pub async fn publish_locator(&self) -> Result<usize> {
        let ids: Vec<String> = Self::head_nodes(&self.state().await).into_iter().map(|n| n.id).collect();
        locator::publish(&self.device.endpoint, &self.device.pkarr, &self.account, &ids).await
    }

    // -----------------------------------------------------------------------------------------
    // Nodes and friends

    /// Claims a node with the link it printed and adds it to the account's home nodes.
    pub async fn claim_node(&self, link: &str, label: &str) -> Result<NodeRef> {
        let link: ClaimLink = link.parse()?;
        let r = NodeRef { id: link.addr.id.to_string(), ticket: Some(ticket_of(&link.addr)), label: label.to_string() };
        let c = self.node(&r).await?;
        c.claim(&link.code).await?;
        if c.info().await?.role != Role::Owner {
            bail!("the node accepted the claim but does not list this account as an owner");
        }
        let added = r.clone();
        self.update(move |s| {
            if !s.home_nodes.iter().any(|n| n.id == added.id) {
                s.home_nodes.push(added.clone());
            }
        })
        .await?;
        Ok(r)
    }

    pub async fn forget_node(&self, id: &str) -> Result<()> {
        let id = id.to_string();
        self.update(move |s| s.home_nodes.retain(|n| n.id != id)).await?;
        Ok(())
    }

    /// What to send a friend so they can add this account.
    pub async fn invite(&self) -> Invite {
        let s = self.state().await;
        Invite { account: self.account.id(), name: s.name.clone(), nodes: s.home_nodes.clone() }
    }

    /// Adds a friend from their invite, and grants them `quota_bytes` on every home node.
    pub async fn add_friend(&self, invite: &str, quota_bytes: u64) -> Result<Friend> {
        let inv = Invite::decode(invite)?;
        if inv.account == self.account.id() {
            bail!("that is this account's own invite");
        }
        let friend = Friend { account: inv.account, name: inv.name, nodes: inv.nodes };
        let f = friend.clone();
        let state = self
            .update(move |s| {
                s.friends.retain(|x| x.account != f.account);
                s.friends.push(f.clone());
            })
            .await?;
        for n in &state.home_nodes {
            match self.node(n).await {
                Ok(c) => {
                    if let Err(e) = c.grant(friend.account, quota_bytes).await {
                        warn!(node = %short(&n.id), "grant failed: {e:#}");
                    }
                }
                Err(e) => warn!(node = %short(&n.id), "grant: {e:#}"),
            }
        }
        Ok(friend)
    }

    /// Every node in the pool, checked now.
    pub async fn pool(&self) -> Vec<PoolNode> {
        let state = self.state().await;
        let mut entries: Vec<(NodeRef, String)> = state.home_nodes.iter().map(|n| (n.clone(), "home".to_string())).collect();
        for f in &state.friends {
            for n in &f.nodes {
                if !entries.iter().any(|(e, _)| e.id == n.id) {
                    entries.push((n.clone(), if f.name.is_empty() { f.account.short() } else { f.name.clone() }));
                }
            }
        }
        let checked = futures_util::future::join_all(entries.into_iter().map(|(n, owner)| async move {
            match async {
                let c = self.node(&n).await?;
                let info = c.info().await?;
                Ok::<_, anyhow::Error>((Self::path_of(&c), info))
            }
            .await
            {
                Ok((path, info)) => PoolNode { id: n.id, label: n.label, owner, reachable: true, path: Some(path), info: Some(info), error: None },
                Err(e) => {
                    self.drop_conn(&n.id).await;
                    PoolNode { id: n.id, label: n.label, owner, reachable: false, path: None, info: None, error: Some(format!("{e:#}")) }
                }
            }
        }))
        .await;
        self.remember_node_info(checked)
    }

    /// Keeps what each node last said (its uptime, the space it gives), so a node that is down
    /// now still shows its figures.
    fn remember_node_info(&self, mut nodes: Vec<PoolNode>) -> Vec<PoolNode> {
        let path = self.device.data_dir.join("node-info.json");
        let mut known: HashMap<String, NodeInfo> = std::fs::read(&path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        for n in &mut nodes {
            match &n.info {
                Some(i) if n.reachable => {
                    known.insert(n.id.clone(), i.clone());
                }
                _ => n.info = known.get(&n.id).cloned(),
            }
        }
        if let Ok(b) = serde_json::to_vec(&known) {
            let _ = std::fs::write(&path, b);
        }
        nodes
    }

    /// Reachable nodes that accept this account's data, one per failure domain (a person).
    pub(crate) async fn writable(&self) -> Vec<NodeRef> {
        let state = self.state().await;
        let mut out = Vec::new();
        let mut domains: Vec<String> = Vec::new();
        for p in self.pool().await {
            let Some(info) = p.info.as_ref().filter(|_| p.reachable) else { continue };
            if info.role == Role::None || domains.contains(&p.owner) {
                continue;
            }
            domains.push(p.owner.clone());
            let r = state
                .pool()
                .into_iter()
                .find(|n| n.id == p.id)
                .unwrap_or(NodeRef { id: p.id.clone(), ticket: None, label: String::new() });
            out.push(r);
        }
        out
    }

    /// Offers a link code for a new device. Each join attempt arrives on the receiver with the
    /// confirmation code to show and a sender for the person's yes or no.
    pub async fn offer_link(&self) -> (LinkCode, tokio::sync::mpsc::Receiver<(ursa_proto::link::Joiner, String, ursa_proto::link::Decision)>) {
        self.device.link.offer(&self.device.endpoint, self.account.clone(), now_ms()).await
    }

    pub async fn cancel_link(&self) {
        self.device.link.cancel().await;
    }
}

pub fn short(id: &str) -> String {
    id.chars().take(10).collect()
}
