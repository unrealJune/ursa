//! `ursa-host`: what a desktop app runs to be an Ursa device.
//!
//! The app owns the account root and keeps it under the OS's protection; it hands it over in
//! `init` (or receives it from `create`, `recover` or `link_join`) and the host keeps it in
//! memory only. Everything else (the pool, coded storage, recovery, linking) is
//! [`ursa_client`].
//!
//! Wire: one JSON object per line. Requests carry an `id` and a `cmd`; every request gets one
//! response with the same `id` (`ok` plus fields, or `ok: false` and `error`). Long operations
//! also emit events first: `{"event":"progress","id","done","total"}`, `{"event":"link_confirm",
//! "id","code"}` while joining, and `{"event":"link_request","id","name","platform","code"}`
//! while offering, answered with `link_decide`. Logs go to stderr.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{Mutex, mpsc, oneshot};
use ursa_client::{Device, NetConfig, Session, UploadMeta};
use ursa_core::account::{Account, KeyFile, recovery_phrase, root_from_phrase};
use ursa_core::crypto::AccountKey;

#[derive(Debug, Deserialize)]
struct Envelope {
    id: u64,
    #[serde(flatten)]
    cmd: Command,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
enum Command {
    Init {
        data_dir: PathBuf,
        /// The account root as hex, if this device has one.
        #[serde(default)]
        account_key: Option<String>,
        #[serde(default)]
        relays: Vec<String>,
        #[serde(default)]
        relay_token: Option<String>,
        #[serde(default)]
        device_name: Option<String>,
        #[serde(default)]
        platform: Option<String>,
    },
    Create { name: String },
    Recover {
        #[serde(default)]
        phrase: Option<String>,
        #[serde(default)]
        key_file: Option<String>,
        #[serde(default)]
        passphrase: Option<String>,
    },
    LinkJoin { code: String },
    LinkOffer,
    LinkDecide { accept: bool },
    LinkCancel,
    Status,
    Pool,
    Claim { link: String, #[serde(default)] label: String },
    ForgetNode { node_id: String },
    Invite,
    Befriend { invite: String, #[serde(default = "default_quota_gb")] quota_gb: u64 },
    RecoveryPhrase,
    KeyFile { passphrase: String },
    List,
    Upload(UploadMeta),
    Thumb { file_id: String },
    Download { file_id: String, out_path: PathBuf },
    Delete { file_id: String },
    Repair,
}

fn default_quota_gb() -> u64 {
    10
}

#[derive(Clone)]
struct Out {
    id: u64,
    tx: mpsc::UnboundedSender<Value>,
}

impl Out {
    fn send(&self, v: Value) {
        let _ = self.tx.send(v);
    }
    fn event(&self, name: &str, mut v: Value) {
        if let Value::Object(m) = &mut v {
            m.insert("event".into(), json!(name));
            m.insert("id".into(), json!(self.id));
        }
        self.send(v);
    }
    fn progress(&self, done: u64, total: u64) {
        self.event("progress", json!({ "done": done, "total": total }));
    }
}

#[derive(Default)]
struct Host {
    device: Mutex<Option<Arc<Device>>>,
    session: Mutex<Option<Arc<Session>>>,
    names: Mutex<(String, String)>,
    /// The decision channel for a link request now on screen.
    decision: Mutex<Option<oneshot::Sender<bool>>>,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info,iroh=warn,noq=warn".into()))
        .init();
    let (tx, mut rx) = mpsc::unbounded_channel::<Value>();
    let writer = tokio::spawn(async move {
        let mut stdout = tokio::io::stdout();
        while let Some(v) = rx.recv().await {
            let mut line = serde_json::to_vec(&v).expect("json");
            line.push(b'\n');
            if stdout.write_all(&line).await.is_err() || stdout.flush().await.is_err() {
                break;
            }
        }
    });
    let host = Arc::new(Host::default());
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        let env: Envelope = match serde_json::from_str(&line) {
            Ok(e) => e,
            Err(e) => {
                let id = serde_json::from_str::<Value>(&line).ok().and_then(|v| v["id"].as_u64());
                let _ = tx.send(json!({ "id": id, "ok": false, "error": format!("bad request: {e}") }));
                continue;
            }
        };
        let out = Out { id: env.id, tx: tx.clone() };
        let host = host.clone();
        tokio::spawn(async move {
            let id = env.id;
            let result = run(&host, env.cmd, &out).await;
            out.send(match result {
                Ok(mut v) => {
                    if let Value::Object(m) = &mut v {
                        m.insert("id".into(), json!(id));
                        m.insert("ok".into(), json!(true));
                    }
                    v
                }
                Err(e) => json!({ "id": id, "ok": false, "error": format!("{e:#}") }),
            });
        });
    }
    if let Some(d) = host.device.lock().await.as_ref() {
        d.shutdown().await;
    }
    drop(tx);
    let _ = writer.await;
    Ok(())
}

impl Host {
    async fn device(&self) -> Result<Arc<Device>> {
        self.device.lock().await.clone().context("send init first")
    }

