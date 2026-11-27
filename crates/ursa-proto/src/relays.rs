//! Ursa's own iroh relays, and the access token they take.
//!
//! The relays are Ursa's default (S2, `docs/research/s2-transfer`: n0's public relays carried
//! almost nothing). They require a token. Release builds bake one in at compile time from
//! `URSA_DEFAULT_RELAY_TOKEN` (CI passes the repository's `URSA_RELAY_TOKEN` secret), so a new
//! install connects without anyone typing it. A token in a shipped binary is not a secret: it
//! keeps casual traffic off the relays and can be rotated by a release, nothing more.
//!
//! The built-in token is presented only when every relay is one of these, so it never reaches
//! a relay someone else runs. A token the user configured always wins.

/// The relays every Ursa app starts with.
pub const DEFAULT_RELAYS: &[&str] = &["https://iroh-euw.streetcrypt.id", "https://iroh-wus.streetcrypt.id"];

/// The token this build was given for [`DEFAULT_RELAYS`], if any.
pub const DEFAULT_RELAY_TOKEN: Option<&str> = option_env!("URSA_DEFAULT_RELAY_TOKEN");

/// The token to present to `relays`: the configured one, else the built-in one when every relay
/// is one of Ursa's own.
pub fn relay_token<S: AsRef<str>>(relays: &[S], configured: Option<&str>) -> Option<String> {
    choose(relays, configured, DEFAULT_RELAY_TOKEN)
}

fn choose<S: AsRef<str>>(relays: &[S], configured: Option<&str>, built_in: Option<&str>) -> Option<String> {
    if let Some(t) = configured.map(str::trim).filter(|t| !t.is_empty()) {
        return Some(t.to_owned());
    }
    let built_in = built_in.map(str::trim).filter(|t| !t.is_empty())?;
    let ours = |r: &str| DEFAULT_RELAYS.iter().any(|d| same_url(d, r));
    let relays: Vec<&str> = relays.iter().map(|r| r.as_ref().trim()).filter(|r| !r.is_empty()).collect();
    (!relays.is_empty() && relays.iter().all(|r| ours(r))).then(|| built_in.to_owned())
}

fn same_url(a: &str, b: &str) -> bool {
    a.trim_end_matches('/').eq_ignore_ascii_case(b.trim().trim_end_matches('/'))
}

#[cfg(test)]
mod tests {
    use super::*;

    const OURS: &[&str] = &["https://iroh-euw.streetcrypt.id/", " https://IROH-WUS.streetcrypt.id"];

    #[test]
    fn configured_token_wins() {
        assert_eq!(choose(OURS, Some(" mine "), Some("baked")).as_deref(), Some("mine"));
        assert_eq!(choose(&["https://relay.example"], Some("mine"), Some("baked")).as_deref(), Some("mine"));
    }

    #[test]
    fn built_in_token_only_for_our_relays() {
        assert_eq!(choose(OURS, None, Some("baked")).as_deref(), Some("baked"));
        assert_eq!(choose(OURS, Some("  "), Some("baked")).as_deref(), Some("baked"));
        assert_eq!(choose(&[OURS[0], "https://relay.example"], None, Some("baked")), None);
        assert_eq!(choose::<&str>(&[], None, Some("baked")), None);
    }

    #[test]
    fn nothing_without_a_token() {
        assert_eq!(choose(OURS, None, None), None);
        assert_eq!(choose(OURS, None, Some("")), None);
    }
}
