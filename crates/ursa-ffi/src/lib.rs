//! The Ursa device for mobile apps: [`ursa_client`] behind a UniFFI interface.
//!
//! One [`UrsaCore`] per process. The app owns the account root and keeps it under the OS's
//! protection (the Android Keystore); it hands it over in [`UrsaCore::init`], or receives it from
//! `create`, `recover_*` or `link_join`, and the core keeps it in memory only.
//!
//! Every call runs on the core's own multi-threaded tokio runtime, not on the foreign caller's
//! thread or UniFFI's executor: chunking, encryption and erasure coding are CPU work that must
//! not queue behind the network.

use std::future::Future;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use anyhow::{Context, anyhow, bail};
use tokio::sync::{Mutex, oneshot};
use ursa_client::{Device, NetConfig, Session, UploadMeta};
use ursa_core::account::{Account, KeyFile, recovery_phrase, root_from_phrase};
use ursa_core::crypto::AccountKey;

uniffi::setup_scaffolding!();

#[cfg(target_os = "android")]
mod logcat;

// ---------------------------------------------------------------------------------------------
// Types the app sees

#[derive(Debug, thiserror::Error, uniffi::Error)]
#[uniffi(flat_error)]
pub enum UrsaError {
    #[error("{message}")]
    Failed { message: String },
}

impl From<anyhow::Error> for UrsaError {
    fn from(e: anyhow::Error) -> Self {
        Self::Failed {
            message: format!("{e:#}"),
        }
    }
}

type Res<T> = Result<T, UrsaError>;

/// The account root (hex) to keep in secure storage, and the account it opens.
#[derive(uniffi::Record)]
pub struct AccountKeys {
    pub account_id: String,
    pub account_key: String,
}

#[derive(uniffi::Record)]
pub struct DeviceInfo {
    pub device_id: String,
    pub name: String,
    pub platform: String,
    pub added_ms: i64,
    pub this_device: bool,
}

#[derive(uniffi::Record)]
pub struct NodeEntry {
    pub id: String,
    pub label: String,
}

#[derive(uniffi::Record)]
pub struct FriendInfo {
    pub account_id: String,
    pub name: String,
    pub nodes: u32,
}

#[derive(uniffi::Record)]
pub struct Status {
    pub device_id: String,
    pub has_account: bool,
    pub account_id: Option<String>,
    pub name: Option<String>,
    pub devices: Vec<DeviceInfo>,
    pub home_nodes: Vec<NodeEntry>,
    pub friends: Vec<FriendInfo>,
    /// Set when `init` held a root but could not open the account (its nodes may be offline).
    pub open_error: Option<String>,
}

#[derive(uniffi::Record)]
pub struct LibraryItem {
    pub file_id: String,
    pub name: String,
    pub size: u64,
    pub mime: String,
    pub created_ms: i64,
    pub modified_ms: i64,
    pub added_ms: i64,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub has_thumb: bool,
}

impl From<ursa_client::Item> for LibraryItem {
    fn from(i: ursa_client::Item) -> Self {
        Self {
            file_id: i.file_id,
            name: i.name,
            size: i.size,
            mime: i.mime,
            created_ms: i.created_ms,
            modified_ms: i.modified_ms,
            added_ms: i.added_ms,
            width: i.width,
            height: i.height,
            has_thumb: i.has_thumb,
        }
    }
}

#[derive(uniffi::Record)]
pub struct UploadRequest {
    pub path: String,
    pub name: Option<String>,
    pub mime: Option<String>,
    pub created_ms: Option<i64>,
    pub modified_ms: Option<i64>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub thumb_path: Option<String>,
}

#[derive(uniffi::Record)]
pub struct PoolNode {
    pub id: String,
    pub label: String,
    /// `home` for the account's own nodes, else the friend's name.
    pub owner: String,
    pub reachable: bool,
    /// `direct` or `relay`, when reachable.
    pub path: Option<String>,
    /// `owner`, `granted` or `none`: this account's standing with the node.
    pub role: Option<String>,
    pub used_bytes: Option<u64>,
    pub quota_bytes: Option<u64>,
    pub version: Option<String>,
    pub backend: Option<String>,
    pub error: Option<String>,
}

