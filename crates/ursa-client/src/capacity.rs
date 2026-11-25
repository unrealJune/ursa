//! What each protection choice lets an account store, and how often it would leave a file out
//! of reach. Pure arithmetic over the pool's numbers, so the app and the CLI agree.

use serde::Serialize;
use ursa_core::coding::Code;

/// The shapes the app offers. More appear (custom) when the account chose something else.
pub const PRESETS: [(u16, u16); 7] = [(1, 2), (4, 6), (3, 5), (1, 3), (8, 12), (6, 10), (4, 8)];

/// 99.99% readable: under 53 minutes a year.
pub const TARGET_HOURS: f64 = 8760.0 * 1e-4;

/// Assumed uptime for a node nobody has measured yet.
pub const UNKNOWN_UP: f64 = 0.98;

/// The most an account can store with `code`, in bytes of its own files.
///
/// Each failure domain holds at most one piece of each stripe, so at most `D/k` of `D` bytes of
/// files. Friends' domains hold at most what each gives this account, and all together at most
/// what this account gives its friends (fair share). The account's own domain is limited only
/// by its own space, and does not count towards fair share.
pub fn limit(code: Code, own_cap: Option<u64>, friend_caps: &[u64], give: u64) -> u64 {
    let (k, n) = (code.k as f64, code.n as f64);
    let domains = friend_caps.len() + own_cap.is_some() as usize;
    if (code.n as usize) > domains {
        return 0;
    }
    let fits = |d: f64| {
        let per = d / k;
        let own = own_cap.map_or(0.0, |c| (c as f64).min(per));
        let friends: f64 = friend_caps.iter().map(|&c| (c as f64).min(per)).sum();
        own + friends.min(give as f64) >= d * n / k * (1.0 - 1e-12)
    };
    let mut hi = own_cap.map_or(0.0, |c| c as f64) + friend_caps.iter().map(|&c| c as f64).sum::<f64>();
    if hi <= 0.0 {
        return 0;
    }
    hi *= k; // never more than this
    let mut lo = 0.0;
    for _ in 0..100 {
        let mid = (lo + hi) / 2.0;
        if fits(mid) { lo = mid } else { hi = mid }
    }
    lo.floor() as u64
}

/// Expected hours a year a file is out of reach: more than `n - k` of its `n` domains down at
/// once, each down at the pool's mean measured rate. `None` if the pool has too few domains.
pub fn offline_hours(code: Code, ups: &[f64]) -> Option<f64> {
    let n = code.n as usize;
    if n > ups.len() || ups.is_empty() {
        return None;
    }
    let q = 1.0 - ups.iter().sum::<f64>() / ups.len() as f64;
    // P(more than n-k of n down), binomial.
    let mut p = 0.0;
    for down in (n - code.k as usize + 1)..=n {
        p += binom(n, down) * q.powi(down as i32) * (1.0 - q).powi((n - down) as i32);
    }
    Some(p * 8760.0)
}

fn binom(n: usize, r: usize) -> f64 {
    (0..r).fold(1.0, |acc, i| acc * (n - i) as f64 / (i + 1) as f64)
}

/// One choice as the app shows it.
#[derive(Clone, Debug, Serialize, serde::Deserialize)]
pub struct Choice {
    pub k: u16,
    pub n: u16,
    /// Nodes that can be down with every file still readable.
    pub survives: u16,
    pub limit_bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offline_hours: Option<f64>,
    /// The pool has enough domains for it.
    pub available: bool,
    pub recommended: bool,
}

