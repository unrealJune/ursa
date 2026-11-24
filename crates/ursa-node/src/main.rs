//! `ursa-node`: an Ursa home node.
//!
//! Stores encrypted objects for its owner (on disk, or in an S3-compatible bucket such as
//! SeaweedFS) and answers `ursa/node/1` over iroh. It never sees a key: every object is
//! ciphertext named by its hash, and it checks each one against that name.

mod server;
mod store;

use std::net::{SocketAddr, SocketAddrV4, Ipv4Addr};
use std::path::PathBuf;

use anyhow::{Context, Result, anyhow};
use clap::Parser;
use iroh::{Endpoint, RelayMap, RelayMode, RelayUrl, SecretKey, TransportAddr};
use iroh::protocol::Router;
use tracing::info;
use ursa_proto::{ALPN, ClaimLink, ticket_of};

use crate::server::{Node, check_claim_code, new_claim_code};
use crate::store::{S3Config, Store};

#[derive(Parser, Debug)]
#[command(version, about = "An Ursa home node")]
struct Args {
    /// Where objects live: `disk` (under --data-dir) or `s3`.
    #[arg(long, env = "URSA_STORE", default_value = "disk")]
    store: String,
    /// Directory for the disk backend, and for the node key when URSA_SECRET_KEY is unset.
    #[arg(long, env = "URSA_DATA_DIR", default_value = "./ursa-node-data")]
    data_dir: PathBuf,

    #[arg(long, env = "URSA_S3_ENDPOINT")]
    s3_endpoint: Option<String>,
    #[arg(long, env = "URSA_S3_BUCKET")]
    s3_bucket: Option<String>,
    #[arg(long, env = "URSA_S3_REGION", default_value = "us-east-1")]
    s3_region: String,

    /// The node's identity as 64 hex characters. Unset: read or create `<data-dir>/node.key`.
    #[arg(long, env = "URSA_SECRET_KEY", hide_env_values = true)]
    secret_key: Option<String>,
    /// Code a device presents to become an owner. Unset: a random one, printed at start.
    #[arg(long, env = "URSA_CLAIM_CODE", hide_env_values = true)]
    claim_code: Option<String>,

    /// Relays to use instead of n0's public ones (comma-separated). Token: URSA_RELAY_TOKEN.
    #[arg(long, env = "URSA_RELAY_URLS", value_delimiter = ',')]
    relay: Vec<String>,
    /// Fixed UDP port to bind (0: any). Fix it when publishing the port (a NodePort, a router
    /// forward) and advertising it with --advertise.
    #[arg(long, env = "URSA_PORT", default_value_t = 0)]
    port: u16,
    /// Extra addresses clients may dial (comma-separated host:port), e.g. a NodePort on the LAN.
    #[arg(long, env = "URSA_ADVERTISE", value_delimiter = ',')]
    advertise: Vec<SocketAddr>,

