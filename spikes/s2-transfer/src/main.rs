//! Spike S2: how fast can a client read 4-of-6 coded stripes from six nodes over iroh?
//!
//! `s2 serve` is a storage node that answers `ursa/s2/0` requests with pseudo-random bytes
//! (nothing is read from disk: this measures the network and the protocol, not a disk).
//! `s2 fetch` connects to every node and, for a fixed time per scenario, reads stripes with:
//!
//! - `single`: whole stripes from one node at a time (the baseline: one server, one link).
//! - `fixed`:  shard j of every stripe from node j, j < k. No redundancy used, so the slowest of
//!             the first k nodes sets the pace.
//! - `race`:   every stripe asked of all n nodes; the first k shards win, the rest are cancelled
//!             (Storj's long-tail cancellation).
//! - `pull`:   each node keeps `per_node` requests in flight and takes the oldest stripe that
//!             still needs a shard it can give; a stripe can use any k of the n nodes, so fast
//!             nodes serve more stripes than slow ones. When nothing else is left, an idle node
//!             duplicates the shard holding up the oldest stripe once it is overdue (hedging).
//!
//! A stripe is `k` shards of `stripe / k` bytes; each shard of a stripe must come from a
//! different node. Goodput counts only completed stripes.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::fmt::Write as _;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use clap::{Parser, Subcommand, ValueEnum};
use iroh::endpoint::Connection;
use iroh::protocol::{AcceptError, ProtocolHandler, Router};
use iroh::{Endpoint, EndpointAddr, EndpointId, SecretKey};
use serde::Serialize;
use tokio::sync::{Notify, mpsc};
use tokio::task::AbortHandle;

const ALPN: &[u8] = b"ursa/s2/0";
const OP_GET: u8 = 0;
const OP_SHUTDOWN: u8 = 1;
const REQ_LEN: usize = 13;
const MIB: usize = 1 << 20;
const SERVE_BUF: usize = 64 * MIB;
const WRITE_CHUNK: usize = 256 * 1024;

#[derive(Parser)]
#[command(about = "Spike S2: parallel shard reads over iroh")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Run a storage node until told to stop (or `--max-seconds`).
    Serve {
        /// Shared run seed: node keys are derived from it, so a client can find nodes by seed.
        #[arg(long)]
        seed: String,
        #[arg(long)]
        index: usize,
        #[arg(long, default_value_t = 1800)]
        max_seconds: u64,
        /// Write this node's endpoint ticket (with its direct addresses) here.
        #[arg(long)]
        ticket_out: Option<PathBuf>,
        /// No relays: direct addresses only (an isolated test network).
        #[arg(long)]
        local: bool,
        /// Use these relays instead of n0's public ones (repeatable). The auth token, if the
        /// relays need one, is read from S2_RELAY_TOKEN so it never appears in a command line.
        #[arg(long = "relay")]
        relays: Vec<String>,
    },
    /// Read stripes from the nodes under each strategy and report goodput.
    Fetch {
        /// Find nodes by seed (keys derived as in `serve`) through iroh's discovery.
        #[arg(long)]
        seed: Option<String>,
        #[arg(long, default_value_t = 6)]
        nodes: usize,
        /// Or name them: endpoint tickets or endpoint ids. Overrides `--seed`.
        #[arg(long = "peer")]
        peers: Vec<String>,
        #[arg(long, default_value_t = 4)]
        k: usize,
        #[arg(long, default_value_t = 8)]
        stripe_mib: usize,
        /// Seconds per striped scenario; single-node scenarios get half.
        #[arg(long, default_value_t = 30)]
        seconds: u64,
        /// Most stripes in flight at once (the client's reorder buffer).
        #[arg(long, default_value_t = 16)]
        window: usize,
        /// Requests in flight per node.
        #[arg(long, default_value_t = 2)]
        per_node: usize,
        #[arg(long, value_delimiter = ',', default_value = "single,fixed,race,pull")]
        strategies: Vec<Strategy>,
        #[arg(long, default_value_t = 300)]
        connect_seconds: u64,
        /// Tell every node to exit when done.
        #[arg(long)]
        shutdown: bool,
        #[arg(long)]
        json: Option<PathBuf>,
        /// Free-form description of the setup, copied into the report.
        #[arg(long, default_value = "")]
        label: String,
        /// No relays: direct addresses only (an isolated test network).
        #[arg(long)]
        local: bool,
        /// Use these relays instead of n0's public ones (repeatable). The auth token, if the
        /// relays need one, is read from S2_RELAY_TOKEN so it never appears in a command line.
        #[arg(long = "relay")]
        relays: Vec<String>,
        /// Run `--fault` (a shell command) this many seconds into each striped scenario.
        #[arg(long, requires = "fault")]
        fault_at: Option<u64>,
        #[arg(long, requires = "fault_at")]
        fault: Option<String>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum, Serialize)]