#[derive(uniffi::Record)]
pub struct RepairReport {
    pub files: u64,
    pub objects_checked: u64,
    pub objects_rebuilt: u64,
    pub unrecoverable: Vec<String>,
}

/// Progress of a long call. Called at most every 100 ms, and always for the last step.
#[uniffi::export(with_foreign)]
pub trait ProgressListener: Send + Sync {
    fn on_progress(&self, done: u64, total: u64);
}

/// What joining an account shows: the confirmation code, which must match the other screen.
#[uniffi::export(with_foreign)]
pub trait JoinListener: Send + Sync {
    fn on_confirm(&self, code: String);
}

/// What offering a link code shows: the code to hand over, then each device asking to join.
/// Answer a request with [`UrsaCore::link_decide`].
#[uniffi::export(with_foreign)]
pub trait OfferListener: Send + Sync {
    fn on_code(&self, code: String);
    fn on_request(&self, name: String, platform: String, confirm: String);
}

// ---------------------------------------------------------------------------------------------
// The core

fn runtime() -> &'static tokio::runtime::Runtime {
    static RT: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(4)
            .thread_name("ursa")
            .enable_all()
            .build()
            .expect("tokio runtime")
    })
}

/// Runs `f` on the core's runtime and waits for it from whatever executor called.
async fn on_rt<T: Send + 'static>(
    f: impl Future<Output = anyhow::Result<T>> + Send + 'static,
) -> Res<T> {
    runtime()
        .spawn(f)
        .await
        .map_err(|e| anyhow!("the core stopped mid-call: {e}"))?
        .map_err(Into::into)
}

fn init_logging() {
    static ONCE: OnceLock<()> = OnceLock::new();
    ONCE.get_or_init(|| {
        #[cfg(target_os = "android")]
        {
            use tracing_subscriber::layer::SubscriberExt;
            use tracing_subscriber::util::SubscriberInitExt;
            let _ = tracing_subscriber::registry()
                .with(logcat::layer())
                .try_init();
        }
        #[cfg(not(target_os = "android"))]
        {
            let _ = tracing_subscriber::fmt()
                .with_env_filter(
                    tracing_subscriber::EnvFilter::try_from_default_env()
                        .unwrap_or_else(|_| "info,iroh=warn,noq=warn".into()),
                )
                .try_init();
        }
    });
}

/// Calls a progress listener at most every 100 ms, and always for the last step.
fn throttled(listener: Arc<dyn ProgressListener>) -> impl Fn(u64, u64) + Send + Sync {
    let last = std::sync::Mutex::new(None::<Instant>);
    move |done, total| {
        let mut last = last.lock().expect("progress lock");
        let due = last.is_none_or(|t| t.elapsed() >= Duration::from_millis(100));
        if due || done >= total {
            *last = Some(Instant::now());
            listener.on_progress(done, total);
        }
    }
}

#[derive(Default)]
struct Inner {
    device: Mutex<Option<Arc<Device>>>,
    session: Mutex<Option<Arc<Session>>>,
    names: Mutex<(String, String)>,
    /// The answer channel for a link request now on screen.
    decision: Mutex<Option<oneshot::Sender<bool>>>,
}

impl Inner {
    async fn device(&self) -> anyhow::Result<Arc<Device>> {
        self.device
            .lock()
            .await
            .clone()
            .context("the core is not started")
    }

    async fn session(&self) -> anyhow::Result<Arc<Session>> {
        self.session
            .lock()
            .await
            .clone()
            .context("this device has no account yet")
    }

    async fn adopt(&self, s: Arc<Session>) -> AccountKeys {
        let keys = AccountKeys {
            account_id: s.account_id().to_string(),
            account_key: hex_root(s.account.root()),
        };
        *self.session.lock().await = Some(s);
        keys
    }