    async fn session(&self) -> Result<Arc<Session>> {
        self.session.lock().await.clone().context("this device has no account yet")
    }

    /// The account root as the app stores it, plus what it shows during onboarding.
    fn account_result(session: &Session) -> Value {
        json!({
            "account_id": session.account_id(),
            "account_key": hex_root(session.account.root()),
        })
    }

    async fn status(&self) -> Result<Value> {
        let device = self.device().await?;
        let Some(s) = self.session.lock().await.clone() else {
            return Ok(json!({ "has_account": false, "device_id": device.id() }));
        };
        let st = s.state().await;
        Ok(json!({
            "has_account": true,
            "device_id": device.id(),
            "account_id": s.account_id(),
            "name": st.name,
            "seq": s.seq().await,
            "devices": st.devices,
            "home_nodes": st.home_nodes,
            "friends": st.friends.iter().map(|f| json!({ "account": f.account, "name": f.name, "nodes": f.nodes.len() })).collect::<Vec<_>>(),
        }))
    }
}

fn hex_root(root: &AccountKey) -> String {
    root.as_bytes().iter().map(|b| format!("{b:02x}")).collect()
}

async fn run(host: &Host, cmd: Command, out: &Out) -> Result<Value> {
    match cmd {
        Command::Init { data_dir, account_key, relays, relay_token, device_name, platform } => {
            let device = Device::start(&data_dir, &NetConfig { relays, relay_token, pkarr: vec![] }).await?;
            let names = (device_name.unwrap_or_else(|| "This PC".into()), platform.unwrap_or_else(|| "windows".into()));
            *host.names.lock().await = names.clone();
            *host.device.lock().await = Some(device.clone());
            if let Some(hex) = account_key {
                let root = AccountKey::from_hex(&hex).map_err(|e| anyhow!("{e}"))?;
                match device.open(root, &names.0, &names.1).await {
                    Ok(s) => *host.session.lock().await = Some(s),
                    // The app shows this and keeps the root: the account's nodes may be offline.
                    Err(e) => return Ok(json!({ "device_id": device.id(), "has_account": false, "open_error": format!("{e:#}") })),
                }
            }
            host.status().await
        }
        Command::Create { name } => {
            let device = host.device().await?;
            let (dn, pf) = host.names.lock().await.clone();
            let s = device.create_account(&name, &dn, &pf).await?;
            let v = Host::account_result(&s);
            *host.session.lock().await = Some(s);
            Ok(v)
        }
        Command::Recover { phrase, key_file, passphrase } => {
            let device = host.device().await?;
            let (dn, pf) = host.names.lock().await.clone();
            let account = match (phrase, key_file) {
                (Some(p), _) => Account::from_root(root_from_phrase(&p).map_err(|e| anyhow!("{e}"))?),
                (None, Some(k)) => KeyFile::from_json(&k)
                    .and_then(|kf| kf.open(passphrase.as_deref().unwrap_or_default()))
                    .map_err(|e| anyhow!("{e}"))?,
                (None, None) => bail!("give a recovery phrase or a key file"),
            };
            let s = device.open(account.root().clone(), &dn, &pf).await?;
            let v = Host::account_result(&s);
            *host.session.lock().await = Some(s);
            Ok(v)
        }
        Command::LinkJoin { code } => {
            let device = host.device().await?;
            let (dn, pf) = host.names.lock().await.clone();
            let o = out.clone();
            let (_, s) = device.join(&code, &dn, &pf, move |c| o.event("link_confirm", json!({ "code": c }))).await?;
            let v = Host::account_result(&s);
            *host.session.lock().await = Some(s);
            Ok(v)
        }
        Command::LinkOffer => {
            let s = host.session().await?;
            let (code, mut attempts) = s.offer_link().await;
            out.event("link_code", json!({ "code": code.to_string() }));
            while let Some((who, confirm, decide)) = attempts.recv().await {
                let (tx, rx) = oneshot::channel();
                *host.decision.lock().await = Some(tx);
                out.event("link_request", json!({ "name": who.name, "platform": who.platform, "code": confirm }));
                let accept = rx.await.unwrap_or(false);
                let _ = decide.send(accept);
                if accept {
                    return Ok(json!({ "linked": who.name, "platform": who.platform }));
                }
            }
            Ok(json!({ "linked": null }))
        }
        Command::LinkDecide { accept } => {
            let tx = host.decision.lock().await.take().context("no link request is waiting")?;
            let _ = tx.send(accept);
            Ok(json!({}))
        }
        Command::LinkCancel => {
            host.session().await?.cancel_link().await;
            if let Some(tx) = host.decision.lock().await.take() {
                let _ = tx.send(false);
            }
            Ok(json!({}))
        }
        Command::Status => host.status().await,
        Command::Pool => Ok(json!({ "nodes": host.session().await?.pool().await })),
        Command::Claim { link, label } => {
            let s = host.session().await?;
            let n = s.claim_node(&link, &label).await?;
            Ok(json!({ "node_id": n.id }))
        }
        Command::ForgetNode { node_id } => {
            host.session().await?.forget_node(&node_id).await?;
            Ok(json!({}))
        }
        Command::Invite => Ok(json!({ "invite": host.session().await?.invite().await.encode() })),
        Command::Befriend { invite, quota_gb } => {
            let f = host.session().await?.add_friend(&invite, quota_gb * 1_000_000_000).await?;
            Ok(json!({ "account": f.account, "name": f.name }))
        }
        Command::RecoveryPhrase => Ok(json!({ "phrase": recovery_phrase(host.session().await?.account.root()) })),
        Command::KeyFile { passphrase } => {
            if passphrase.chars().count() < 8 {
                bail!("use a passphrase of at least 8 characters");
            }
            let s = host.session().await?;
            let kf = KeyFile::create(&s.account, &passphrase).map_err(|e| anyhow!("{e}"))?;
            Ok(json!({ "key_file": kf.to_json(), "account_id": s.account_id() }))
        }
        Command::List => Ok(json!({ "items": host.session().await?.list().await? })),
        Command::Upload(meta) => {
            let s = host.session().await?;
            let o = out.clone();
            let item = s.upload(meta, move |d, t| o.progress(d, t)).await?;
            Ok(json!({ "item": item }))
        }
        Command::Thumb { file_id } => Ok(json!({ "path": host.session().await?.thumb(&file_id).await? })),
        Command::Download { file_id, out_path } => {
            let s = host.session().await?;
            let o = out.clone();
            s.download(&file_id, &out_path, move |d, t| o.progress(d, t)).await?;
            Ok(json!({ "path": out_path }))
        }
        Command::Delete { file_id } => {
            host.session().await?.delete(&file_id).await?;
            Ok(json!({}))
        }
        Command::Repair => {
            let s = host.session().await?;
            let o = out.clone();
            let r = s.repair(move |d, t| o.progress(d as u64, t as u64)).await?;
            Ok(json!({ "report": r }))
        }
    }
}
