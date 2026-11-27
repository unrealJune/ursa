# Ursa

Encrypted photo and file backup across storage you and your friends run.

Your devices encrypt everything before it leaves them, cut it into erasure-coded shards, and
spread those shards across always-on nodes: your own home node and the nodes your friends give
to the pool. No node can read what it stores, any `k` of a stripe's `n` shards rebuild it, and a
read pulls from the fastest nodes in parallel. You store on others roughly what you host for
them.

Ursa is early software. Formats and protocols can still change between releases.

## What's here

| Path | What it is |
| --- | --- |
| [`crates/ursa-core`](crates/ursa-core) | Keys, content-defined chunking, per-chunk encryption, manifests |
| [`crates/ursa-proto`](crates/ursa-proto) | The client↔node protocol over [iroh](https://iroh.computer) |
| [`crates/ursa-client`](crates/ursa-client) | A device: account session, the pool, coded storage, recovery, linking |
| [`crates/ursa-node`](crates/ursa-node) | The storage node (disk or any S3-compatible store) |
| [`crates/ursa-host`](crates/ursa-host) | The process the Windows app drives over stdio |
| [`crates/ursa-ffi`](crates/ursa-ffi) | `ursa-client` for the Android app, through UniFFI |
| [`crates/ursa-cli`](crates/ursa-cli) | `ursa`, a device on the command line |
| [`crates/ursa-telemetry`](crates/ursa-telemetry) | Opt-in developer telemetry (OTLP logs) |
| [`apps/windows`](apps/windows) | The Windows app (WinUI 3) |
| [`apps/android`](apps/android) | The Android app (Expo + the Rust core) |
| [`charts/ursa-node`](charts/ursa-node) | Helm chart for a node |
| [`docs/design.md`](docs/design.md) | Design, threat model and roadmap |
| [`docs/research`](docs/research) | The measurements behind the design (codecs, transfer, post-quantum crypto) |

## Install

Every release is cut automatically from `main` and published on the
[Releases page](https://github.com/unrealJune/ursa/releases). Each product has its own version
and tag: `windows-vX.Y.Z`, `android-vX.Y.Z` and `node-vX.Y.Z`.

### Windows

Download `Ursa-<version>-x64-Setup.exe` (or `-arm64-`) from the latest `windows-v*` release and
run it. Ursa installs per user, without admin rights, and updates itself from GitHub Releases.
Builds are not code-signed yet, so SmartScreen will warn on first run.

### Android

Download `Ursa-<version>.apk` from the latest `android-v*` release and install it. Updates are
not automatic yet: install the new APK over the old one.

### A storage node

Image: `ghcr.io/unrealjune/ursa-node` (linux/amd64 and linux/arm64), tagged `X.Y.Z`, `X.Y`,
`latest` and `sha-<commit>`.

With Helm, first create a Secret with the node's identity and (for S3) its credentials:

```sh
kubectl create secret generic ursa-node \
  --from-literal=URSA_SECRET_KEY="$(openssl rand -hex 32)" \
  --from-literal=AWS_ACCESS_KEY_ID=... \
  --from-literal=AWS_SECRET_ACCESS_KEY=...

helm install ursa-node oci://ghcr.io/unrealjune/charts/ursa-node \
  --set existingSecret=ursa-node \
  --set s3.endpoint=http://seaweedfs-s3:8333
```

Or keep objects on a volume with `--set store=disk --set persistence.enabled=true`. Every option
is documented in [`values.yaml`](charts/ursa-node/values.yaml). To follow releases
automatically (Flux, Argo CD, Renovate), track the chart with a semver range such as `0.x`.

With Docker:

```sh
docker run -d --name ursa-node -p 7842:7842/udp \
  -e URSA_SECRET_KEY="$(openssl rand -hex 32)" -e URSA_PORT=7842 \
  -e URSA_STORE=disk -e URSA_DATA_DIR=/data -v ursa-data:/data \
  ghcr.io/unrealjune/ursa-node:latest
```

Keep `URSA_SECRET_KEY`: it is the node's identity.

## Build from source

You need stable Rust. Then:

```sh
cargo test --workspace
cargo build --release -p ursa-node -p ursa-cli
```

The Windows and Android apps have their own instructions in
[`apps/windows`](apps/windows) and [`apps/android/README.md`](apps/android/README.md).

Ursa's relays take an access token. Release builds bake one in from the `URSA_RELAY_TOKEN`
repository secret (see [`relays.rs`](crates/ursa-proto/src/relays.rs)); it is never in the
source. Your own builds have none unless you set `URSA_DEFAULT_RELAY_TOKEN` when building, or
enter a token in the app's network settings.

## Contributing

Commits follow [Conventional Commits](https://www.conventionalcommits.org): CI reads them to
decide each release. `feat:` cuts a minor release, `fix:`/`perf:` a patch, `feat!:` or a
`BREAKING CHANGE:` footer a major one; `docs`, `chore`, `ci`, `test`, `style`, `build` and
`refactor` release nothing. Pull requests must pass the [CI workflow](.github/workflows/ci.yml).

Report security problems privately: see [SECURITY.md](SECURITY.md).

## License

Ursa is free software under the [GNU General Public License v3.0 or later](LICENSE).
