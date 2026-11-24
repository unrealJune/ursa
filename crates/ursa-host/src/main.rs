//! `ursa-host`: what a desktop app runs to reach its Ursa node.
//!
//! The app owns the account key (it keeps it under the OS's own protection) and hands it over
//! in `init`; the host keeps it in memory only. Everything else the app asks for goes through
//! here: connecting to (and claiming) the home node, listing the library, uploading (chunk,
//! encrypt, skip what the node already has, store, index) and downloading (fetch in parallel,
//! verify, decrypt, write in order).
//!
//! Wire: one JSON object per line. Requests carry an `id` and a `cmd`; every request gets one
//! response with the same `id` (`ok` plus fields, or `ok: false` and `error`); long operations
//! also emit `{"event":"progress","id":…,"done":…,"total":…}` lines first. Logs go to stderr.

mod library;

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{OnceCell, mpsc};

use crate::library::{Library, Out, UploadMeta};

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
        account_key: String,
        #[serde(default)]
        relays: Vec<String>,
        #[serde(default)]
        relay_token: Option<String>,
    },
    Status,
    Connect { link: String },
    Forget,
    List,
    Upload(UploadMeta),
    Thumb { file_id: String },
    Download { file_id: String, out_path: PathBuf },
    Delete { file_id: String },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,iroh=warn,noq=warn".into()),
        )
        .init();

    // Every line to stdout goes through one writer task, so lines never interleave.
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

    let library: Arc<OnceCell<Library>> = Arc::new(OnceCell::new());
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
        let out = Out::new(env.id, tx.clone());
        let library = library.clone();
        // Requests run concurrently: a download must not hold up the gallery's thumbnails.
        tokio::spawn(async move {
            let id = env.id;
            let result = run(&library, env.cmd, &out).await;
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
    // stdin closed: the app has gone, so do we.
    drop(tx);
    let _ = writer.await;
    Ok(())
}

async fn run(cell: &OnceCell<Library>, cmd: Command, out: &Out) -> Result<Value> {
    if let Command::Init { data_dir, account_key, relays, relay_token } = cmd {
        let lib = Library::open(data_dir, &account_key, relays, relay_token).await?;
        let device = lib.device_id();
        cell.set(lib).map_err(|_| anyhow::anyhow!("already initialised"))?;
        return Ok(json!({ "device_id": device }));
    }
    let lib = cell.get().context("send init first")?;
    match cmd {
        Command::Init { .. } => unreachable!(),
        Command::Status => lib.status().await,
        Command::Connect { link } => lib.connect(&link).await,
        Command::Forget => lib.forget().await,
        Command::List => lib.list().await,
        Command::Upload(meta) => lib.upload(meta, out).await,
        Command::Thumb { file_id } => lib.thumb(&file_id).await,
        Command::Download { file_id, out_path } => lib.download(&file_id, &out_path, out).await,
        Command::Delete { file_id } => lib.delete(&file_id).await,
    }
}