#[serde(rename_all = "lowercase")]
enum Strategy {
    Single,
    Fixed,
    Race,
    Pull,
}

fn node_secret(seed: &str, index: usize) -> SecretKey {
    SecretKey::from_bytes(&blake3::derive_key("ursa s2 node key", format!("{seed}/{index}").as_bytes()))
}

#[tokio::main]
async fn main() -> Result<()> {
    match Cli::parse().cmd {
        Cmd::Serve { seed, index, max_seconds, ticket_out, local, relays } => {
            serve(seed, index, max_seconds, ticket_out, Net { local, relays }).await
        }
        Cmd::Fetch {
            seed,
            nodes,
            peers,
            k,
            stripe_mib,
            seconds,
            window,
            per_node,
            strategies,
            connect_seconds,
            shutdown,
            json,
            label,
            local,
            relays,
            fault_at,
            fault,
        } => {
            let addrs: Vec<EndpointAddr> = if !peers.is_empty() {
                peers.iter().map(|p| parse_peer(p)).collect::<Result<_>>()?
            } else {
                let seed = seed.ok_or_else(|| anyhow!("pass --peer or --seed"))?;
                (0..nodes).map(|i| EndpointAddr::new(node_secret(&seed, i).public())).collect()
            };
            let params = Params { k, stripe: stripe_mib * MIB, seconds, window, per_node };
            fetch(addrs, params, strategies, connect_seconds, shutdown, json, label, Net { local, relays }, fault_at.zip(fault)).await
        }
    }
}

fn parse_peer(s: &str) -> Result<EndpointAddr> {
    if let Ok(t) = s.parse::<iroh_tickets::endpoint::EndpointTicket>() {
        return Ok(t.endpoint_addr().clone());
    }
    let id: EndpointId = s.parse().map_err(|e| anyhow!("{s} is neither a ticket nor an endpoint id: {e}"))?;
    Ok(EndpointAddr::new(id))
}

// ---------------------------------------------------------------------------------------------
// Server

#[derive(Debug, Clone)]
struct Server {
    data: Arc<Vec<u8>>,
    shutdown: Arc<Notify>,
}

impl ProtocolHandler for Server {
    async fn accept(&self, conn: Connection) -> Result<(), AcceptError> {
        loop {
            let Ok((mut send, mut recv)) = conn.accept_bi().await else {
                return Ok(());
            };
            let data = self.data.clone();
            let shutdown = self.shutdown.clone();
            tokio::spawn(async move {
                let mut req = [0u8; REQ_LEN];
                if recv.read_exact(&mut req).await.is_err() {
                    return;
                }
                match req[0] {
                    OP_GET => {
                        let stripe = u64::from_le_bytes(req[1..9].try_into().unwrap());
                        let len = u32::from_le_bytes(req[9..13].try_into().unwrap()) as usize;
                        let span = data.len() - WRITE_CHUNK;
                        let mut off = (stripe as usize).wrapping_mul(1_048_573) % span;
                        let mut left = len;
                        while left > 0 {
                            let n = left.min(WRITE_CHUNK);
                            if send.write_all(&data[off..off + n]).await.is_err() {
                                return; // cancelled by the client
                            }
                            left -= n;
                            off = (off + n) % span;
                        }
                        let _ = send.finish();
                    }
                    OP_SHUTDOWN => {
                        let _ = send.finish();
                        shutdown.notify_one();
                    }
                    _ => {}
                }
            });
        }
    }
}