    async fn status(&self, open_error: Option<String>) -> anyhow::Result<Status> {
        let device = self.device().await?;
        let Some(s) = self.session.lock().await.clone() else {
            return Ok(Status {
                device_id: device.id(),
                has_account: false,
                account_id: None,
                name: None,
                devices: vec![],
                home_nodes: vec![],
                friends: vec![],
                open_error,
            });
        };
        let st = s.state().await;
        let me = device.id();
        Ok(Status {
            device_id: me.clone(),
            has_account: true,
            account_id: Some(s.account_id().to_string()),
            name: Some(st.name.clone()),
            devices: st
                .devices
                .iter()
                .map(|d| DeviceInfo {
                    device_id: d.device.clone(),
                    name: d.name.clone(),
                    platform: d.platform.clone(),
                    added_ms: d.added_ms,
                    this_device: d.device == me,
                })
                .collect(),
            home_nodes: st
                .home_nodes
                .iter()
                .map(|n| NodeEntry {
                    id: n.id.clone(),
                    label: n.label.clone(),
                })
                .collect(),
            friends: st
                .friends
                .iter()
                .map(|f| FriendInfo {
                    account_id: f.account.to_string(),
                    name: f.name.clone(),
                    nodes: f.nodes.len() as u32,
                })
                .collect(),
            open_error,
        })
    }
}

fn hex_root(root: &AccountKey) -> String {
    root.as_bytes().iter().map(|b| format!("{b:02x}")).collect()
}

#[derive(uniffi::Object)]
pub struct UrsaCore {
    inner: Arc<Inner>,
}

#[uniffi::export]
impl UrsaCore {
    #[uniffi::constructor]
    pub fn new() -> Arc<Self> {
        init_logging();
        Arc::new(Self {
            inner: Arc::new(Inner::default()),
        })
    }

    /// Starts this device's endpoint (its identity lives in `data_dir`) and, given the stored
    /// root, opens the account. A root that cannot be opened is reported in `open_error`, not
    /// thrown: the account's nodes may simply be offline, and the app must keep the root.
    pub async fn init(
        &self,
        data_dir: String,
        account_key: Option<String>,
        relays: Vec<String>,
        relay_token: Option<String>,
        device_name: String,
        platform: String,
    ) -> Res<Status> {
        let inner = self.inner.clone();
        on_rt(async move {
            // Not `match inner.device.lock().await.clone()`: a scrutinee's temporaries live to
            // the end of the match, so the guard would still be held when `None` stores the
            // device, and the second lock would wait on the first forever.
            let started = inner.device.lock().await.clone();
            let device = match started {
                Some(d) => d,
                None => {
                    let d = Device::start(
                        &PathBuf::from(&data_dir),
                        &NetConfig {
                            relays,
                            relay_token,
                            pkarr: vec![],
                        },
                    )
                    .await?;
                    *inner.device.lock().await = Some(d.clone());
                    d
                }
            };
            *inner.names.lock().await = (device_name.clone(), platform.clone());
            let mut open_error = None;
            let has_session = inner.session.lock().await.is_some();
            if let Some(hex) = account_key
                && !has_session
            {
                let root = AccountKey::from_hex(&hex).map_err(|e| anyhow!("{e}"))?;
                match device.open(root, &device_name, &platform).await {
                    Ok(s) => *inner.session.lock().await = Some(s),
                    Err(e) => open_error = Some(format!("{e:#}")),
                }
            }
            inner.status(open_error).await
        })
        .await
    }

    pub async fn status(&self) -> Res<Status> {
        let inner = self.inner.clone();
        on_rt(async move { inner.status(None).await }).await
    }

    /// A new account, with this device as its first.
    pub async fn create_account(&self, name: String) -> Res<AccountKeys> {
        let inner = self.inner.clone();
        on_rt(async move {
            let (dn, pf) = inner.names.lock().await.clone();
            let s = inner
                .device()
                .await?
                .create_account(name.trim(), &dn, &pf)
                .await?;
            Ok(inner.adopt(s).await)
        })
        .await
    }

    /// Recovers an account from its 24 words. The account is found through its locator record.
    pub async fn recover_phrase(&self, phrase: String) -> Res<AccountKeys> {
        let inner = self.inner.clone();
        on_rt(async move {
            let root = root_from_phrase(&phrase).map_err(|e| anyhow!("{e}"))?;
            open_root(&inner, Account::from_root(root)).await
        })
        .await
    }

    /// Recovers an account from a key file (its JSON) and the passphrase it was saved with.
    pub async fn recover_key_file(&self, key_file: String, passphrase: String) -> Res<AccountKeys> {
        let inner = self.inner.clone();
        on_rt(async move {
            let account = KeyFile::from_json(&key_file)
                .and_then(|kf| kf.open(&passphrase))
                .map_err(|e| anyhow!("{e}"))?;
            open_root(&inner, account).await
        })
        .await
    }

