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
        Cmd::Serve { seed, index, max_seconds, ticket_out } => serve(seed, index, max_seconds, ticket_out).await,
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
        } => {
            let addrs: Vec<EndpointAddr> = if !peers.is_empty() {
                peers.iter().map(|p| parse_peer(p)).collect::<Result<_>>()?
            } else {
                let seed = seed.ok_or_else(|| anyhow!("pass --peer or --seed"))?;
                (0..nodes).map(|i| EndpointAddr::new(node_secret(&seed, i).public())).collect()
            };
            let params = Params { k, stripe: stripe_mib * MIB, seconds, window, per_node };
            fetch(addrs, params, strategies, connect_seconds, shutdown, json, label).await
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

async fn serve(seed: String, index: usize, max_seconds: u64, ticket_out: Option<PathBuf>) -> Result<()> {
    let endpoint = Endpoint::builder(iroh::endpoint::presets::N0)
        .secret_key(node_secret(&seed, index))
        .bind()
        .await?;
    let mut data = vec![0u8; SERVE_BUF];
    blake3::Hasher::new().update(seed.as_bytes()).finalize_xof().fill(&mut data);
    let shutdown = Arc::new(Notify::new());
    let router = Router::builder(endpoint.clone())
        .accept(ALPN, Server { data: Arc::new(data), shutdown: shutdown.clone() })
        .spawn();
    let _ = tokio::time::timeout(Duration::from_secs(30), endpoint.online()).await;
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

fn path_of(conn: &Connection) -> String {
    let paths = conn.paths();
    let selected = paths.iter().find(|p| p.is_selected());
    match selected {
        Some(p) if p.is_ip() => format!("direct {:?}", p.remote_addr()),
        Some(p) if p.is_relay() => format!("relay {:?}", p.remote_addr()),
        Some(p) => format!("other {:?}", p.remote_addr()),
        None => "none".into(),
    }
}

struct StripeState {
    started: Instant,
    done: usize,
    /// Scenario-node positions with a request outstanding for this stripe.
    pending: Vec<usize>,
    /// Bitmask of positions ever asked for this stripe.
    asked: u64,
    extras: usize,
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
}

fn pct(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    sorted[((sorted.len() - 1) as f64 * p).round() as usize]
}

#[allow(clippy::too_many_arguments)]
/// Runs one scenario: `nodes` are indices into `conns`; `k` shards of `shard_len` per stripe.
async fn run_scenario(
    name: String,
    strategy: Strategy,
    conns: &[Connection],
    nodes: Vec<usize>,
    k: usize,
    shard_len: usize,
    p: Params,
    seconds: u64,
) -> Report {
    let nn = nodes.len();
    let (tx, mut rx) = mpsc::unbounded_channel::<Event>();
    let received: Vec<Arc<AtomicU64>> = (0..nn).map(|_| Arc::new(AtomicU64::new(0))).collect();
    let mut inflight: BTreeMap<u64, StripeState> = BTreeMap::new();
    let mut handles: HashMap<(u64, usize), AbortHandle> = HashMap::new();
    let mut busy = vec![0usize; nn];
    let mut next_stripe = 0u64;
    let mut completed = 0u64;
    let mut latencies: Vec<f64> = Vec::new();
    let mut recent: VecDeque<f64> = VecDeque::new();
    let (mut hedges, mut failures) = (0u64, 0u64);

    let start = Instant::now();
    let deadline = start + Duration::from_secs(seconds);

    loop {
        let now = Instant::now();
        if now >= deadline {
            break;
        }
        // Overdue: 1.5 × the median recent stripe time, at least 200 ms.
        let hedge_after = if recent.len() >= 4 {
            let mut v: Vec<f64> = recent.iter().copied().collect();
            v.sort_by(f64::total_cmp);
            Some(Duration::from_secs_f64((pct(&v, 0.5) * 1.5 / 1000.0).max(0.2)))
        } else {
            None
        };

        // Hand out work until no node can take any.
        loop {
            let mut progressed = false;
            for pos in 0..nn {
                while busy[pos] < p.per_node {
                    let bit = 1u64 << pos;
                    // Which shards this position may serve, by strategy.
                    let eligible = match strategy {
                        Strategy::Single | Strategy::Race | Strategy::Pull => true,
                        Strategy::Fixed => pos < k,
                    };
                    if !eligible {
                        break;
                    }
                    // 1. The oldest stripe still owed by this node.
                    let mut pick = inflight.iter().find_map(|(&s, st)| {
                        let wants = match strategy {
                            Strategy::Pull => st.done + st.pending.len() < k,
                            Strategy::Single | Strategy::Fixed | Strategy::Race => true,
                        };
                        (wants && st.asked & bit == 0).then_some((s, false))
                    });
                    // 2. A new stripe, if the window allows.
                    if pick.is_none() && inflight.len() < p.window {
                        let s = next_stripe;
                        next_stripe += 1;
                        inflight.insert(s, StripeState { started: Instant::now(), done: 0, pending: Vec::new(), asked: 0, extras: 0 });
                        pick = Some((s, false));
                    }
                    // 3. Pull only: duplicate a shard of the overdue oldest stripe.
                    if pick.is_none() && strategy == Strategy::Pull {
                        if let Some(after) = hedge_after {
                            pick = inflight.iter().find_map(|(&s, st)| {
                                (st.done < k
                                    && st.asked & bit == 0
                                    && st.extras < nn - k
                                    && st.started.elapsed() > after)
                                    .then_some((s, true))
                            });
                        }
                    }
                    let Some((stripe, extra)) = pick else { break };
                    let st = inflight.get_mut(&stripe).unwrap();
                    st.asked |= bit;
                    st.pending.push(pos);
                    if extra {
                        st.extras += 1;
                        hedges += 1;
                    }
                    busy[pos] += 1;
                    let conn = conns[nodes[pos]].clone();
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
        let Some(st) = inflight.get_mut(&ev.stripe) else { continue };
        st.pending.retain(|&q| q != ev.pos);
        if ev.ok {
            st.done += 1;
        } else {
            failures += 1;
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
            for q in st.pending {
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
        nodes,
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
) -> Result<()> {
    let n = addrs.len();
    if p.k == 0 || p.k > n || n > 64 {
        bail!("need 0 < k <= nodes <= 64");
    }
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_nanos();
    let endpoint = Endpoint::builder(iroh::endpoint::presets::N0)
        .secret_key(SecretKey::from_bytes(&blake3::derive_key("ursa s2 client key", &nanos.to_le_bytes())))
        .bind()
        .await?;
    let _ = tokio::time::timeout(Duration::from_secs(30), endpoint.online()).await;

    let until = Instant::now() + Duration::from_secs(connect_seconds);
    let t = Instant::now();
    let conns: Vec<Connection> = futures_join(addrs.iter().map(|a| connect(&endpoint, a.clone(), until))).await?;
    let connect_s = t.elapsed().as_secs_f64();
    eprintln!("connected to {n} nodes in {connect_s:.1} s");

    // Warm up every path (and give hole punching a chance) before measuring.
    let warm = Arc::new(AtomicU64::new(0));
    for _ in 0..3 {
        for c in &conns {
            let _ = fetch_shard(c, 0, MIB, &warm).await;
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    let paths: Vec<String> = conns.iter().map(path_of).collect();

    let shard_len = p.stripe / p.k;
    let mut reports = Vec::new();
    for s in &strategies {
        match s {
            Strategy::Single => {
                for i in 0..n {
                    let r = run_scenario(format!("single n{i}"), *s, &conns, vec![i], 1, p.stripe, p, (p.seconds / 2).max(5)).await;
                    eprintln!("{}: {:.0} Mbit/s", r.scenario, r.goodput_mbit);
                    reports.push(r);
                }
            }
            _ => {
                let name = format!("{s:?} {}-of-{n}", p.k).to_lowercase();
                let r = run_scenario(name, *s, &conns, (0..n).collect(), p.k, shard_len, p, p.seconds).await;
                eprintln!("{}: {:.0} Mbit/s", r.scenario, r.goodput_mbit);
                reports.push(r);
            }
        }
    }
    let paths_after: Vec<String> = conns.iter().map(path_of).collect();

    // Report
    let mut out = String::new();
    let _ = writeln!(out, "# S2 results{}", if label.is_empty() { String::new() } else { format!(": {label}") });
    let _ = writeln!(
        out,
        "\n{n} nodes, {}-of-{n}, {} MiB stripes ({} KiB shards), window {} stripes, {} requests in flight per node. Connected in {connect_s:.1} s. Goodput counts completed stripes only; waste is bytes received that no completed stripe used.",
        p.k,
        p.stripe / MIB,
        shard_len / 1024,
        p.window,
        p.per_node
    );
    let _ = writeln!(out, "\n| Node | path after warm-up | path at end |\n| --- | --- | --- |");
    for i in 0..n {
        let _ = writeln!(out, "| n{i} | {} | {} |", paths[i], paths_after[i]);
    }
    let _ = writeln!(out, "\n| Scenario | Goodput Mbit/s | Received Mbit/s | Waste | Stripe p50 / p90 / max ms | Hedges | Per node Mbit/s |\n| --- | ---: | ---: | ---: | ---: | ---: | --- |");
    for r in &reports {
        let per: Vec<String> = r
            .nodes
            .iter()
            .zip(&r.per_node_mbit)
            .map(|(i, m)| format!("n{i} {m:.0}"))
            .collect();
        let _ = writeln!(
            out,
            "| {} | {:.0} | {:.0} | {:.0}% | {:.0} / {:.0} / {:.0} | {} | {} |",
            r.scenario,
            r.goodput_mbit,
            r.received_mbit,
            r.waste_pct,
            r.stripe_ms_p50,
            r.stripe_ms_p90,
            r.stripe_ms_max,
            r.hedges,
            per.join(" · ")
        );
    }
    println!("{out}");
    if let Some(path) = json {
        let doc = serde_json::json!({ "label": label, "params": p, "connect_s": connect_s, "paths": paths, "paths_after": paths_after, "reports": reports });
        std::fs::write(path, serde_json::to_string_pretty(&doc)?)?;
    }

    if shutdown {
        for c in &conns {
            let _ = send_op(c, OP_SHUTDOWN).await;
        }
    }
    endpoint.close().await;
    Ok(())
}

/// Joins futures concurrently, in order, failing on the first error.
async fn futures_join<F, T>(futs: impl Iterator<Item = F>) -> Result<Vec<T>>
where
    F: std::future::Future<Output = Result<T>>,
{
    let mut set = Vec::new();
    for f in futs {
        set.push(Box::pin(f));
    }
    let mut out = Vec::with_capacity(set.len());
    let results = join_all(set).await;
    for r in results {
        out.push(r?);
    }
    Ok(out)
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
