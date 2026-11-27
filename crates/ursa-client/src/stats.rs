//! Timings for one upload, download or index commit, logged as a single summary line when it
//! ends, so telemetry shows where the time went: preparing, sealing, coding and placing shards,
//! which node was slow and over which path. Never a file name, file id or shard id: node ids,
//! sizes and times only (docs/design.md, Observability).
//!
//! The shard code records into the operation that is running through a task-local, so the
//! object store needs no extra parameters and work outside an operation (repair, audits) records
//! nothing. Everything here runs on the operation's own task (`buffered` streams are polled by
//! it), which is what makes a task-local enough.

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

tokio::task_local! {
    static CURRENT: Stats;
}

/// What one node did for one operation.
#[derive(Default, Clone)]
struct NodeStats {
    ok: u32,
    failed: u32,
    bytes: u64,
    total: Duration,
    slowest: Duration,
}

#[derive(Default)]
struct Inner {
    nodes: BTreeMap<String, NodeStats>,
    /// Extra shard requests a download made because one was slow (after HEDGE_AFTER).
    hedges: u32,
    /// Objects a download rebuilt from parity rather than read from its data shards.
    decoded: u32,
    /// Named stages, in the order they were first recorded.
    stages: Vec<(&'static str, Duration)>,
}

#[derive(Default)]
pub(crate) struct Stats {
    inner: Mutex<Inner>,
}

impl Stats {
    /// Runs `f` with this operation's recorder in scope.
    pub(crate) async fn scope<F: std::future::Future>(self, f: F) -> (Self, F::Output) {
        CURRENT
            .scope(self, async move {
                let out = f.await;
                (CURRENT.with(|s| s.take()), out)
            })
            .await
    }

    fn take(&self) -> Stats {
        Stats { inner: Mutex::new(std::mem::take(&mut *self.inner.lock().expect("stats"))) }
    }

    /// A shard put or get on `node`: its size, how long it took, whether it worked.
    pub(crate) fn shard(node: &str, bytes: usize, took: Duration, ok: bool) {
        let _ = CURRENT.try_with(|s| {
            let mut g = s.inner.lock().expect("stats");
            let n = g.nodes.entry(short(node)).or_default();
            if ok {
                n.ok += 1;
                n.bytes += bytes as u64;
            } else {
                n.failed += 1;
            }
            n.total += took;
            n.slowest = n.slowest.max(took);
        });
    }

    pub(crate) fn hedge() {
        let _ = CURRENT.try_with(|s| s.inner.lock().expect("stats").hedges += 1);
    }

    pub(crate) fn decoded() {
        let _ = CURRENT.try_with(|s| s.inner.lock().expect("stats").decoded += 1);
    }

    /// Adds `took` to the named stage (a stage recorded several times sums).
    pub(crate) fn stage(name: &'static str, took: Duration) {
        let _ = CURRENT.try_with(|s| {
            let mut g = s.inner.lock().expect("stats");
            match g.stages.iter_mut().find(|(n, _)| *n == name) {
                Some((_, d)) => *d += took,
                None => g.stages.push((name, took)),
            }
        });
    }

    /// `stage=ms` pairs, in order: `pool=210ms seal=95ms place=1840ms`.
    pub(crate) fn stages(&self) -> String {
        let g = self.inner.lock().expect("stats");
        g.stages.iter().map(|(n, d)| format!("{n}={}ms", d.as_millis())).collect::<Vec<_>>().join(" ")
    }

    /// One entry per node: `911c18a4ca[relay] 12 ok 3.1MB avg 410ms max 1290ms 0.6MB/s`, with
    /// `path` naming how this device reaches it now.
    pub(crate) fn nodes(&self, path: impl Fn(&str) -> String) -> String {
        let g = self.inner.lock().expect("stats");
        g.nodes
            .iter()
            .map(|(id, n)| {
                let calls = n.ok + n.failed;
                let avg = if calls > 0 { n.total.as_millis() / calls as u128 } else { 0 };
                let mut s = format!("{id}[{}] {} ok", path(id), n.ok);
                if n.failed > 0 {
                    s.push_str(&format!(" {} failed", n.failed));
                }
                s.push_str(&format!(" {} avg {avg}ms max {}ms", mb(n.bytes), n.slowest.as_millis()));
                if !n.total.is_zero() && n.bytes > 0 {
                    s.push_str(&format!(" {}", rate(n.bytes, n.total)));
                }
                s
            })
            .collect::<Vec<_>>()
            .join(", ")
    }

    pub(crate) fn hedges(&self) -> u32 {
        self.inner.lock().expect("stats").hedges
    }

    pub(crate) fn decodes(&self) -> u32 {
        self.inner.lock().expect("stats").decoded
    }
}

/// Times a stage of the running operation: `let _t = Stage::start("seal");`.
pub(crate) struct Stage {
    name: &'static str,
    started: Instant,
}

impl Stage {
    pub(crate) fn start(name: &'static str) -> Self {
        Stage { name, started: Instant::now() }
    }
}

impl Drop for Stage {
    fn drop(&mut self) {
        Stats::stage(self.name, self.started.elapsed());
    }
}

fn short(id: &str) -> String {
    id.chars().take(10).collect()
}

pub(crate) fn mb(bytes: u64) -> String {
    format!("{:.1}MB", bytes as f64 / 1_000_000.0)
}

/// Throughput, `1.8MB/s`.
pub(crate) fn rate(bytes: u64, took: Duration) -> String {
    let secs = took.as_secs_f64().max(0.001);
    format!("{:.2}MB/s", bytes as f64 / 1_000_000.0 / secs)
}