    /// Plain-HTTP health endpoint for probes (`GET /` answers `ok`). 0 disables it.
    #[arg(long, env = "URSA_HEALTH_PORT", default_value_t = 8080)]
    health_port: u16,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,iroh=warn,noq=warn".into()),
        )
        .init();
    let args = Args::parse();

    let store = match args.store.as_str() {
        "disk" => Store::disk(args.data_dir.join("store")),
        "s3" => Store::s3(S3Config {
            endpoint: args.s3_endpoint.clone().context("URSA_S3_ENDPOINT is required for the s3 store")?,
            bucket: args.s3_bucket.clone().context("URSA_S3_BUCKET is required for the s3 store")?,
            region: args.s3_region.clone(),
            access_key_id: std::env::var("AWS_ACCESS_KEY_ID").context("AWS_ACCESS_KEY_ID is required")?,
            secret_access_key: std::env::var("AWS_SECRET_ACCESS_KEY")
                .context("AWS_SECRET_ACCESS_KEY is required")?,
        })?,
        other => return Err(anyhow!("unknown store {other:?}: use disk or s3")),
    };
    info!("store: {}", store.describe());

    let secret = load_secret(&args)?;
    let claim_code = match &args.claim_code {
        Some(c) => {
            check_claim_code(c)?;
            c.clone()
        }
        None => new_claim_code(),
    };
    let node = Node::new(store, claim_code.clone()).await.context("open the store")?;

    let mut builder = Endpoint::builder(iroh::endpoint::presets::N0).secret_key(secret);
    if !args.relay.is_empty() {
        let urls = args
            .relay
            .iter()
            .map(|u| u.trim().parse::<RelayUrl>().map_err(|e| anyhow!("bad relay URL {u}: {e}")))
            .collect::<Result<Vec<_>>>()?;
        let mut map = RelayMap::from_iter(urls);
        if let Ok(token) = std::env::var("URSA_RELAY_TOKEN") {
            map = map.with_auth_token(token.trim().to_owned());
        }
        builder = builder.relay_mode(RelayMode::Custom(map));
    }
    if args.port != 0 {
        builder = builder
            .bind_addr(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, args.port))
            .map_err(|e| anyhow!("bind UDP port {}: {e}", args.port))?;
    }
    let endpoint = builder.bind().await.context("bind the endpoint")?;
    let router = Router::builder(endpoint.clone()).accept(ALPN, node.clone()).spawn();
    let _ = tokio::time::timeout(std::time::Duration::from_secs(20), endpoint.online()).await;

    let mut addr = endpoint.addr();
    addr.addrs.extend(args.advertise.iter().map(|a| TransportAddr::Ip(*a)));
    info!("node id: {}", endpoint.id());
    info!("ticket: {}", ticket_of(&addr));
    if node.claimed().await {
        info!("claimed; new devices can still be added with the claim code");
    } else if args.claim_code.is_some() {
        info!("unclaimed; claim it with URSA_CLAIM_CODE: ursa-claim:<URSA_CLAIM_CODE>@{}", ticket_of(&addr));
    } else {
        // Printed once, to whoever runs the node, because that person is the owner.
        info!("unclaimed; paste this into the Ursa app: {}", ClaimLink::new(claim_code, addr.clone()));
    }

    if args.health_port != 0 {
        tokio::spawn(health(args.health_port));
    }
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = terminate() => {}
    }
    info!("shutting down");
    router.shutdown().await.ok();
    Ok(())
}

fn load_secret(args: &Args) -> Result<SecretKey> {
    if let Some(hex_key) = &args.secret_key {
        let mut k = [0u8; 32];
        hex::decode_to_slice(hex_key.trim(), &mut k).context("URSA_SECRET_KEY must be 64 hex characters")?;
        return Ok(SecretKey::from_bytes(&k));
    }
    let path = args.data_dir.join("node.key");
    if let Ok(existing) = std::fs::read_to_string(&path) {
        let mut k = [0u8; 32];
        hex::decode_to_slice(existing.trim(), &mut k).context("node.key is corrupt")?;
        return Ok(SecretKey::from_bytes(&k));
    }
    let mut k = [0u8; 32];
    getrandom::fill(&mut k).expect("OS randomness");
    std::fs::create_dir_all(&args.data_dir)?;
    std::fs::write(&path, hex::encode(k))?;
    info!("created a node key at {}", path.display());
    Ok(SecretKey::from_bytes(&k))
}

async fn health(port: u16) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let Ok(listener) = tokio::net::TcpListener::bind((Ipv4Addr::UNSPECIFIED, port)).await else {
        tracing::warn!("health port {port} unavailable");
        return;
    };
    loop {
        let Ok((mut s, _)) = listener.accept().await else { continue };
        tokio::spawn(async move {
            let mut buf = [0u8; 512];
            let _ = s.read(&mut buf).await;
            let _ = s.write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 2\r\nconnection: close\r\n\r\nok").await;
        });
    }
}

#[cfg(unix)]
async fn terminate() {
    let mut s = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).expect("SIGTERM handler");
    s.recv().await;
}

#[cfg(not(unix))]
async fn terminate() {
    std::future::pending::<()>().await
}
