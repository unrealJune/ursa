//! `ursa`: an Ursa device on the command line. Each `--profile` directory is one device (its
//! iroh key, and, for this tool, the account root in `account.root`), so a script can play
//! several people and several devices on one machine.
//!
//! The account root sits in the profile in the clear: this is a test and admin tool. The apps
//! keep it under the OS's protection instead.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, anyhow, bail};
use clap::{Parser, Subcommand};
use ursa_client::{Device, NetConfig, Session, UploadMeta};
use ursa_core::account::{Account, KeyFile, recovery_phrase, root_from_phrase};
use ursa_core::crypto::AccountKey;

#[derive(Parser)]
#[command(name = "ursa", version, about = "Ursa on the command line")]
struct Cli {
    /// This device's directory.
    #[arg(long, env = "URSA_PROFILE", global = true, default_value = "./ursa-profile")]
    profile: PathBuf,
    /// iroh relays (comma-separated); empty: n0's public ones.
    #[arg(long, env = "URSA_RELAY_URLS", value_delimiter = ',', global = true)]
    relay: Vec<String>,
    #[arg(long, env = "URSA_RELAY_TOKEN", global = true, hide_env_values = true)]
    relay_token: Option<String>,
    /// pkarr relays for account lookup (comma-separated); empty: iroh's.
    #[arg(long, env = "URSA_PKARR_RELAYS", value_delimiter = ',', global = true)]
    pkarr: Vec<String>,
    /// This device's name, as other devices list it.
    #[arg(long, global = true, default_value = "ursa CLI")]
    device_name: String,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Create an account on this device. Prints its id; --key-file also writes a recovery file.
    Create {
        #[arg(long)]
        name: String,
        #[arg(long, requires = "passphrase")]
        key_file: Option<PathBuf>,
        #[arg(long, env = "URSA_PASSPHRASE", hide_env_values = true)]
        passphrase: Option<String>,
        /// Also print the 24-word recovery phrase.
        #[arg(long)]
        show_phrase: bool,
    },
    /// Recover an account onto this device from a key file or a recovery phrase.
    Recover {
        #[arg(long, requires = "passphrase", conflicts_with = "phrase")]
        key_file: Option<PathBuf>,
        #[arg(long, env = "URSA_PASSPHRASE", hide_env_values = true)]
        passphrase: Option<String>,
        #[arg(long)]
        phrase: Option<String>,
    },
    /// The account, its devices, nodes and friends, and the pool as reachable now.
    Status,
    /// Claim a node with the `ursa-claim:` link it printed, as one of this account's home nodes.
    Claim {
        link: String,
        #[arg(long, default_value = "")]
        label: String,
    },
    /// Print this account's invite (`ursa-invite:`), for friends to add.
    Invite,
    /// Add a friend from their invite, and grant them space on this account's nodes.
    Befriend {
        invite: String,
        #[arg(long, default_value_t = 10)]
        quota_gb: u64,
    },
    /// Upload files; prints each one's file id.
    Upload { files: Vec<PathBuf> },
    /// List the library: file id, size, name.
    Ls,
    /// Download a file.
    Get { file_id: String, out: PathBuf },
    /// Remove a file from the library.
    Rm { file_id: String },
    /// Rebuild missing shards onto live nodes.
    Repair,
    /// Offer a link code for a new device and wait for it. --yes confirms without asking.
    LinkOffer {
        #[arg(long)]
        yes: bool,
        /// Write the code to this file as well (for scripts).
        #[arg(long)]
        code_file: Option<PathBuf>,
    },
    /// Join an account with a link code from one of its devices.
    LinkJoin { code: String },
    /// Write a recovery key file for this account.
    ExportKey {
        path: PathBuf,
        #[arg(long, env = "URSA_PASSPHRASE", hide_env_values = true)]
        passphrase: String,
    },
    /// Republish the account's locator record.
    Publish,
    /// Choose the code for this account's files: any K of N pieces rebuild a file. --auto
    /// goes back to picking it from the pool's size. Run `recode` to move existing files.
    Protection {
        #[arg(long, required_unless_present = "auto")]
        k: Option<u16>,
        #[arg(long, required_unless_present = "auto")]
        n: Option<u16>,
        #[arg(long, conflicts_with_all = ["k", "n"])]
        auto: bool,
    },
    /// Move every file to the chosen protection, deleting the old pieces.
    Recode,
    /// What the account stores, what each protection choice would let it store, the pool.
    Storage,
    /// Give friends this much space in total on your nodes, split equally.
    Give { gb: f64 },
    /// Delete pieces nothing references (left by stopped uploads and switches, deleted files,
    /// old indexes) that are older than the grace period.
    Gc {
        #[arg(long, default_value_t = 72.0)]
        grace_hours: f64,
    },
}