    /// Joins the account behind a link code another device shows.
    pub async fn link_join(
        &self,
        code: String,
        listener: Arc<dyn JoinListener>,
    ) -> Res<AccountKeys> {
        let inner = self.inner.clone();
        on_rt(async move {
            let (dn, pf) = inner.names.lock().await.clone();
            let device = inner.device().await?;
            let (_, s) = device
                .join(&code, &dn, &pf, move |c| listener.on_confirm(c))
                .await?;
            Ok(inner.adopt(s).await)
        })
        .await
    }

    /// Offers a link code until a device joins (its name), or the offer is cancelled (None).
    pub async fn link_offer(&self, listener: Arc<dyn OfferListener>) -> Res<Option<String>> {
        let inner = self.inner.clone();
        on_rt(async move {
            let s = inner.session().await?;
            let (code, mut attempts) = s.offer_link().await;
            listener.on_code(code.to_string());
            while let Some((who, confirm, decide)) = attempts.recv().await {
                let (tx, rx) = oneshot::channel();
                *inner.decision.lock().await = Some(tx);
                listener.on_request(who.name.clone(), who.platform.clone(), confirm);
                let accept = rx.await.unwrap_or(false);
                let _ = decide.send(accept);
                if accept {
                    return Ok(Some(who.name));
                }
            }
            Ok(None)
        })
        .await
    }

    pub async fn link_decide(&self, accept: bool) -> Res<()> {
        let inner = self.inner.clone();
        on_rt(async move {
            let tx = inner
                .decision
                .lock()
                .await
                .take()
                .context("no device is waiting to join")?;
            let _ = tx.send(accept);
            Ok(())
        })
        .await
    }

    pub async fn link_cancel(&self) -> Res<()> {
        let inner = self.inner.clone();
        on_rt(async move {
            if let Some(s) = inner.session.lock().await.clone() {
                s.cancel_link().await;
            }
            if let Some(tx) = inner.decision.lock().await.take() {
                let _ = tx.send(false);
            }
            Ok(())
        })
        .await
    }

    /// Every node in the pool, checked now.
    pub async fn pool(&self) -> Res<Vec<PoolNode>> {
        let inner = self.inner.clone();
        on_rt(async move {
            let nodes = inner.session().await?.pool().await;
            Ok(nodes
                .into_iter()
                .map(|n| PoolNode {
                    id: n.id,
                    label: n.label,
                    owner: n.owner,
                    reachable: n.reachable,
                    path: n.path,
                    role: n.info.as_ref().map(|i| {
                        match i.role {
                            ursa_proto::Role::Owner => "owner",
                            ursa_proto::Role::Granted => "granted",
                            ursa_proto::Role::None => "none",
                        }
                        .to_string()
                    }),
                    used_bytes: n.info.as_ref().map(|i| i.used_bytes),
                    quota_bytes: n.info.as_ref().and_then(|i| i.quota_bytes),
                    version: n.info.as_ref().map(|i| i.version.clone()),
                    backend: n.info.as_ref().map(|i| i.backend.clone()),
                    error: n.error,
                })
                .collect())
        })
        .await
    }

    /// Claims a node with its `ursa-claim:` link; returns its id.
    pub async fn claim_node(&self, link: String, label: String) -> Res<String> {
        let inner = self.inner.clone();
        on_rt(async move {
            Ok(inner
                .session()
                .await?
                .claim_node(link.trim(), label.trim())
                .await?
                .id)
        })
        .await
    }

    pub async fn forget_node(&self, node_id: String) -> Res<()> {
        let inner = self.inner.clone();
        on_rt(async move { inner.session().await?.forget_node(&node_id).await }).await
    }

    /// This account's `ursa-invite:` code, for a friend to add.
    pub async fn invite(&self) -> Res<String> {
        let inner = self.inner.clone();
        on_rt(async move { Ok(inner.session().await?.invite().await.encode()) }).await
    }

    /// Adds a friend from their invite and grants them `quota_gb` on this account's nodes.
    pub async fn befriend(&self, invite: String, quota_gb: u64) -> Res<FriendInfo> {
        let inner = self.inner.clone();
        on_rt(async move {
            let f = inner
                .session()
                .await?
                .add_friend(invite.trim(), quota_gb * 1_000_000_000)
                .await?;
            Ok(FriendInfo {
                account_id: f.account.to_string(),
                name: f.name,
                nodes: f.nodes.len() as u32,
            })
        })
        .await
    }

