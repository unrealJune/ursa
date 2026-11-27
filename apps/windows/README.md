# Ursa for Windows

A WinUI 3 app (.NET, unpackaged) with the Rust core beside it: `ursa-host.exe`
([`crates/ursa-host`](../../crates/ursa-host)), which the app starts and drives over stdio. The
app keeps the account key under DPAPI and hands it to the host, which holds it in memory only.

## Build

You need the .NET SDK named in [`global.json`](global.json) and stable Rust. From the repository
root:

```powershell
cargo build --release -p ursa-host
cd apps/windows/Ursa.App
dotnet build -c Release -r win-x64 -p:Platform=x64
```

The build copies `target/release/ursa-host.exe` next to `Ursa.exe` when it exists. Use
`-r win-arm64 -p:Platform=ARM64` on an ARM PC.

## Releases and updates

[`release-windows.yml`](../../.github/workflows/release-windows.yml) publishes the app and
packages it with [Velopack](https://velopack.io) into a per-user installer, a portable zip and an
update feed per architecture, attached to a `windows-vX.Y.Z` GitHub release.

Installed copies update themselves from those releases
([`Services/Updater.cs`](Ursa.App/Services/Updater.cs)): Ursa checks shortly after it starts and
every six hours, downloads an update in the background, and installs it when it closes, or at
once from Settings → Updates. A copy run from `bin\` (not installed) never updates.

## Diagnostics

A build can name an OpenTelemetry collector (`-p:UrsaOtelEndpoint=…`, or `URSA_OTEL_ENDPOINT` at
run time). Nothing is sent until the user turns on Settings → This PC → Send diagnostics; builds
with `-p:UrsaTelemetryDefault=on` start with it on. Diagnostics carry errors and timings, never
file names, contents or keys.
