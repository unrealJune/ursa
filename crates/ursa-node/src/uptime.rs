//! How much of the last 30 days the node was running. The node records the spans it was up
//! (start, last heartbeat) in its store; a span ends where the heartbeats stopped. Friends' apps
//! show the figure, and the app's availability estimate is built from it.

use serde::{Deserialize, Serialize};

pub const WINDOW_MS: i64 = 30 * 24 * 3600 * 1000;
/// How often the current span is extended and saved.
pub const HEARTBEAT_MS: i64 = 60 * 1000;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Uptime {
    /// `[start_ms, last_seen_ms]`, oldest first.
    pub spans: Vec<[i64; 2]>,
    /// When the node first ran. Kept when old spans are dropped, so a long gap still counts.
    #[serde(default)]
    pub first_ms: Option<i64>,
}

impl Uptime {
    /// A new span starting now; spans older than the window are dropped.
    pub fn start(&mut self, now: i64) {
        self.first_ms.get_or_insert(self.spans.first().map_or(now, |s| s[0]));
        self.spans.retain(|s| s[1] > now - WINDOW_MS);
        self.spans.push([now, now]);
    }

    /// Extends the current span to `now`.
    pub fn beat(&mut self, now: i64) {
        match self.spans.last_mut() {
            Some(s) => s[1] = now,
            None => {
                self.first_ms.get_or_insert(now);
                self.spans.push([now, now]);
            }
        }
    }

    /// When the node first ran, within the window.
    pub fn since(&self) -> Option<i64> {
        self.first_ms.or_else(|| self.spans.first().map(|s| s[0]))
    }

    /// Fraction of the window (or of the time since the first start, if shorter) the node was up.
    pub fn fraction(&self, now: i64) -> Option<f64> {
        let from = (now - WINDOW_MS).max(self.since()?);
        let total = now - from;
        if total < HEARTBEAT_MS {
            return Some(1.0);
        }
        let up: i64 = self.spans.iter().map(|s| (s[1].min(now) - s[0].max(from)).max(0)).sum();
        Some((up as f64 / total as f64).clamp(0.0, 1.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const H: i64 = 3600 * 1000;

    #[test]
    fn a_node_up_the_whole_time_is_at_one() {
        let mut u = Uptime::default();
        u.start(0);
        u.beat(10 * H);
        assert_eq!(u.fraction(10 * H), Some(1.0));
    }

    #[test]
    fn a_gap_counts_as_down() {
        let mut u = Uptime::default();
        u.start(0);
        u.beat(9 * H);
        u.start(10 * H); // off for an hour
        u.beat(20 * H);
        let f = u.fraction(20 * H).unwrap();
        assert!((f - 0.95).abs() < 1e-9, "{f}");
    }

    #[test]
    fn only_the_last_30_days_count() {
        let mut u = Uptime::default();
        u.start(0);
        u.beat(10 * 24 * H); // up 10 days, then down 40
        u.start(50 * 24 * H);
        u.beat(60 * 24 * H);
        // Window is days 30..60: up for days 50..60 only.
        let f = u.fraction(60 * 24 * H).unwrap();
        assert!((f - 1.0 / 3.0).abs() < 1e-9, "{f}");
        // Starting again prunes the span that left the window.
        u.start(61 * 24 * H);
        assert_eq!(u.spans.len(), 2);
    }

    #[test]
    fn never_run_has_no_figure() {
        assert_eq!(Uptime::default().fraction(5), None);
    }
}
