//! Finding an account's nodes from nothing but its root, the way iroh finds endpoints: a pkarr
//! record (signed DNS TXT) published under the account's **locator key** through a pkarr relay.
//!
//! The locator key is derived from the root in its own context, so the record does not reveal the
//! account id. It lists the endpoint ids of the nodes holding the account head: its home nodes and
//! the friends' nodes that store its shards. Devices republish it whenever the head changes.

use anyhow::{Context, Result, anyhow};
use iroh::address_lookup::PkarrRelayClient;
use iroh::{Endpoint, SecretKey};
use iroh_dns::pkarr::SignedPacket;
use ursa_core::account::Account;

/// iroh's own pkarr relay (n0's DNS server, which also answers from the Mainline DHT).
pub const DEFAULT_PKARR_RELAY: &str = "https://dns.iroh.link/pkarr";
const NAME: &str = "_ursa";
/// How long resolvers may cache the record.
const TTL: u32 = 300;

fn locator_key(account: &Account) -> SecretKey {
    SecretKey::from_bytes(&account.locator_seed())
}

/// Publishes "the account head lives on these nodes" to every relay. Best effort: a relay that
/// is down does not stop the others.
fn client(endpoint: &Endpoint, relay: &url::Url) -> Result<PkarrRelayClient> {
    let dns = endpoint.dns_resolver().map_err(|e| anyhow!("no DNS resolver: {e}"))?.clone();
    Ok(PkarrRelayClient::new(relay.clone(), endpoint.tls_config().clone(), dns))
}

pub async fn publish(
    endpoint: &Endpoint,
    relays: &[url::Url],
    account: &Account,
    node_ids: &[String],
) -> Result<usize> {
    if node_ids.is_empty() {
        return Ok(0);
    }
    let values: Vec<String> = node_ids.iter().take(12).map(|id| format!("n={id}")).collect();
    let packet = SignedPacket::from_txt_strings(&locator_key(account), NAME, values, TTL)
        .map_err(|e| anyhow!("build the locator record: {e}"))?;
    let mut ok = 0;
    for relay in relays {
        match client(endpoint, relay)?.publish(&packet).await {
            Ok(()) => ok += 1,
            Err(e) => tracing::warn!(%relay, "locator publish failed: {e}"),
        }
    }
    Ok(ok)
}

/// The node ids listed in the account's locator record, from the first relay that has it.
pub async fn resolve(endpoint: &Endpoint, relays: &[url::Url], account: &Account) -> Result<Vec<String>> {
    let id = locator_key(account).public();
    let mut last = anyhow!("no pkarr relay configured");
    for relay in relays {
        match client(endpoint, relay)?.resolve(id).await {
            Ok(packet) => {
                let ids: Vec<String> = packet
                    .txt_records(NAME)
                    .into_iter()
                    .filter_map(|v| v.strip_prefix("n=").map(str::to_string))
                    .collect();
                return Ok(ids);
            }
            Err(e) => last = anyhow!("{relay}: {e}"),
        }
    }
    Err(last).context("no relay had the account's locator record")
}

pub fn default_relays() -> Vec<url::Url> {
    vec![DEFAULT_PKARR_RELAY.parse().expect("url")]
}