fn root_path(profile: &Path) -> PathBuf {
    profile.join("account.root")
}

fn save_root(profile: &Path, root: &AccountKey) -> Result<()> {
    std::fs::create_dir_all(profile)?;
    std::fs::write(root_path(profile), ursa_core::account::recovery_phrase(root))?;
    Ok(())
}

fn load_root(profile: &Path) -> Result<AccountKey> {
    let phrase = std::fs::read_to_string(root_path(profile)).context("this profile has no account: create, recover or link-join first")?;
    root_from_phrase(&phrase).map_err(|e| anyhow!("{e}"))
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn".into()))
        .init();
    let cli = Cli::parse();
    std::fs::create_dir_all(&cli.profile)?;
    let net = NetConfig { relays: cli.relay.clone(), relay_token: cli.relay_token.clone(), pkarr: cli.pkarr.clone() };
    let device = Device::start(&cli.profile, &net).await?;
    let platform = std::env::consts::OS;
    let result = run(&cli, &device, platform).await;
    device.shutdown().await;
    result
}

async fn session(cli: &Cli, device: &Arc<Device>, platform: &str) -> Result<Arc<Session>> {
    device.open(load_root(&cli.profile)?, &cli.device_name, platform).await
}

async fn run(cli: &Cli, device: &Arc<Device>, platform: &str) -> Result<()> {
    match &cli.cmd {
        Cmd::Create { name, key_file, passphrase, show_phrase } => {
            if root_path(&cli.profile).exists() {
                bail!("this profile already has an account");
            }
            let s = device.create_account(name, &cli.device_name, platform).await?;
            save_root(&cli.profile, s.account.root())?;
            if let (Some(path), Some(pass)) = (key_file, passphrase) {
                std::fs::write(path, KeyFile::create(&s.account, pass).map_err(|e| anyhow!("{e}"))?.to_json())?;
                eprintln!("wrote recovery key file {}", path.display());
            }
            if *show_phrase {
                eprintln!("recovery phrase: {}", recovery_phrase(s.account.root()));
            }
            println!("{}", s.account_id());
            eprintln!("account created; claim a node you own next (ursa claim <link>): the library needs one");
        }
        Cmd::Recover { key_file, passphrase, phrase } => {
            let account = match (key_file, phrase) {
                (Some(path), _) => KeyFile::from_json(&std::fs::read_to_string(path)?)
                    .and_then(|k| k.open(passphrase.as_deref().unwrap_or_default()))
                    .map_err(|e| anyhow!("{e}"))?,
                (None, Some(p)) => Account::from_root(root_from_phrase(p).map_err(|e| anyhow!("{e}"))?),
                (None, None) => bail!("give --key-file and --passphrase, or --phrase"),
            };
            let s = device.open(account.root().clone(), &cli.device_name, platform).await?;
            save_root(&cli.profile, s.account.root())?;
            let st = s.state().await;
            println!("{}", s.account_id());
            eprintln!(
                "recovered {:?}: {} device(s), {} home node(s), {} friend(s), head seq {:?}",
                st.name,
                st.devices.len(),
                st.home_nodes.len(),
                st.friends.len(),
                s.seq().await
            );
        }
        Cmd::Status => {
            let s = session(cli, device, platform).await?;
            let st = s.state().await;
            let out = serde_json::json!({
                "account": s.account_id(),
                "name": st.name,
                "device": device.id(),
                "seq": s.seq().await,
                "devices": st.devices,
                "home_nodes": st.home_nodes.iter().map(|n| &n.id).collect::<Vec<_>>(),
                "friends": st.friends.iter().map(|f| serde_json::json!({"account": f.account, "name": f.name})).collect::<Vec<_>>(),
                "pool": s.pool().await,
            });
            println!("{}", serde_json::to_string_pretty(&out)?);
        }
        Cmd::Claim { link, label } => {
            let s = session(cli, device, platform).await?;
            let n = s.claim_node(link, label).await?;
            println!("{}", n.id);
        }
        Cmd::Invite => {
            let s = session(cli, device, platform).await?;
            println!("{}", s.invite().await.encode());
        }
        Cmd::Befriend { invite, quota_gb } => {
            let s = session(cli, device, platform).await?;
            let f = s.add_friend(invite, quota_gb * 1_000_000_000).await?;
            println!("{}", f.account);
        }
        Cmd::Upload { files } => {
            let s = session(cli, device, platform).await?;
            for f in files {
                let item = s.upload(UploadMeta { path: f.clone(), ..Default::default() }, |_, _| {}).await?;
                println!("{}\t{}", item.file_id, item.name);
            }
        }
        Cmd::Ls => {
            let s = session(cli, device, platform).await?;
            for i in s.list().await? {
                println!("{}\t{}\t{}", i.file_id, i.size, i.name);
            }
        }
        Cmd::Get { file_id, out } => {
            let s = session(cli, device, platform).await?;
            s.download(file_id, out, |_, _| {}).await?;
            eprintln!("wrote {}", out.display());
        }
        Cmd::Rm { file_id } => {
            let s = session(cli, device, platform).await?;
            s.delete(file_id).await?;
        }
        Cmd::Repair => {
            let s = session(cli, device, platform).await?;
            let r = s.repair(|_, _| {}).await?;
            println!("{}", serde_json::to_string_pretty(&r)?);
            if !r.unrecoverable.is_empty() {
                bail!("{} object(s) could not be recovered", r.unrecoverable.len());
            }
        }
        Cmd::LinkOffer { yes, code_file } => {
            let s = session(cli, device, platform).await?;
            let (code, mut attempts) = s.offer_link().await;
            if let Some(p) = code_file {
                std::fs::write(p, code.to_string())?;
            }
            println!("{code}");
            eprintln!("waiting for a device to join…");
            while let Some((who, confirm, decide)) = attempts.recv().await {
                eprintln!("{} ({}) wants to join. Confirmation code: {confirm}", who.name, who.platform);
                let ok = if *yes {
                    true
                } else {
                    eprint!("Does the other device show {confirm}? [y/N] ");
                    std::io::stderr().flush().ok();
                    let mut line = String::new();
                    std::io::stdin().read_line(&mut line)?;
                    line.trim().eq_ignore_ascii_case("y")
                };
                let _ = decide.send(ok);
                if ok {
                    // Give the reply time to reach the joiner, then record the device.
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                    eprintln!("linked {}", who.name);
                    break;
                }
            }
        }
        Cmd::LinkJoin { code } => {
            if root_path(&cli.profile).exists() {
                bail!("this profile already has an account");
            }
            let (root, s) = device
                .join(code, &cli.device_name, platform, |c| eprintln!("Confirmation code: {c} (check it matches the other device)"))
                .await?;
            save_root(&cli.profile, &root)?;
            println!("{}", s.account_id());
        }
        Cmd::ExportKey { path, passphrase } => {
            let account = Account::from_root(load_root(&cli.profile)?);
            std::fs::write(path, KeyFile::create(&account, passphrase).map_err(|e| anyhow!("{e}"))?.to_json())?;
            println!("{}", path.display());
        }
        Cmd::Protection { k, n, auto } => {
            let s = session(cli, device, platform).await?;
            let shape = if *auto { None } else { Some(ursa_client::storage::Shape { k: k.unwrap(), n: n.unwrap() }) };
            s.set_protection(shape).await?;
            println!("{}", serde_json::to_string(&s.storage(None).await?.protection)?);
        }
        Cmd::Recode => {
            let s = session(cli, device, platform).await?;
            let r = s.recode(|_, _| {}).await?;
            println!("{}", serde_json::to_string_pretty(&r)?);
            if !r.failures.is_empty() {
                bail!("{} file(s) could not be re-coded", r.failures.len());
            }
        }
        Cmd::Storage => {
            let s = session(cli, device, platform).await?;
            println!("{}", serde_json::to_string_pretty(&s.storage(None).await?)?);
        }
        Cmd::Gc { grace_hours } => {
            let s = session(cli, device, platform).await?;
            let r = s.collect_garbage(std::time::Duration::from_secs_f64(grace_hours * 3600.0)).await?;
            println!("{}", serde_json::to_string_pretty(&r)?);
        }
        Cmd::Give { gb } => {
            let s = session(cli, device, platform).await?;
            s.set_give((gb * 1e9) as u64).await?;
        }
        Cmd::Publish => {
            let s = session(cli, device, platform).await?;
            println!("published to {} relay(s)", s.publish_locator().await?);
        }
    }
    Ok(())
}
