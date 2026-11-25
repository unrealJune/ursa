//! Developer telemetry for every Ursa binary: `tracing` events, exported as OTLP logs.
//!
//! Each binary installs [`layer`] once, beside whatever prints its logs (logcat on Android,
//! stderr elsewhere). The layer does nothing until [`configure`] names a collector, which the
//! desktop binaries do at start from their environment and the Android core does when the app
//! hands it the endpoint baked into its build. Configured once per process; a second call is
//! ignored.
//!
//! Every record carries the resource attributes that say where it came from:
//! `service.name` (`ursa-android`, `ursa-windows`, `ursa-node`, …), `service.version`, and
//! `service.instance.id`, the per-install telemetry id the app shows in its settings. The app's
//! own layer (JavaScript on Android, C# on Windows) exports with the same id, so one query
//! shows a device's app and core together; `ursa.layer` tells them apart.
//!
//! Nothing here reads file names or contents: it exports what the code already logs.

use std::sync::OnceLock;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use opentelemetry::KeyValue;
use opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge;
use opentelemetry_otlp::{LogExporter, WithExportConfig, WithHttpConfig};
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::logs::{BatchLogProcessor, SdkLogger, SdkLoggerProvider};
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::Layer;
use tracing_subscriber::layer::Context as LayerContext;

/// Where a process's telemetry goes, and who it says it is.
#[derive(Clone, Debug)]
pub struct Config {
    /// The OTLP/HTTP base, e.g. `https://otlp.example.com` (`/v1/logs` is appended).
    pub endpoint: String,
    /// `ursa-android`, `ursa-windows`, `ursa-node`, …
    pub service: String,
    /// `core`, `host`, `node`: which part of the app this process is.
    pub layer: String,
    pub version: String,
    /// The install's telemetry id, shared with the app's own layer.
    pub instance_id: String,
}

impl Config {
    /// From the standard environment: `OTEL_EXPORTER_OTLP_ENDPOINT` (absent ⇒ None),
    /// `OTEL_SERVICE_INSTANCE_ID` (else the host name, else "unknown") and
    /// `OTEL_SERVICE_VERSION` (else `version`: the app that starts a process knows its own).
    pub fn from_env(service: &str, layer: &str, version: &str) -> Option<Self> {
        let endpoint = std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT")
            .ok()
            .filter(|e| !e.trim().is_empty())?;
        let instance_id = std::env::var("OTEL_SERVICE_INSTANCE_ID")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .or_else(|| std::env::var("HOSTNAME").ok())
            .or_else(|| std::env::var("COMPUTERNAME").ok())
            .unwrap_or_else(|| "unknown".into());
        let version = std::env::var("OTEL_SERVICE_VERSION")
            .ok()
            .filter(|v| !v.trim().is_empty())
            .unwrap_or_else(|| version.into());
        Some(Self {
            endpoint,
            service: service.into(),
            layer: layer.into(),
            version,
            instance_id,
        })
    }
}

struct Exporting {
    provider: SdkLoggerProvider,
    bridge: OpenTelemetryTracingBridge<SdkLoggerProvider, SdkLogger>,
}

static EXPORTING: OnceLock<Exporting> = OnceLock::new();

/// The layer every binary installs. Forwards events to the collector once [`configure`]d.
/// Filtered on its own (a per-layer filter), so what it skips stays available to the console
/// layer beside it, and what nobody wants is never turned on.
pub fn layer<S>() -> impl Layer<S> + Send + Sync
where
    S: Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
{
    Otlp.with_filter(tracing_subscriber::filter::filter_fn(exported))
}

/// See [`layer`].
pub struct Otlp;

impl<S: Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>> Layer<S> for Otlp {
    fn on_event(&self, event: &Event<'_>, ctx: LayerContext<'_, S>) {
        if let Some(e) = EXPORTING.get() {
            e.bridge.on_event(event, ctx);
        }
    }
}

/// What goes to the collector: Ursa's own events from info up, everyone else's from warn up,
/// and never the exporter's own HTTP stack (an export failure that logs would export itself).
fn exported(m: &tracing::Metadata<'_>) -> bool {
    let target = m.target();
    for quiet in ["opentelemetry", "reqwest", "hyper", "h2", "rustls", "tower"] {
        if target.starts_with(quiet) {
            return false;
        }
    }
    let ours = target.starts_with("ursa");
    let level = *m.level();
    if ours {
        level <= Level::INFO
    } else {
        level <= Level::WARN
    }
}

/// Starts exporting. Returns false when telemetry was already configured in this process.
pub fn configure(cfg: Config) -> Result<bool> {
    if EXPORTING.get().is_some() {
        return Ok(false);
    }
    let url = format!("{}/v1/logs", cfg.endpoint.trim().trim_end_matches('/'));
    // reqwest's blocking client runs its own runtime, and building or dropping one inside a
    // tokio context panics; every caller of this is inside one. So it is built on a thread.
    let exporter = std::thread::spawn(move || -> Result<LogExporter> {
        let client = reqwest::blocking::Client::builder()
            // Its own roots, not the platform verifier: on Android that verifier needs a JNI
            // context the core does not have.
            .tls_certs_only(
                webpki_root_certs::TLS_SERVER_ROOT_CERTS
                    .iter()
                    .filter_map(|c| reqwest::Certificate::from_der(c).ok()),
            )
            .timeout(Duration::from_secs(15))
            .build()
            .context("telemetry HTTP client")?;
        LogExporter::builder()
            .with_http()
            .with_http_client(client)
            .with_endpoint(url)
            .build()
            .context("OTLP log exporter")
    })
    .join()
    .map_err(|_| anyhow!("telemetry setup panicked"))??;
    let resource = Resource::builder()
        .with_service_name(cfg.service.clone())
        .with_attributes([
            KeyValue::new("service.version", cfg.version.clone()),
            KeyValue::new("service.instance.id", cfg.instance_id.clone()),
            KeyValue::new("ursa.layer", cfg.layer.clone()),
            KeyValue::new("os.type", std::env::consts::OS),
        ])
        .build();
    let provider = SdkLoggerProvider::builder()
        .with_resource(resource)
        .with_log_processor(BatchLogProcessor::builder(exporter).build())
        .build();
    let bridge = OpenTelemetryTracingBridge::new(&provider);
    if EXPORTING.set(Exporting { provider, bridge }).is_err() {
        return Ok(false);
    }
    tracing::info!(service = %cfg.service, layer = %cfg.layer, version = %cfg.version, "telemetry: exporting");
    Ok(true)
}

/// Logging for a desktop or server process: lines on stdout or stderr (`RUST_LOG`, else
/// `default_filter`), plus the collector when the environment names one ([`Config::from_env`]).
pub fn init_console(default_filter: &str, to_stderr: bool, telemetry: Option<Config>) {
    use tracing_subscriber::prelude::*;
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| default_filter.into());
    let console = if to_stderr {
        tracing_subscriber::fmt::layer()
            .with_writer(std::io::stderr)
            .boxed()
    } else {
        tracing_subscriber::fmt::layer().boxed()
    };
    tracing_subscriber::registry()
        .with(filter)
        .with(console)
        .with(layer())
        .init();
    if let Some(cfg) = telemetry
        && let Err(e) = configure(cfg)
    {
        tracing::warn!("telemetry: {e:#}");
    }
}

/// Sends what is buffered. For a process about to exit (or be suspended).
pub fn flush() {
    if let Some(e) = EXPORTING.get() {
        let _ = e.provider.force_flush();
    }
}
