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
    /// After a link: the other device's relays, which the core now runs on. The app keeps
    /// them (with `relay_token`) for its next start. None after anything else.
    pub relays: Option<Vec<String>>,
    pub relay_token: Option<String>,
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

impl From<UploadRequest> for UploadMeta {
    fn from(r: UploadRequest) -> Self {
        UploadMeta {
            path: r.path.into(),
            name: r.name,
            mime: r.mime,
            created_ms: r.created_ms,
            modified_ms: r.modified_ms,
            width: r.width,
            height: r.height,
            thumb_path: r.thumb_path.map(Into::into),
        }
    }
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

/// What moving the library to the chosen protection did.
#[derive(uniffi::Record)]
pub struct RecodeReport {
    pub files: u64,
    pub recoded_files: u64,
    pub objects_recoded: u64,
    pub shards_deleted: u64,
    pub failures: Vec<String>,
}

/// What a sweep of unused pieces did.
#[derive(uniffi::Record)]
pub struct GcReport {
    pub nodes_swept: u64,
    pub nodes_skipped: u64,
    pub deleted: u64,
    pub bytes_freed: u64,
    pub kept_young: u64,
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
/// Runs a call on the core's runtime. A call that fails is logged here, on its way to the app,
/// with the line of the call (each exported method makes one), so it reaches the device log and
/// telemetry: before, an error went straight back to the app and the core kept no trace of it,
/// so a failure the app showed only on screen (a bulk delete's "the account head kept
/// changing") could not be found afterwards.
#[track_caller]
fn on_rt<T: Send + 'static>(
    f: impl Future<Output = anyhow::Result<T>> + Send + 'static,
) -> impl Future<Output = Res<T>> {
    let at = std::panic::Location::caller();
    run_logged(f, at, true)
}

/// [`on_rt`] for calls whose input is a secret (recovery words, a key file or its passphrase,
/// the account root, a link or claim code): a failure is logged without its message, which may
/// quote the input it could not parse. Telemetry never carries keys.
#[track_caller]
fn on_rt_secret<T: Send + 'static>(
    f: impl Future<Output = anyhow::Result<T>> + Send + 'static,
) -> impl Future<Output = Res<T>> {
    let at = std::panic::Location::caller();
    run_logged(f, at, false)
}