/// How an endpoint reaches the world: no relays, our own relays, or n0's public ones.
struct Net {
    local: bool,
    relays: Vec<String>,
}

async fn bind(secret: SecretKey, net: &Net) -> Result<Endpoint> {
    let mut b = Endpoint::builder(iroh::endpoint::presets::N0).secret_key(secret);
    if net.local {
        b = b.relay_mode(iroh::RelayMode::Disabled);
    } else if !net.relays.is_empty() {
        let urls = net
            .relays
            .iter()
            .map(|u| u.parse::<iroh::RelayUrl>().map_err(|e| anyhow!("bad relay URL {u}: {e}")))
            .collect::<Result<Vec<_>>>()?;
        let mut map = iroh::RelayMap::from_iter(urls);
        if let Ok(token) = std::env::var("S2_RELAY_TOKEN") {
            map = map.with_auth_token(token.trim().to_owned());
        }
        b = b.relay_mode(iroh::RelayMode::Custom(map));
    }
    let endpoint = b.bind().await?;
    if !net.local {
        let _ = tokio::time::timeout(Duration::from_secs(30), endpoint.online()).await;
    }
    Ok(endpoint)
}

async fn serve(seed: String, index: usize, max_seconds: u64, ticket_out: Option<PathBuf>, net: Net) -> Result<()> {
    let endpoint = bind(node_secret(&seed, index), &net).await?;
    let mut data = vec![0u8; SERVE_BUF];
    blake3::Hasher::new().update(seed.as_bytes()).finalize_xof().fill(&mut data);
    let shutdown = Arc::new(Notify::new());
    let router = Router::builder(endpoint.clone())
        .accept(ALPN, Server { data: Arc::new(data), shutdown: shutdown.clone() })
        .spawn();
    let ticket = iroh_tickets::endpoint::EndpointTicket::new(endpoint.addr()).to_string();
    println!("node {index} id {} ticket {ticket}", endpoint.id());
    if let Some(p) = ticket_out {
        std::fs::write(p, &ticket)?;
    }
    tokio::select! {
        _ = shutdown.notified() => println!("node {index}: shutdown requested"),
        _ = tokio::time::sleep(Duration::from_secs(max_seconds)) => println!("node {index}: max time"),
        _ = tokio::signal::ctrl_c() => {}
    }
    router.shutdown().await.ok();
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Client

#[derive(Clone, Copy, Serialize)]
struct Params {
    k: usize,
    stripe: usize,
    seconds: u64,
    window: usize,
    per_node: usize,
}

async fn fetch_shard(conn: &Connection, stripe: u64, len: usize, received: &AtomicU64) -> Result<()> {
    let (mut send, mut recv) = conn.open_bi().await?;
    let mut req = [0u8; REQ_LEN];
    req[0] = OP_GET;
    req[1..9].copy_from_slice(&stripe.to_le_bytes());
    req[9..13].copy_from_slice(&(len as u32).to_le_bytes());
    send.write_all(&req).await?;
    send.finish()?;
    let mut got = 0usize;
    while let Some(chunk) = recv.read_chunk(MIB).await? {
        got += chunk.len();
        received.fetch_add(chunk.len() as u64, Ordering::Relaxed);
    }
    if got != len {
        bail!("short shard: {got} of {len}");
    }
    Ok(())
}

async fn send_op(conn: &Connection, op: u8) -> Result<()> {
    let (mut send, mut recv) = conn.open_bi().await?;
    let mut req = [0u8; REQ_LEN];
    req[0] = op;
    send.write_all(&req).await?;
    send.finish()?;
    let mut sink = [0u8; 16];
    let _ = tokio::time::timeout(Duration::from_secs(5), recv.read(&mut sink)).await;
    Ok(())
}

/// The selected path, described without the address: results get committed, and a direct path's
/// address is someone's home IP. A relay is named by its URL (ours, not personal).
fn path_of(conn: &Connection) -> String {
    let paths = conn.paths();
    let selected = paths.iter().find(|p| p.is_selected());
    match selected {
        Some(p) => match p.remote_addr() {
            iroh::TransportAddr::Ip(sa) => {
                let scope = match sa.ip() {
                    std::net::IpAddr::V4(v4) if v4.is_private() || v4.is_loopback() || v4.is_link_local() => "private",
                    std::net::IpAddr::V6(v6) if v6.is_loopback() || (v6.segments()[0] & 0xfe00) == 0xfc00 || (v6.segments()[0] & 0xffc0) == 0xfe80 => "private",
                    _ => "public",
                };
                let family = if sa.is_ipv4() { "IPv4" } else { "IPv6" };
                format!("direct, {scope} {family}")
            }
            iroh::TransportAddr::Relay(url) => format!("relay {url}"),
            _ => "other".into(),
        },
        None => "none".into(),
    }
}

struct StripeState {
    started: Instant,
    done: usize,
    /// Scenario-node positions with a request outstanding for this stripe, and when it began.
    pending: Vec<(usize, Instant)>,
    /// Bitmask of positions ever asked for this stripe.
    asked: u64,
}

struct Event {
    pos: usize,
    stripe: u64,
    ok: bool,
}

#[derive(Serialize)]
struct Report {
    scenario: String,
    strategy: Strategy,
    /// Original node indices, in position order.
    nodes: Vec<usize>,
    k: usize,
    stripe_mib: usize,
    seconds: f64,
    stripes: u64,
    goodput_mbit: f64,
    received_mbit: f64,
    waste_pct: f64,
    per_node_mbit: Vec<f64>,
    per_node_share_pct: Vec<f64>,
    stripe_ms_p50: f64,
    stripe_ms_p90: f64,
    stripe_ms_max: f64,
    hedges: u64,
    failures: u64,
    /// Nodes taken out of rotation after a failed request (original indices).
    dropped: Vec<usize>,
    /// Goodput per second of the run.
    timeline_mbit: Vec<f64>,
    fault: Option<String>,
}

fn pct(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    sorted[((sorted.len() - 1) as f64 * p).round() as usize]
}

/// A request is overdue when it has run 2.5x longer than its node's measured rate predicts
/// (at least 1 s), or, before the node has a rate, 2.5x the median stripe time.
fn overdue(started: Instant, rate: Option<f64>, shard_len: usize, median_stripe: Option<f64>) -> bool {
    let expected = match (rate, median_stripe) {
        (Some(r), _) => shard_len as f64 / r,
        (None, Some(m)) => m,
        (None, None) => return false,
    };
    started.elapsed().as_secs_f64() > (2.5 * expected).max(1.0)
}

struct Scenario {
    name: String,
    strategy: Strategy,
    /// Positions -> indices into the live connection list.
    nodes: Vec<usize>,
    k: usize,
    shard_len: usize,
    seconds: u64,
    fault: Option<(u64, String)>,
}

/// Runs one scenario. Pull scheduling, in order of preference for an idle node: the oldest
/// stripe that still needs a shard (an overdue request does not count as coming), then a new
/// stripe if the window allows. A node whose request fails leaves the rotation.
async fn run_scenario(sc: Scenario, conns: &[(usize, Connection)], p: Params) -> Report {
    let Scenario { name, strategy, nodes, k, shard_len, seconds, fault } = sc;
    let nn = nodes.len();
    let (tx, mut rx) = mpsc::unbounded_channel::<Event>();
    let received: Vec<Arc<AtomicU64>> = (0..nn).map(|_| Arc::new(AtomicU64::new(0))).collect();
    let mut inflight: BTreeMap<u64, StripeState> = BTreeMap::new();
    let mut handles: HashMap<(u64, usize), AbortHandle> = HashMap::new();
    let mut busy = vec![0usize; nn];
    let mut dead = vec![false; nn];
    let mut rate: Vec<Option<f64>> = vec![None; nn];
    let mut next_stripe = 0u64;
    let mut completed = 0u64;
    let mut latencies: Vec<f64> = Vec::new();
    let mut recent: VecDeque<f64> = VecDeque::new();
    let mut timeline: Vec<u64> = vec![0; seconds as usize + 1];
    let (mut hedges, mut failures) = (0u64, 0u64);

    let start = Instant::now();
    let deadline = start + Duration::from_secs(seconds);
    if let Some((at, cmd)) = fault.clone() {
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(at)).await;
            eprintln!("fault at {at} s: {cmd}");
            let _ = tokio::process::Command::new("sh").arg("-c").arg(&cmd).status().await;
        });
    }

    loop {
        if Instant::now() >= deadline {
            break;
        }
        let median = if recent.len() >= 4 {
            let mut v: Vec<f64> = recent.iter().copied().collect();
            v.sort_by(f64::total_cmp);
            Some(pct(&v, 0.5) / 1000.0)
        } else {
            None
        };

        loop {
            let mut progressed = false;
            for pos in 0..nn {
                while busy[pos] < p.per_node && !dead[pos] {
                    if strategy == Strategy::Fixed && pos >= k {
                        break;
                    }
                    let bit = 1u64 << pos;
                    // 1. The oldest stripe that still needs a shard this node can give.
                    let mut pick = inflight.iter().find_map(|(&s, st)| {
                        if st.asked & bit != 0 {
                            return None;
                        }
                        match strategy {
                            Strategy::Pull => {
                                let coming = st
                                    .pending
                                    .iter()
                                    .filter(|(q, t)| !overdue(*t, rate[*q], shard_len, median))
                                    .count();
                                let late = st.pending.len() - coming;
                                (st.done + coming < k).then_some((s, late > 0))
                            }
                            Strategy::Single | Strategy::Fixed | Strategy::Race => Some((s, false)),
                        }
                    });
                    // 2. A new stripe, if the window allows.
                    if pick.is_none() && inflight.len() < p.window {
                        let s = next_stripe;
                        next_stripe += 1;
                        inflight.insert(s, StripeState { started: Instant::now(), done: 0, pending: Vec::new(), asked: 0 });
                        pick = Some((s, false));
                    }
                    let Some((stripe, hedge)) = pick else { break };
                    let st = inflight.get_mut(&stripe).unwrap();
                    st.asked |= bit;
                    st.pending.push((pos, Instant::now()));
                    if hedge {
                        hedges += 1;
                    }
                    busy[pos] += 1;
                    let conn = conns[nodes[pos]].1.clone();
                    let tx = tx.clone();
                    let ctr = received[pos].clone();
                    let h = tokio::spawn(async move {
                        let ok = fetch_shard(&conn, stripe, shard_len, &ctr).await.is_ok();
                        let _ = tx.send(Event { pos, stripe, ok });
                    });
                    handles.insert((stripe, pos), h.abort_handle());
                    progressed = true;
                }
            }
            if !progressed {
                break;
            }
        }

        let ev = tokio::select! {
            ev = rx.recv() => ev,
            _ = tokio::time::sleep(Duration::from_millis(20)) => None,
            _ = tokio::time::sleep_until(deadline.into()) => None,
        };
        let Some(ev) = ev else { continue };
        if handles.remove(&(ev.stripe, ev.pos)).is_none() {
            continue; // already cancelled and accounted for
        }
        busy[ev.pos] -= 1;
        if !ev.ok {
            failures += 1;
            if !dead[ev.pos] {
                eprintln!("{name}: node n{} dropped after a failed request", conns[nodes[ev.pos]].0);
            }
            dead[ev.pos] = true;
        }
        let Some(st) = inflight.get_mut(&ev.stripe) else { continue };
        if let Some(i) = st.pending.iter().position(|(q, _)| *q == ev.pos) {
            let (_, began) = st.pending.remove(i);
            if ev.ok {
                st.done += 1;
                let r = shard_len as f64 / began.elapsed().as_secs_f64().max(1e-3);
                rate[ev.pos] = Some(rate[ev.pos].map_or(r, |old| 0.7 * old + 0.3 * r));
            }
        }
        if st.done >= k {
            let st = inflight.remove(&ev.stripe).unwrap();
            let ms = st.started.elapsed().as_secs_f64() * 1000.0;
            latencies.push(ms);
            recent.push_back(ms);
            if recent.len() > 32 {
                recent.pop_front();
            }
            completed += 1;
            let sec = (start.elapsed().as_secs() as usize).min(timeline.len() - 1);
            timeline[sec] += (k * shard_len) as u64;
            for (q, _) in st.pending {
                if let Some(h) = handles.remove(&(ev.stripe, q)) {
                    h.abort();
                    busy[q] -= 1;
                }
            }
        }
    }
    for (_, h) in handles.drain() {
        h.abort();
    }

    let secs = start.elapsed().as_secs_f64();
    let good = completed as f64 * (k * shard_len) as f64;
    let per_node: Vec<f64> = received.iter().map(|c| c.load(Ordering::Relaxed) as f64).collect();
    let recv_total: f64 = per_node.iter().sum();
    latencies.sort_by(f64::total_cmp);
    Report {
        scenario: name,
        strategy,
        nodes: nodes.iter().map(|&i| conns[i].0).collect(),
        k,
        stripe_mib: k * shard_len / MIB,
        seconds: secs,
        stripes: completed,
        goodput_mbit: good * 8.0 / secs / 1e6,
        received_mbit: recv_total * 8.0 / secs / 1e6,
        waste_pct: if recv_total > 0.0 { 100.0 * (recv_total - good).max(0.0) / recv_total } else { 0.0 },
        per_node_mbit: per_node.iter().map(|b| b * 8.0 / secs / 1e6).collect(),
        per_node_share_pct: per_node.iter().map(|b| if recv_total > 0.0 { 100.0 * b / recv_total } else { 0.0 }).collect(),
        stripe_ms_p50: pct(&latencies, 0.5),
        stripe_ms_p90: pct(&latencies, 0.9),
        stripe_ms_max: latencies.last().copied().unwrap_or(0.0),
        hedges,
        failures,
        dropped: (0..nn).filter(|&i| dead[i]).map(|i| conns[nodes[i]].0).collect(),
        timeline_mbit: timeline[..seconds as usize].iter().map(|b| *b as f64 * 8.0 / 1e6).collect(),
        fault: fault.map(|(at, cmd)| format!("at {at} s: {cmd}")),
    }
}