/// Every preset plus `current` (if it is not one), with limits and availability, and the
/// cheapest one that is readable 99.99% of the time marked as recommended.
pub fn choices(current: Code, own_cap: Option<u64>, friend_caps: &[u64], give: u64, ups: &[f64]) -> Vec<Choice> {
    let domains = friend_caps.len() + own_cap.is_some() as usize;
    let mut codes: Vec<Code> = PRESETS.iter().map(|&(k, n)| Code { k, n }).collect();
    if !codes.contains(&current) {
        codes.push(current);
    }
    let mut out: Vec<Choice> = codes
        .into_iter()
        .map(|c| Choice {
            k: c.k,
            n: c.n,
            survives: c.n - c.k,
            limit_bytes: limit(c, own_cap, friend_caps, give),
            offline_hours: offline_hours(c, ups),
            available: c.n as usize <= domains,
            recommended: false,
        })
        .collect();
    let best = out
        .iter()
        .enumerate()
        .filter(|(_, c)| c.available && c.offline_hours.is_some_and(|h| h <= TARGET_HOURS))
        .min_by(|(_, a), (_, b)| {
            let (ra, rb) = (a.n as f64 / a.k as f64, b.n as f64 / b.k as f64);
            ra.partial_cmp(&rb).unwrap().then(b.survives.cmp(&a.survives))
        })
        .map(|(i, _)| i);
    if let Some(i) = best {
        out[i].recommended = true;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const GB: u64 = 1_000_000_000;

    fn c(k: u16, n: u16) -> Code {
        Code::new(k, n).unwrap()
    }

    fn near(a: u64, b: u64) -> bool {
        a.abs_diff(b) <= b / 1000 + 1
    }

    // The prototype's pool: a big own node, five friends, 1 TB given.
    const FRIENDS: [u64; 5] = [500 * GB, 800 * GB, 300 * GB, 250 * GB, 400 * GB];

    #[test]
    fn fair_share_binds_when_friends_have_room() {
        let own = Some(2900 * GB);
        // 3-of-5: 1/3 at home, 4/3 on friends ≤ 1 TB → 750 GB.
        assert!(near(limit(c(3, 5), own, &FRIENDS, 1000 * GB), 750 * GB));
        // Two copies: one at home, one on friends → 1 TB.
        assert!(near(limit(c(1, 2), own, &FRIENDS, 1000 * GB), 1000 * GB));
        // Three copies: two on friends → 500 GB.
        assert!(near(limit(c(1, 3), own, &FRIENDS, 1000 * GB), 500 * GB));
        // 4-of-6: 5/4 on friends → 800 GB.
        assert!(near(limit(c(4, 6), own, &FRIENDS, 1000 * GB), 800 * GB));
    }

    #[test]
    fn small_friends_bind_when_giving_is_generous() {
        // Give everything: now the smallest grants run out first. For 3-of-5 each domain holds
        // D/3; Priya (300) and Sam (250) cap out. D=1450 → per 483: own 483, friends
        // 483+483+300+250+400 = 1916; 2399 < 2416. Binding just under 1450.
        let l = limit(c(3, 5), Some(2900 * GB), &FRIENDS, 10_000 * GB);
        assert!(l > 1400 * GB && l < 1450 * GB, "{l}");
    }

    #[test]
    fn too_few_domains_is_zero() {
        assert_eq!(limit(c(4, 8), Some(GB), &FRIENDS, GB), 0);
        assert_eq!(limit(c(1, 2), None, &[], 0), 0);
    }

    #[test]
    fn nothing_given_means_only_what_fits_at_home() {
        // With fair share at zero, friends take nothing, so no code with n ≥ 2 fits anything.
        assert_eq!(limit(c(1, 2), Some(100 * GB), &FRIENDS, 0), 0);
    }

    #[test]
    fn offline_hours_match_the_plan_table() {
        // PLAN.md: 4-of-6 at 95% uptime ≈ 20 h a year; 6-of-10 ≈ 0.6 h.
        let h = offline_hours(c(4, 6), &[0.95; 6]).unwrap();
        assert!((h - 20.0).abs() < 1.5, "{h}");
        let h = offline_hours(c(6, 10), &[0.95; 10]).unwrap();
        assert!((h - 0.6).abs() < 0.1, "{h}");
        assert_eq!(offline_hours(c(3, 5), &[0.99; 4]), None);
    }

    #[test]
    fn recommends_the_cheapest_that_meets_the_target() {
        let ups = [0.995, 0.98, 0.97, 0.99, 0.96];
        let ch = choices(c(3, 5), Some(2900 * GB), &FRIENDS[..4], 1000 * GB, &ups);
        let rec: Vec<_> = ch.iter().filter(|c| c.recommended).map(|c| (c.k, c.n)).collect();
        assert_eq!(rec, vec![(3, 5)]);
        assert!(!ch.iter().find(|c| (c.k, c.n) == (4, 6)).unwrap().available);
    }

    #[test]
    fn a_custom_choice_is_listed() {
        let ch = choices(c(2, 4), Some(GB), &FRIENDS, GB, &[0.99; 6]);
        assert!(ch.iter().any(|c| (c.k, c.n) == (2, 4)));
    }
}