async fn run_logged<T: Send + 'static>(
    f: impl Future<Output = anyhow::Result<T>> + Send + 'static,
    at: &'static std::panic::Location<'static>,
    with_message: bool,
) -> Res<T> {
    let r = runtime()
        .spawn(f)
        .await
        .map_err(|e| anyhow!("the core stopped mid-call: {e}"))
        .and_then(|r| r);
    if let Err(e) = &r {
        let call = format!("{}:{}", at.file(), at.line());
        if with_message {
            tracing::warn!(call = %call, "call failed: {e:#}");
        } else {
            tracing::warn!(call = %call, "call failed (message withheld: the input is secret)");
        }
    }
    r.map_err(Into::into)
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
                .with(ursa_telemetry::layer())
                .try_init();
        }
        #[cfg(not(target_os = "android"))]
        {
            use tracing_subscriber::Layer;
            use tracing_subscriber::layer::SubscriberExt;
            use tracing_subscriber::util::SubscriberInitExt;
            let _ = tracing_subscriber::registry()
                .with(
                    tracing_subscriber::fmt::layer().with_filter(
                        tracing_subscriber::EnvFilter::try_from_default_env()
                            .unwrap_or_else(|_| "info,iroh=warn,noq=warn".into()),
                    ),
                )
                .with(ursa_telemetry::layer())
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
            relays: None,
            relay_token: None,
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
        on_rt_secret(async move {
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

    /// Starts sending the core's logs to an OTLP collector, as `ursa-android` / `core` with the
    /// app's telemetry id (the app's own JavaScript exports under the same id). Once per process;
    /// returns whether this call started it.
    pub fn configure_telemetry(
        &self,
        endpoint: String,
        instance_id: String,
        version: String,
    ) -> Res<bool> {
        Ok(ursa_telemetry::configure(ursa_telemetry::Config {
            endpoint,
            service: "ursa-android".into(),
            layer: "core".into(),
            version,
            instance_id,
        })?)
    }

    /// Sends the core's buffered logs now (the app is going to the background).
    pub fn flush_telemetry(&self) {
        ursa_telemetry::flush();
    }

    pub async fn status(&self) -> Res<Status> {
        let inner = self.inner.clone();
        on_rt(async move { inner.status(None).await }).await
    }

    /// Re-reads the account from its nodes (what other devices changed), then the status.
    pub async fn refresh(&self) -> Res<Status> {
        let inner = self.inner.clone();
        on_rt(async move {
            inner.session().await?.refresh().await?;
            inner.status(None).await
        })
        .await
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
        on_rt_secret(async move {
            let root = root_from_phrase(&phrase).map_err(|e| anyhow!("{e}"))?;
            open_root(&inner, Account::from_root(root)).await
        })
        .await
    }

    /// Recovers an account from a key file (its JSON) and the passphrase it was saved with.
    pub async fn recover_key_file(&self, key_file: String, passphrase: String) -> Res<AccountKeys> {
        let inner = self.inner.clone();
        on_rt_secret(async move {
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
        on_rt_secret(async move {
            let (dn, pf) = inner.names.lock().await.clone();
            let device = inner.device().await?;
            let linked = device
                .join(&code, &dn, &pf, move |c| listener.on_confirm(c))
                .await?;
            // Joining may have restarted the device on the other device's relays.
            *inner.device.lock().await = Some(linked.session.device.clone());
            let mut keys = inner.adopt(linked.session).await;
            if let Some(n) = linked.network {
                keys.relays = Some(n.relays);
                keys.relay_token = n.relay_token;
            }
            Ok(keys)
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
        on_rt_secret(async move {
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
        on_rt_secret(async move { Ok(recovery_phrase(inner.session().await?.account.root())) }).await
    }

    /// The account root sealed under a passphrase, as key-file JSON.
    pub async fn key_file(&self, passphrase: String) -> Res<String> {
        let inner = self.inner.clone();
        on_rt_secret(async move {
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

    /// Uploads a file and writes it into the library at once.
    pub async fn upload(
        &self,
        request: UploadRequest,
        listener: Arc<dyn ProgressListener>,
    ) -> Res<LibraryItem> {
        let inner = self.inner.clone();
        on_rt(async move {
            Ok(inner
                .session()
                .await?
                .upload(request.into(), throttled(listener))
                .await?
                .into())
        })
        .await
    }

    /// Uploads a file and leaves its library entry for `commit_uploads`, which writes any number
    /// in one index change. The file lists like any other meanwhile; it is only kept for good
    /// once a commit after it succeeds (a process that ends first loses it from the library).
    pub async fn upload_staged(
        &self,
        request: UploadRequest,
        listener: Arc<dyn ProgressListener>,
    ) -> Res<LibraryItem> {
        let inner = self.inner.clone();
        on_rt(async move {
            Ok(inner
                .session()
                .await?
                .upload_staged(request.into(), throttled(listener))
                .await?
                .into())
        })
        .await
    }

    /// Writes every staged upload into the library in one change. Returns how many it wrote.
    pub async fn commit_uploads(&self) -> Res<u32> {
        let inner = self.inner.clone();
        on_rt(async move { Ok(inner.session().await?.commit_uploads().await? as u32) }).await
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

    /// Deletes several files in one library change (one index upload, one head save).
    pub async fn delete_many(&self, file_ids: Vec<String>) -> Res<()> {
        let inner = self.inner.clone();
        on_rt(async move {
            let ids: Vec<&str> = file_ids.iter().map(String::as_str).collect();
            inner.session().await?.delete_many(&ids).await
        })
        .await
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

    /// Everything the Pool and Usage screens show, as JSON: the same report the Windows host
    /// sends (`ursa_client::storage::StorageReport`), so both apps read one shape. `k` and `n`
    /// add a shape being considered to the choices.
    pub async fn storage_json(&self, k: Option<u16>, n: Option<u16>) -> Res<String> {
        let inner = self.inner.clone();
        on_rt(async move {
            let extra = k.zip(n).map(|(k, n)| ursa_client::storage::Shape { k, n });
            let report = inner.session().await?.storage(extra).await?;
            Ok(serde_json::to_string(&report)?)
        })
        .await
    }

    /// Chooses the code for the account's files (`k` of `n` pieces rebuild one); neither:
    /// picked from the pool's size. [`UrsaCore::recode`] moves the files already stored.
    pub async fn set_protection(&self, k: Option<u16>, n: Option<u16>) -> Res<()> {
        let inner = self.inner.clone();
        on_rt(async move {
            let shape = match (k, n) {
                (Some(k), Some(n)) => Some(ursa_client::storage::Shape { k, n }),
                (None, None) => None,
                _ => bail!("give both k and n, or neither"),
            };
            inner.session().await?.set_protection(shape).await
        })
        .await
    }

    /// Moves every file to the chosen protection and deletes the old pieces. Safe to stop and
    /// run again: a later run finishes it and clears what this one left.
    pub async fn recode(&self, listener: Arc<dyn ProgressListener>) -> Res<RecodeReport> {
        let inner = self.inner.clone();
        on_rt(async move {
            let progress = throttled(listener);
            let r = inner.session().await?.recode(progress).await?;
            Ok(RecodeReport {
                files: r.files as u64,
                recoded_files: r.recoded_files as u64,
                objects_recoded: r.objects_recoded as u64,
                shards_deleted: r.shards_deleted as u64,
                failures: r.failures,
            })
        })
        .await
    }

    /// Gives every friend an equal share of `bytes` on each node this account owns.
    pub async fn set_give(&self, bytes: u64) -> Res<()> {
        let inner = self.inner.clone();
        on_rt(async move { inner.session().await?.set_give(bytes).await }).await
    }

    /// Deletes pieces nothing references that are older than `grace_hours` (default 72).
    pub async fn collect_garbage(&self, grace_hours: Option<f64>) -> Res<GcReport> {
        let inner = self.inner.clone();
        on_rt(async move {
            let grace = grace_hours.map_or(ursa_client::gc::DEFAULT_GRACE, |h| Duration::from_secs_f64(h.max(0.0) * 3600.0));
            let r = inner.session().await?.collect_garbage(grace).await?;
            Ok(GcReport {
                nodes_swept: r.nodes_swept as u64,
                nodes_skipped: r.nodes_skipped.len() as u64,
                deleted: r.deleted,
                bytes_freed: r.bytes_freed,
                kept_young: r.kept_young as u64,
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