    pub async fn recovery_phrase(&self) -> Res<String> {
        let inner = self.inner.clone();
        on_rt(async move { Ok(recovery_phrase(inner.session().await?.account.root())) }).await
    }

    /// The account root sealed under a passphrase, as key-file JSON.
    pub async fn key_file(&self, passphrase: String) -> Res<String> {
        let inner = self.inner.clone();
        on_rt(async move {
            if passphrase.chars().count() < 8 {
                bail!("use a passphrase of at least 8 characters");
            }
            let s = inner.session().await?;
            Ok(KeyFile::create(&s.account, &passphrase)
                .map_err(|e| anyhow!("{e}"))?
                .to_json())
        })
        .await
    }

    pub async fn list(&self) -> Res<Vec<LibraryItem>> {
        let inner = self.inner.clone();
        on_rt(async move {
            Ok(inner
                .session()
                .await?
                .list()
                .await?
                .into_iter()
                .map(Into::into)
                .collect())
        })
        .await
    }

    pub async fn upload(
        &self,
        request: UploadRequest,
        listener: Arc<dyn ProgressListener>,
    ) -> Res<LibraryItem> {
        let inner = self.inner.clone();
        on_rt(async move {
            let meta = UploadMeta {
                path: request.path.into(),
                name: request.name,
                mime: request.mime,
                created_ms: request.created_ms,
                modified_ms: request.modified_ms,
                width: request.width,
                height: request.height,
                thumb_path: request.thumb_path.map(Into::into),
            };
            Ok(inner
                .session()
                .await?
                .upload(meta, throttled(listener))
                .await?
                .into())
        })
        .await
    }

    /// The decrypted thumbnail's path, cached on this device.
    pub async fn thumb(&self, file_id: String) -> Res<String> {
        let inner = self.inner.clone();
        on_rt(async move {
            Ok(inner
                .session()
                .await?
                .thumb(&file_id)
                .await?
                .to_string_lossy()
                .into_owned())
        })
        .await
    }

    pub async fn download(
        &self,
        file_id: String,
        out_path: String,
        listener: Arc<dyn ProgressListener>,
    ) -> Res<()> {
        let inner = self.inner.clone();
        on_rt(async move {
            inner
                .session()
                .await?
                .download(&file_id, &PathBuf::from(out_path), throttled(listener))
                .await
        })
        .await
    }

    pub async fn delete(&self, file_id: String) -> Res<()> {
        let inner = self.inner.clone();
        on_rt(async move { inner.session().await?.delete(&file_id).await }).await
    }

    pub async fn repair(&self, listener: Arc<dyn ProgressListener>) -> Res<RepairReport> {
        let inner = self.inner.clone();
        on_rt(async move {
            let progress = throttled(listener);
            let r = inner
                .session()
                .await?
                .repair(move |d, t| progress(d as u64, t as u64))
                .await?;
            Ok(RepairReport {
                files: r.files as u64,
                objects_checked: r.objects_checked as u64,
                objects_rebuilt: r.objects_rebuilt as u64,
                unrecoverable: r.unrecoverable,
            })
        })
        .await
    }

    /// Stops the endpoint (to start again with other relays, through `init`). This device's
    /// identity stays in its data directory.
    pub async fn shutdown(&self) -> Res<()> {
        let inner = self.inner.clone();
        on_rt(async move {
            inner.session.lock().await.take();
            if let Some(d) = inner.device.lock().await.take() {
                d.shutdown().await;
            }
            Ok(())
        })
        .await
    }

    /// Forgets the account on this device (the app deletes the stored root). The endpoint and
    /// this device's identity stay.
    pub async fn sign_out(&self) -> Res<()> {
        let inner = self.inner.clone();
        on_rt(async move {
            inner.session.lock().await.take();
            Ok(())
        })
        .await
    }
}

async fn open_root(inner: &Inner, account: Account) -> anyhow::Result<AccountKeys> {
    let (dn, pf) = inner.names.lock().await.clone();
    let s = inner
        .device()
        .await?
        .open(account.root().clone(), &dn, &pf)
        .await?;
    Ok(inner.adopt(s).await)
}