async fn connect(endpoint: &Endpoint, addr: EndpointAddr, until: Instant) -> Result<Connection> {
    let mut last = anyhow!("no attempt");
    while Instant::now() < until {
        match tokio::time::timeout(Duration::from_secs(20), endpoint.connect(addr.clone(), ALPN)).await {
            Ok(Ok(c)) => return Ok(c),
            Ok(Err(e)) => last = anyhow!("{e}"),
            Err(_) => last = anyhow!("timed out"),
        }
        tokio::time::sleep(Duration::from_secs(3)).await;
    }
    Err(last).with_context(|| format!("connect {}", addr.id))
}

#[allow(clippy::too_many_arguments)]
async fn fetch(
    addrs: Vec<EndpointAddr>,
    p: Params,
    strategies: Vec<Strategy>,
    connect_seconds: u64,
    shutdown: bool,
    json: Option<PathBuf>,
    label: String,
    net: Net,
    fault: Option<(u64, String)>,
) -> Result<()> {
    let n = addrs.len();
    if p.k == 0 || p.k > n || n > 64 {
        bail!("need 0 < k <= nodes <= 64");
    }
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_nanos();
    let endpoint = bind(SecretKey::from_bytes(&blake3::derive_key("ursa s2 client key", &nanos.to_le_bytes())), &net).await?;

    // Nodes that cannot be reached are left out, as a real read would leave them out.
    let until = Instant::now() + Duration::from_secs(connect_seconds);
    let t = Instant::now();
    let attempts = join_all(addrs.iter().map(|a| Box::pin(connect(&endpoint, a.clone(), until))).collect()).await;
    let connect_s = t.elapsed().as_secs_f64();
    let mut conns: Vec<(usize, Connection)> = Vec::new();
    let mut offline: Vec<usize> = Vec::new();
    for (i, r) in attempts.into_iter().enumerate() {
        match r {
            Ok(c) => conns.push((i, c)),
            Err(e) => {
                eprintln!("n{i} offline: {e:#}");
                offline.push(i);
            }
        }
    }
    let live = conns.len();
    eprintln!("connected to {live} of {n} nodes in {connect_s:.1} s");
    if live < p.k {
        bail!("only {live} nodes reachable, need {}", p.k);
    }

    // Warm up every path (and give hole punching a chance) before measuring.
    let warm = Arc::new(AtomicU64::new(0));
    for _ in 0..3 {
        for (_, c) in &conns {
            let _ = fetch_shard(c, 0, MIB, &warm).await;
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    let paths: Vec<String> = conns.iter().map(|(_, c)| path_of(c)).collect();

    let shard_len = p.stripe / p.k;
    let mut reports = Vec::new();
    for s in &strategies {
        match s {
            Strategy::Single => {
                for pos in 0..live {
                    let sc = Scenario {
                        name: format!("single n{}", conns[pos].0),
                        strategy: *s,
                        nodes: vec![pos],
                        k: 1,
                        shard_len: p.stripe,
                        seconds: (p.seconds / 2).max(5),
                        fault: None,
                    };
                    let r = run_scenario(sc, &conns, p).await;
                    eprintln!("{}: {:.0} Mbit/s", r.scenario, r.goodput_mbit);
                    reports.push(r);
                }
            }
            _ => {
                let sc = Scenario {
                    name: format!("{s:?} {}-of-{live}", p.k).to_lowercase(),
                    strategy: *s,
                    nodes: (0..live).collect(),
                    k: p.k,
                    shard_len,
                    seconds: p.seconds,
                    fault: fault.clone(),
                };
                let r = run_scenario(sc, &conns, p).await;
                eprintln!("{}: {:.0} Mbit/s", r.scenario, r.goodput_mbit);
                reports.push(r);
            }
        }
    }
    let paths_after: Vec<String> = conns.iter().map(|(_, c)| path_of(c)).collect();

    // Report
    let mut out = String::new();
    let _ = writeln!(out, "# S2 results{}", if label.is_empty() { String::new() } else { format!(": {label}") });
    let _ = writeln!(
        out,
        "\n{live} of {n} nodes reachable, {}-of-{n} stripes, {} MiB ({} KiB shards), window {} stripes, {} requests in flight per node. Connected in {connect_s:.1} s. Goodput counts completed stripes only; waste is bytes received that no completed stripe used.",
        p.k,
        p.stripe / MIB,
        shard_len / 1024,
        p.window,
        p.per_node
    );
    if let Some((at, cmd)) = &fault {
        let _ = writeln!(out, "\nFault injected {at} s into each striped scenario: `{cmd}`");
    }
    let _ = writeln!(out, "\n| Node | path after warm-up | path at end |\n| --- | --- | --- |");
    for (pos, (i, _)) in conns.iter().enumerate() {
        let _ = writeln!(out, "| n{i} | {} | {} |", paths[pos], paths_after[pos]);
    }
    for i in &offline {
        let _ = writeln!(out, "| n{i} | offline | offline |");
    }
    let _ = writeln!(out, "\n| Scenario | Goodput Mbit/s | Received Mbit/s | Waste | Stripe p50 / p90 / max ms | Hedges | Dropped | Per node Mbit/s |\n| --- | ---: | ---: | ---: | ---: | ---: | --- | --- |");
    for r in &reports {
        let per: Vec<String> = r.nodes.iter().zip(&r.per_node_mbit).map(|(i, m)| format!("n{i} {m:.0}")).collect();
        let dropped: Vec<String> = r.dropped.iter().map(|i| format!("n{i}")).collect();
        let _ = writeln!(
            out,
            "| {} | {:.0} | {:.0} | {:.0}% | {:.0} / {:.0} / {:.0} | {} | {} | {} |",
            r.scenario,
            r.goodput_mbit,
            r.received_mbit,
            r.waste_pct,
            r.stripe_ms_p50,
            r.stripe_ms_p90,
            r.stripe_ms_max,
            r.hedges,
            if dropped.is_empty() { "-".into() } else { dropped.join(" ") },
            per.join(" · ")
        );
    }
    // Goodput over time, in 5 s buckets, for the striped scenarios.
    let striped: Vec<&Report> = reports.iter().filter(|r| r.strategy != Strategy::Single).collect();
    if let Some(first) = striped.first() {
        let buckets = first.timeline_mbit.len().div_ceil(5);
        let _ = write!(out, "\nGoodput Mbit/s by 5 s window:\n\n| Scenario |");
        for b in 0..buckets {
            let _ = write!(out, " {}-{} s |", b * 5, (b + 1) * 5);
        }
        let _ = write!(out, "\n| --- |");
        for _ in 0..buckets {
            let _ = write!(out, " ---: |");
        }
        let _ = writeln!(out);
        for r in &striped {
            let _ = write!(out, "| {} |", r.scenario);
            for chunk in r.timeline_mbit.chunks(5) {
                let _ = write!(out, " {:.0} |", chunk.iter().sum::<f64>() / chunk.len() as f64);
            }
            let _ = writeln!(out);
        }
    }
    println!("{out}");
    if let Some(path) = json {
        let doc = serde_json::json!({ "label": label, "params": p, "connect_s": connect_s, "offline": offline, "paths": paths, "paths_after": paths_after, "reports": reports });
        std::fs::write(path, serde_json::to_string_pretty(&doc)?)?;
    }

    if shutdown {
        for (_, c) in &conns {
            let _ = tokio::time::timeout(Duration::from_secs(5), send_op(c, OP_SHUTDOWN)).await;
        }
    }
    let _ = tokio::time::timeout(Duration::from_secs(5), endpoint.close()).await;
    Ok(())
}

async fn join_all<F: std::future::Future + Unpin>(mut futs: Vec<F>) -> Vec<F::Output> {
    use std::pin::Pin;
    use std::task::{Context as Cx, Poll};
    let mut outs: Vec<Option<F::Output>> = (0..futs.len()).map(|_| None).collect();
    std::future::poll_fn(move |cx: &mut Cx<'_>| {
        let mut pending = false;
        for (i, f) in futs.iter_mut().enumerate() {
            if outs[i].is_none() {
                match Pin::new(f).poll(cx) {
                    Poll::Ready(v) => outs[i] = Some(v),
                    Poll::Pending => pending = true,
                }
            }
        }
        if pending { Poll::Pending } else { Poll::Ready(outs.iter_mut().map(|o| o.take().unwrap()).collect()) }
    })
    .await
}
