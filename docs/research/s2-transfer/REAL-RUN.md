# S2 over the real internet

The CI runs answered how to schedule reads. This run answers what CI cannot simulate: whether
iroh gets **direct paths through real home routers**, how fast **our own relays** carry data when
it does not, and what **real uplinks** deliver. Same tool, same strategies.

## What it needs

- **5–6 nodes**, each on a *different* home network or host (a friend's NAS, a spare desktop, a
  VPS, Buttercup). Six lets the client lose one and still have a spare; five is the minimum for
  4-of-5. Nothing needs a port opened: iroh dials out and punches through NAT. Outbound UDP must
  not be blocked.
- **The client**: a laptop on a decent home downlink. Ideally on a *seventh* network, so no
  node is on the client's LAN (a LAN node would make the result look better than the internet).
- **About 30 minutes** where the nodes are left running.

What a node does: answers requests with pseudo-random bytes from memory. It stores nothing,
reads nothing from disk, and exits after `--max-seconds` (2 h below) or when the client sends
the shutdown at the end of a run. It is a test tool: anyone who learns a node's id while it runs
can pull bytes from it or stop it, so do not leave it running.

## 1. Get the binary

Build it with `cargo build --release` in this directory (add `--target <triple>` for the
machines below; the static Linux builds need `musl-tools`):

| Machine | Artifact | Binary |
| --- | --- | --- |
| Linux x86-64 (most NASes, VPSes, PCs) | `s2-x86_64-unknown-linux-musl` | `s2` (static) |
| Linux arm64 (Raspberry Pi 4/5, ARM NAS, Ampere VPS) | `s2-aarch64-unknown-linux-musl` | `s2` (static) |
| Windows | `s2-x86_64-pc-windows-msvc` | `s2.exe` |
| Mac (Apple silicon) | `s2-aarch64-apple-darwin` | `s2` |

```sh
gh run download <run-id> -n s2-x86_64-unknown-linux-musl   # then: chmod +x s2
```

The Linux binaries are static, so on a NAS that only runs containers:
`docker run --rm -it --network host -v "$PWD":/s2 alpine /s2/s2 serve …`

## 2. Agree on the run

- **Seed**: one string for the whole run, e.g. `ursa-real-1`. Node keys come from it, so the
  client finds every node from the seed alone. Use a new seed for every run.
- **Index**: each node gets its own number, `0` to `5`.
- **Relays**: our own, not n0's public ones (which carried nothing in CI). The relays need the
  relay auth token in `S2_RELAY_TOKEN`; it is read from the environment so it never shows up in
  a process list or a shell history file. Do not commit it.

## 3. Start each node

Linux / macOS:

```sh
export S2_RELAY_TOKEN='…'
./s2 serve --seed ursa-real-1 --index 0 --max-seconds 7200 \
  --relay https://iroh-wus.streetcrypt.id --relay https://iroh-euw.streetcrypt.id
```

Windows (PowerShell):

```powershell
$env:S2_RELAY_TOKEN = '…'
.\s2.exe serve --seed ursa-real-1 --index 0 --max-seconds 7200 --relay https://iroh-wus.streetcrypt.id --relay https://iroh-euw.streetcrypt.id
```

Each node prints `node N id … ticket …` once it is online. Note its owner's **upload speed**
(any speed test), since that is the number the result is judged against.

## 4. Run the client

```powershell
$env:S2_RELAY_TOKEN = '…'
.\s2.exe fetch --seed ursa-real-1 --nodes 6 --seconds 60 --relay https://iroh-wus.streetcrypt.id --relay https://iroh-euw.streetcrypt.id --label "real run 1: <who/where, uploads>" --json real-1.json
```

That runs every strategy (each node alone, then fixed, race, pull) and prints a table, including
whether each node's path is **direct** or through a **relay**. Then, still with the same seed:

- **A failure, for real**: start `--strategies pull --seconds 120`, and about 40 s in, have one
  node owner stop their node (Ctrl-C, or pull its network cable for a black hole). Use a new seed
  for the next run, since that node now has to restart.
- **Bigger window** if pull is far below the sum of uploads: `--window 32`.

Add `--shutdown` to the last client run to stop every node at the end.

## 5. Record it

Commit `real-N.json` and the printed table under `results/real/`, with each node's location
(city level is plenty), connection type and measured upload. Never the relay token, and no
endpoint tickets (they hold home IP addresses).
