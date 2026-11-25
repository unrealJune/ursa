# Ursa: plan

Status: draft 2, 2026-11-23 (client-first ordering). A working document. Decisions marked **DECIDE** are still open.

## What it is

Your friends run always-on storage nodes. Each node gives the pool some disk. Your photos,
videos **and any other files** (Drive-style: folders, documents, archives, project trees) are
encrypted on your device, cut into shards, and spread across those nodes so that:

- **No host can read anything.** A host stores ciphertext fragments under random-looking IDs.
- **Losing hosts loses nothing.** Any `k` of a stripe's `n` shards rebuild it, so a friend's
  NAS dying, or a friend leaving, costs nothing until more than `n − k` of them go at once.
- **Reads are fast.** A read pulls shards from the `k` fastest hosts in parallel (RAID-0
  striping across the internet), and starts asking a spare host if one is slow.
- **You store what you give.** What you place on others is bounded by what you host for them.

The difference from Street Cryptid: storage nodes are assumed to be up. Phones are clients
that upload and browse, never storage, so none of the background/power-state machinery
comes along. (Phone *upload* still lives under iOS background rules; see spike S5.)

## Where the work is

The storage layer is the small part: storing a blob, serving a byte range, holding leases. It is
a few thousand lines on top of existing crates. **The product is the client**: backup that just
works, a timeline that scrolls instantly, sharing that is obvious. The plan is ordered so the app
is usable early, and the distributed storage arrives behind a fixed interface while it is.

## Prior art (references, not dependencies)

Decided 2026-11-23: we depend on neither Tahoe-LAFS nor Ente. We read both and take their ideas.

- **Tahoe-LAFS**: the same idea from 2007 (a "friendnet" grid, k-of-n erasure coding, client-side
  encryption, capability-based sharing). **Take**: the capability model (read/write/verify caps),
  leases and garbage collection, the repair design, the accounting ideas, and its threat-model
  write-ups. **Not a base** because: it is Python/Twisted with nothing a phone can embed (phones
  would need a web gateway); storage servers must be reachable addresses, with no NAT traversal;
  mutable files are signed with RSA, which fails the post-quantum goal; small 128 KiB segments
  make it slow on big files; it has no accounts, devices or albums, which is most of what we
  build anyway. Gridsync (a polished desktop client for Tahoe) is the warning: a good client on
  that base stayed niche.
- **Ente Photos** (AGPL, E2E-encrypted): the UX bar. **Take**: the backup flow and its edge
  cases (iOS/Android photo-library permissions, limited-library access, Live Photos, edits,
  duplicates, HEIC/RAW), the timeline and album UX, on-device ML search, their per-file key
  layout, and how they lay out web share links. **Not a base** because its client is built
  around one central object store and libsodium (not post-quantum), and a fork would put us on
  their release train under AGPL. Spike S0 reads its code for the above.
- **Self-hosted Ente + Garage** (Ente's server "museum" on a geo-distributed S3 store spread
  across friends' homes) was considered and rejected on 2026-11-23. It gets E2E photos on
  friends' hardware in a weekend, but: one central server and Postgres that everyone depends
  on; Garage trusts every node, so a bad node is detected on read but not repaired around; 3×
  replication with single-node reads instead of coded shards read in parallel; no post-quantum
  wrapping of shared-album keys; no per-friend reciprocity; nodes need a VPN instead of iroh's
  hole punching. Sharding, no center, and iroh's transport and speed are the point of Ursa.
  **Garage** remains worth reading for zone-aware placement and its layout/rebalancing design.
- **Immich**: a self-hosted photo app with excellent UX, but server-centric and not
  E2E-encrypted. Reference for the timeline, map, faces and the web UI.
- **Storj / Sia**: erasure-coded storage across untrusted hosts at scale. Storj's
  long-tail cancellation (request more shards than needed, cancel the slowest) is the read
  speed trick.
- **Peergos**: E2E-encrypted file sharing with a good capability/sharing design.

## Threat model

| Party | Can | Must not |
| --- | --- | --- |
| A storage host (friend) | delete, withhold, corrupt or serve old shards; see sizes, timing, which node stores for whom | read content, read metadata (names, dates, GPS, album structure), forge content |
| Network / relay | observe, record for later | read content, link shards to files |
| Someone recording today with a future quantum computer | decrypt anything protected only by classical public-key crypto | read stored or shared content |
| A recipient you shared with | keep what they downloaded forever | read anything you did not share with them |
| A lost or stolen device | read what that device could read | keep reading new content after it is revoked |

What follows from it:

- **Content confidentiality is symmetric, and 256-bit symmetric is already post-quantum.**
  Grover halves the effective strength; 256-bit keys stay at 128. You are right that the bulk
  of this needs no PQ public-key crypto at all: you encrypt with a key you hold.
- **PQ public-key crypto is needed only where a key crosses between identities**: adding a
  device, sharing with a friend, a share link to a person, recovery. That is where
  "harvest now, decrypt later" bites, so those wraps are hybrid (classical + ML-KEM).
- **No ratchet.** A ratchet buys forward secrecy for a *conversation*: old messages stay safe
  if today's key leaks. Archival storage is the opposite: you must be able to read your 2019
  photos with today's keys, so a key compromise exposes the archive by construction, ratchet or
  not. What we want instead is **key hierarchy + rotation**: revoking a device or a recipient
  rotates the keys that protect *future* content, and the old content's exposure is accepted
  (they could have downloaded it anyway).
- **Transport encryption is defence in depth, not the boundary.** Everything on the wire is
  already ciphertext. iroh's QUIC/TLS 1.3 is enough; turning on the hybrid `X25519MLKEM768`
  key exchange (rustls supports it) would protect the *metadata* (who fetched which shard IDs)
  against a recording adversary. Nice to have, not a blocker (spike S4).
- **Integrity comes from hashes, not from trusting hosts.** Every shard is named by its
  BLAKE3 hash and checked on receipt; every manifest is signed.

## Crypto, concretely

| Job | Choice | Why |
| --- | --- | --- |
| Content encryption | AES-256-GCM (aws-lc-rs), one key per chunk (derived from an account secret and the chunk's content) | Post-quantum as it stands: a 256-bit key keeps ~128 bits against Grover. Hardware AES on every phone and desktop; S1 measured 6.4 / 7.5 GB/s seal / open on one Apple M2 core. Each key encrypts exactly one plaintext, so nonces cannot repeat. aws-lc-rs also ships ML-KEM, so one library covers both halves. XChaCha20-Poly1305 is the fallback if a platform lacks AES instructions. |
| Hashing / content IDs | BLAKE3 | Fast (SIMD, parallel), and **Bao** gives verified streaming: a range of a file can be checked without the whole thing. iroh-blobs is built on it. |
| Key derivation | BLAKE3 `derive_key` / HKDF-SHA256 | |
| Key wrapping across identities | **X-Wing** (X25519 + ML-KEM-768) from **libcrux**, then HKDF-SHA256 into AES-256-GCM (HPKE single-shot shape) | Hybrid: breaks only if both break. libcrux is formally verified and the one choice that builds for native and the browser (aws-lc-rs cannot target wasm). S3: under 0.5 ms to wrap or unwrap anywhere; interoperates with RustCrypto and aws-lc-rs from the same seeds. X-Wing is a draft, so wrapped keys carry a version byte. |
| Signatures (library-log commits, device certs, quota receipts) | Hybrid **Ed25519 (`ed25519-dalek`) + ML-DSA-65 (libcrux)**, both must verify | Forgery needs a quantum computer *at signing-verification time*, which is not a harvest-later threat, so this is lower priority than the KEM; still cheap enough to do from day one and painful to retrofit. S3: 0.2 ms to sign and 0.05 ms to verify on an iPhone-class core. A hybrid signature is 3.4 KB, so commits are signed, never individual files. |
| Transport | iroh QUIC (TLS 1.3, Ed25519 node identity) | Node identity is classical. Acceptable: impersonating a node needs a live quantum attack and still yields only ciphertext. |

**Never plain convergent encryption** (key = hash of the content). It deduplicates across
users but lets anyone holding a file test whether you have it. Chunk keys are derived from the
content *and an account secret*, so dedup works only within one account and nobody outside it
can run that test.

## Data model

```
Account
 ├─ root key (32 B, symmetric)          → never leaves enrolled devices / recovery
 ├─ identity keypair (X-Wing + hybrid sig) → what friends wrap things to
 ├─ devices[]  (each: own keypair, cert signed by the account)
 └─ Library (encrypted, synced to every own device, backed up to the pool as a small object)
     ├─ Drives / folders (a tree)        → each shared root has a folder key
     ├─ Albums (a view over media files) → each has an album key
     ├─ Media index: captured-at, GPS, EXIF, faces/search embeddings (encrypted)
     └─ File manifest (one per version)
          ├─ name, size, mime, mtime      (encrypted with the file key)
          ├─ chunks[] → chunk id, chunk key, (stripe, offset, length)
          └─ renditions (media only): preview (~2 MB), thumbnail (~30 KB), video proxies
```

A photo is a file that also has renditions and a media-index entry. Photos and arbitrary files
are one storage system and one library; the photo timeline and the Drive-style file browser are
two views of it.

### How a file is stored: chunks, then stripes

Two layers, because the unit you want for **content** (dedup, edits, versions, small files) is not
the unit you want for **storage** (coding, placement, big parallel reads).

1. **Chunk** the file with content-defined chunking (FastCDC; sizes from **spike S1**, start
   around 1–4 MiB average). Editing the middle of a 2 GB file changes a few chunks, and only
   those upload. Unchanged chunks are shared between versions and between files.
2. **Encrypt each chunk with its own key**, derived from an account secret and the chunk's
   content (`chunk key = KDF(account dedup secret, BLAKE3(plaintext))`). This deduplicates
   within your account only: hosts and friends cannot test whether you hold a known file, and
   identical chunks of two *different* accounts look unrelated. Each key encrypts exactly one
   plaintext, so the AES-GCM nonce can be fixed. The manifest lists each chunk's key, so
   sharing a file means sharing its (wrapped) manifest and nothing else.
3. **Pack chunks into stripes** of a fixed size (8 MiB, from S1). A 3 KB text
   file shares a stripe with other chunks instead of costing six shards and six round trips.
   Big files fill stripes of their own.
4. **Erasure-code each stripe** into `n` shards, any `k` of which rebuild it (Reed–Solomon,
   `reed-solomon-simd`). The code is **systematic**: the first `k` shards are the stripe's bytes,
   cut in order. So a healthy read of one chunk is a plain range read from the one or two data
   shards that hold it, with no decoding; a big sequential read pulls from all `k` in parallel;
   only a degraded read decodes, and only the byte range it needs (RS columns are independent).
5. **Name** each shard by its BLAKE3 hash and **place** each shard of a stripe on a different
   node (and preferably a different friend and location). The stripe → node map lives in a
   stripe table on the home node, not in each file manifest, so repair moves a shard without
   touching any manifest.

`k`/`n` is **chosen per pool from an availability target and measured node uptime** (see
"Reliability on hobbyist nodes"), with 4-of-6 as the minimum shape: 1.5× overhead (vs 3× for
triple replication), survives any 2 nodes gone. Wider codes (8-of-12, 6-of-10, 4-of-8) are what
hobbyist uptime actually needs. A pool of fewer than 6 nodes falls back to replication. It is a
per-folder setting through the protection levels; thumbnails are replicated to everyone.

**Deleting and compaction.** Deleting a file drops its references; a chunk with no references
is dead. A stripe that is mostly dead is **repacked**: its live chunks move into new stripes
and the old shards are deleted (restic's `prune` is the model). Versions are kept for a
retention window (e.g. 30 days) before their chunks can die.

### Thumbnails and previews are a separate tier

Browsing must feel local. Thumbnails and small previews are **replicated** (not coded) to every
one of your devices and to several pool nodes, and synced eagerly. Originals are coded and
fetched on demand. This is what makes a 50,000-photo library scroll instantly.

## The pool

### Placement

Rendezvous (HRW) hashing over the nodes you may use, weighted by each node's free quota for
you, with hard constraints: one shard of a stripe per node, at most `n − k` shards of a stripe
per friend (so a friend who owns three nodes cannot take a stripe out alone), and spread across
declared locations where possible. Deterministic placement means a client can often *find*
shards without asking anyone; the manifest still records the truth.

### Storage allocation ("store what you give")

- Each host sets a **quota per friend** (or a pool-wide default) and enforces it locally: it
  refuses shards past the quota. That is the only enforcement that matters; everything else
  is accounting.
- **Fair share rule** (default, **DECIDE** the ratio): the bytes you place across the pool
  ≤ the capacity you contribute. The app shows it as one bar: "Giving 2 TB · Using 1.4 TB".
- Hosts sign **storage receipts** (shard ID, size, lease expiry) so both sides agree on the
  ledger and a client can prove what it was promised.
- **Leases**: every shard is held under a lease (e.g. 90 days) the owner renews in bulk. A
  shard whose owner vanished (lost account, left the pool) is garbage collected when the lease
  lapses. Without leases, dead accounts would fill friends' disks forever.

### Audit and repair

- Your own always-on node (or any always-on device you own) is the **repair agent**. It holds
  the manifests, so it knows where everything is.
- **Audit**: periodically ask each host for a random byte range of a random shard with its
  Bao proof. Cheap, and a host cannot fake it without the data.
- **Repair**: when a stripe drops to `k + 1` healthy shards (configurable), fetch `k`,
  regenerate the missing ones, place them elsewhere, update the manifest. A node gone past a
  grace period (e.g. 72 h) is treated as lost.
- **Leaving the pool gracefully**: a node announces it is draining; owners move their shards
  off before it goes.

### Reliability on hobbyist nodes, and how much to trust each one

The nodes are friends' NASes and spare desktops: they reboot for updates, lose power, sit behind
flaky home internet, and sometimes just disappear. Ursa assumes that and is built so it does
not show.

**Two different failures.** *Unavailable*: too many hosts are down right now to read a stripe;
it comes back when they do. *Lost*: too many shards are gone for good before repair replaced
them. Repair (above) protects against loss; only the code's width and the home node's copies
protect availability.

**The code must fit the nodes' real uptime.** Hours per year that a stripe cannot be read, if
each node is independently down a fraction `p` of the time:

| Code | Overhead | Survives | Nodes needed | 98% up | 95% up | 90% up |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 4-of-6 | 1.50× | 2 | 6 | 1.3 h | 20 h | 139 h |
| 6-of-9 | 1.50× | 3 | 9 | 0.2 h | 5.6 h | 73 h |
| 8-of-12 | 1.50× | 4 | 12 | 0.02 h | 1.6 h | 38 h |
| 6-of-10 | 1.67× | 4 | 10 | 0.006 h | 0.6 h | 14 h |
| 4-of-8 | 2.00× | 4 | 8 | 0.001 h | 0.14 h | 3.8 h |
| 8-of-14 | 1.75× | 6 | 14 | 0 | 0.02 h | 1.6 h |

So **4-of-6 is too thin for hobbyist nodes**: at 95% uptime it leaves some of your files
unreadable for about 20 hours a year. Wider codes buy far more than extra parity does at the
same overhead (8-of-12 is 12× better than 4-of-6 for the same 1.5×). The default is therefore
not a fixed `k`/`n` but a **target** ("readable 99.99% of the time"), and the app picks the
narrowest code that meets it from the pool's *measured* uptime and size. On a small pool that
means more overhead; as the pool grows, the same target gets cheaper. (4-of-6 stays the
benchmark shape and the minimum.)

Real failures are not independent: one house losing power takes every node in it. Placement
already counts **failure domains** (a friend, a site), and the target is computed over domains,
not nodes. Two nodes in one closet count as one.

**Speed when a node is offline or slow.** Nothing waits for a missing node:

- The pull scheduler (S2) only ever needs `k` of `n`. An offline node is skipped (a failed
  request takes it out of rotation until it reconnects, with backoff); a slow node gets
  proportionally fewer shards; a node that goes silent mid-transfer has its outstanding shards
  duplicated elsewhere once they are overdue against that node's own measured rate.
- Reading around a missing shard costs a decode: 0.5–0.8 GB/s per core (S1), far above any link.
- The home node's **hot cache** of whole copies serves recent and pinned files without the
  pool at all, so the most common reads never depend on friends' uptime.
- **Measured (S2):** with the fastest of six nodes (200 Mbit/s) killed, black-holed or slowed to
  2 Mbit/s 20 s into a read, every 5 s window afterwards still delivered 94-121 Mbit/s, at the
  112 Mbit/s ceiling of the five nodes left, with no stall. A silent node costs the same as a
  crashed one, because requests are judged against each node's own measured speed. What a
  failure takes away is that node's capacity, which is why breadth and the home node's cache
  are the defence.

**Trust is earned and measured, per node.** Every node has a score built from what Ursa
observes: uptime from heartbeats, audit pass rate, measured throughput, how long it has been in
the pool. Placement weights by it; reads prefer it.

- **New nodes are on probation**: they hold extra shards *beyond* the code (a 4-of-6 stripe
  gets a 7th shard there) so nothing depends on them until they have a month of history.
- **Flaky nodes are demoted** automatically: fewer shards, parity rather than data shards, then
  none.
- A failed audit (wrong bytes) removes a node from placement at once and starts repair.

**Ways to limit exposure, all configurable per folder or album:**

| Protection | What it does | Overhead |
| --- | --- | --- |
| Standard (default) | Code chosen to meet the availability target from measured uptime | 1.5–2× in the pool |
| Important | A stricter target (tolerates more failures), plus a whole copy on the home node | 2–2.5× pool + 1× home |
| Home + pool | Always a whole copy on the home node; the pool is the off-site backup | 1.5× pool + 1× home |
| Archive | Wide code, no cache; cheapest, slowest to read | ~1.5× pool |

And independent of the level:

- **Per-friend caps**: at most `n − k` shards of a stripe per friend (a hard rule), lowerable to
  one; a byte cap per friend; and a minimum trust score per folder.
- **Exclude** a node or a friend from a folder entirely.
- **Evacuate**: move every shard off a node now (a friend is moving, or you stopped trusting a
  box); the app shows progress and when exposure is back to normal.
- **Advanced**: set `k`/`n` directly.

**Make the state visible.** One health line answers "is everything safe?": *All files readable ·
1 node down for 3 h · repair starts in 69 h*, and each folder shows its margin (*can lose 3 more
nodes*). A phone that just takes photos should never need more than that line.

## Speed

- **Reads: a pull scheduler, not a race** (S2). Each host keeps two requests in flight and
  takes the oldest stripe that still needs a shard it holds; any `k` of the `n` hosts complete
  a stripe, so fast hosts serve more stripes and slow ones fewer, and every uplink stays busy.
  An idle host duplicates the shard holding up the oldest stripe once that stripe is overdue.
  Measured against throttled home-like uplinks: 2.7× a fixed assignment and 1.3× racing
  `k + 1` (Storj's long-tail cancellation). Racing also burns friends' upload on shards that
  lose: the two slowest hosts sent bytes nobody used. Keep at least 8 stripes (64 MiB) in flight
  at home-link speeds; 4 cost a quarter of the throughput.
- **Striping helps most when hosts are alike.** Each stripe needs `k` *different* hosts, so no
  host can supply more than `1/k` of the bytes. Six similar uplinks read at nearly their sum;
  one fat uplink among thin ones is capped at a quarter (S2: 197 Mbit/s striped vs 188 from the
  fastest host alone). Your own home node is usually that fat, close host, so it also keeps
  **whole copies of what you open often** (a hot cache), and reads go to it first.
- **Our own relays, always; direct paths are the norm.** From the open internet, iroh punched
  through two NATs (Kubernetes and the home router) to five nodes on Buttercup and read at
  841 Mbit/s, 2.4x the best single node. With n0's public relays the same setup got no direct
  path and ~0.3 Mbit/s, so they must never be the default. Ursa ships with its own relays.
- **The home node must be directly reachable on its own LAN.** In a Kubernetes pod it was
  not: a laptop on the same network went out to the relay and back (114-201 Mbit/s). The home
  node runs on the host network (or advertises a published UDP port) and is found on the LAN
  by local discovery, because it is the cache every nearby device reads first.
- **Uploads** are bounded by *your* uplink and cost `n/k` × the file size. Two mitigations:
  1. **Upload via your own node.** A phone sends the encrypted segments to your home node over
     LAN (or once, over the internet), and the home node does the coding and fan-out. It holds
     only ciphertext plus keys you already trust it with (it's yours). Every account has one
     (decided).
  2. **Delegated fan-out**: a well-connected host may re-send shards to their other hosts. Safe,
     because shards are ciphertext and checked by hash on arrival.
- **Local cache**: recently viewed originals stay on device; LAN peers (your own devices)
  are tried first.
- Benchmarks before design freeze: **spike S1/S2**.

## Devices

The account must feel like one thing across phone, laptop and desktop.

- **Adding a device**: scan a QR on an existing device (Street Cryptid's pairing + SAS
  pattern). The existing device wraps the account root key to the new device's X-Wing key and
  signs its device cert. One scan, no passwords.
- **Recovery**: a recovery phrase (the root key) and, optionally, **social recovery**: the root
  key Shamir-split across `t`-of-`m` friends, which fits a friends pool naturally.
- **Revoking a device**: remove its cert, rotate the root key and every album key, re-wrap
  manifests (small objects; shards are untouched because chunk keys live inside manifests and
  only the wrapping changes). A revoked device keeps what it already decrypted.
- **Library sync between own devices**: a small signed, encrypted operation log (CRDT) per
  library, replicated to all own devices and backed up to the pool. **Not iroh-docs** (see
  Networking).

## Sharing

- **Unit of sharing is the album** (or a single file, which is an album of one). Sharing gives
  the recipient the album key, HPKE-wrapped to their identity key.
- **Roles**: viewer (reads), contributor (adds to a shared album; their items are wrapped by
  the same album key, and they pay the storage for their own uploads), owner.
- **Links**: `https://ursa.…/s#<capability>` with the key in the fragment, so the server never
  sees it. Optional passphrase, expiry. A link for someone without the app is served by a web
  reader (wasm, decrypts in the browser), like the Street Cryptid web build.
- **Controls, and what they honestly mean**:
  - *Revoke*: rotates the album key so the person gets nothing added later. They keep what they
    downloaded; the UI says so.
  - *Expiry*: the same rotation on a timer.
  - *View-only / no download*: an honour-system flag in the app. **Say so in the UI.** Anything
    a screen shows can be kept.
  - *Strip location / EXIF on share* (default on): the shared rendition is re-encoded without
    GPS and device metadata; your original keeps it.
  - *Per-recipient renditions*: share a 2048 px version, keep the original private.
- Revocation that rotates keys means re-wrapping small manifests, never re-uploading shards,
  because chunk keys live inside manifests. That is the main reason keys are per chunk and
  listed in the manifest rather than derived from a shared folder or album key.
- **Folders share like albums**: a shared folder has a folder key wrapping its manifests;
  editor/viewer roles, links and revocation work the same. An editor's uploads count against
  the editor's own storage.

## Networking: what to keep, what to write

Street Cryptid's lessons: the iroh **endpoint** (QUIC, hole punching, relays, discovery) on 1.4
is solid after the noq patch. The pain was **iroh-docs** (engine death, stranded blobs) and
**iroh-blobs' content-fetch semantics** (one attempt, wait for gossip).

Recommendation:

- **Depend on the iroh endpoint** (vendored/patched as in Street Cryptid only where needed).
  Writing our own NAT traversal and relay layer is months for nothing.
- **Write our own protocols** on it, as ALPNs: `ursa/shard/1` (put, get-range, audit, lease
  renew, delete), `ursa/pair/1`, `ursa/sync/1` (library log). Each is small, owned, and testable
  against fakes. A shard store is far simpler than iroh-blobs: content-addressed files on
  disk, a ledger, a lease table.
- **Reuse `bao-tree`** for verified range reads, not iroh-blobs as a whole.
- No iroh-docs, no gossip. Library sync is a log we own.
- Self-hosted relays, like Street Cryptid.

This is "vendor our own" for the parts that hurt, without taking on the parts that work.

## Products

- **Node** (always-on): Rust daemon. Linux/Docker, NAS (Synology/Unraid via Docker), Windows
  service, macOS. Stores shards for the pool, and for its owner is the repair agent and
  upload fan-out point. Shares the structure of `repeater/host`.
- **Desktop app**: browse, upload folders, and optionally runs a node. Its main job for
  arbitrary files is a **sync client**, Drive-for-desktop style: an Ursa folder in Explorer /
  Finder with files shown as placeholders that download on open (Windows Cloud Files API,
  macOS File Provider), plus "keep this folder on this device".
- **Files on the phone**: the iOS Files app (File Provider extension) and Android's document
  picker (`DocumentsProvider`), so any app can open and save into Ursa.
- **Phone app**: camera roll backup, browse, share. Not storage. Expo + UniFFI, the Street
  Cryptid stack (decided).
- **Web reader**: wasm core, for share links and browsing.

One Rust core crate (`ursa-core`: crypto, manifests, coding, placement, protocols) under all of
them.

## Plan

### The seam: `ObjectStore`

The client never knows how bytes are kept. It talks to its home node, and the home node talks to
an `ObjectStore`:

```rust
trait ObjectStore {
    async fn put(&self, id: ObjectId, bytes: impl Stream<Item = Bytes>, tier: Tier) -> Result<Receipt>;
    async fn get(&self, id: ObjectId, range: Range<u64>) -> Result<impl Stream<Item = Bytes>>;
    async fn delete(&self, id: ObjectId) -> Result<()>;
    async fn health(&self, id: ObjectId) -> Result<Health>; // copies/shards held, last audit
}
```

Objects are already encrypted when they reach it, so every implementation sees only ciphertext.
Implementations, in the order they arrive: `LocalDisk` (the home node alone), `Replicated`
(whole copies on N nodes), `Coded` (4-of-6 shards across the pool). The same conformance test
suite runs against each, and the app does not change between them.

### Phase 0: spikes (each answers one question with a number or a decision)

| | Question | Done when |
| --- | --- | --- |
| S0 | What Ente gets right | Notes on Ente's backup edge cases (permissions, limited library, Live Photos, edits, HEIC/RAW, duplicates), timeline/album UX and key layout, written into this repo |
| S1 | Chunk size, stripe size and `k`/`n` vs throughput | FastCDC + encrypt + RS-code + hash a 4 GB video on a phone-class ARM core and a desktop; table of GB/s by stripe size and code, chunking and cipher costs, degraded reads |
| S2 | iroh bulk throughput | Parallel fetch from 6 nodes: loopback ceiling, throttled home-like uplinks, then our own nodes over the real internet; goodput by read strategy |
| S3 | PQ libraries | **Done 2026-11-23**: libcrux for X-Wing and ML-DSA-65 everywhere (builds for iOS, Android, browser); see `spikes/s3-pq` |
| S4 | PQ transport | Can iroh's rustls config take `X25519MLKEM768`? Patch size if not |
| S5 | iOS photo upload | How much a `BGProcessingTask` + background `URLSession` upload can move per night, and whether it must go over HTTP to the home node |

**S1 done (2026-11-23), except a real-phone run.** `spikes/s1-codec`, results on Apple
silicon, Linux arm64/x86-64, Windows and the laptop. The CPU is never the bottleneck: one Apple
M2 core writes 4-of-6 at ~1 GB/s and reads at 1.4 GB/s, against home uplinks of 0.02–1 Gbit/s.
Decided: aws-lc-rs AES-256-GCM (fastest on Apple silicon and x86, level with ring on Neoverse, and it ships ML-KEM; RustCrypto's ran in software on ARM until its cfg flags were turned on); 8 MiB stripes,
4-of-6, coded in 256 KiB column slices; BLAKE3 shard ids, which are the largest cost on ARM.
Details and tables: `spikes/s1-codec/README.md`.

**S2 (2026-11-23): strategies measured, real-internet run pending.** `spikes/s2-transfer`. iroh
carries 2.2–2.4 Gbit/s per connection on loopback, so the protocol is not the limit at home
speeds. Against six throttled uplinks (100/50/40/20/200/8 Mbit/s), the pull scheduler reached
197 Mbit/s (theory: 236), racing `k + 1` 148, a fixed assignment 74, the best single host 188.
Losing the fastest node mid-read: no stall; goodput drops to the
remaining nodes' ceiling within 5 s (94-121 Mbit/s against 112), whether it crashed, went silent
or slowed to a trickle. Decided: pull scheduling with rate-based hedging, at least 8 stripes in
flight, a hot cache on the home node, our own relays. Real nodes (five pods on Buttercup): an internet client got direct paths
through two NATs and read at 841 Mbit/s; n0's public relays broke both the data path and hole
punching; on the home LAN the pods were only reachable through our relay. Still owed: nodes in
several homes. Details: `spikes/s2-transfer/README.md`.

**S3 done (2026-11-23).** `spikes/s3-pq`. RustCrypto, libcrux and aws-lc-rs all interoperate
from the same seeds, and every operation is under 2 ms everywhere, so portability decided it:
aws-lc-rs cannot build for the browser, which the web reader needs. Decided: libcrux for X-Wing
and ML-DSA-65 on every platform, `ed25519-dalek` for Ed25519, aws-lc-rs kept for bulk AES-GCM on
native platforms, all behind Ursa's own `crypto` module with the interop checks as tests.

S0 and S5 shape Phase 1. S1 and S2 are not built on until Phase 3, but they run first
anyway: parallel coded reads over iroh being fast is Ursa's reason to exist, and if the
numbers are bad the design changes before anything sits on top of it. S4 can wait.

### Phase 1: the app on one home node (the vertical slice)

The goal is an app we use every day for our own camera rolls, with storage that is just the
home node (`LocalDisk`).

- Rust core: account identity, keys, chunking, per-chunk encryption, stripe packing,
  manifests, renditions (thumbnail, preview, video proxy), the library log.
- Home node: the client protocol over iroh, `LocalDisk`, thumbnail generation if the phone
  did not supply one.
- Phone app (Expo + UniFFI): camera-roll backup that survives the app being closed, the
  timeline with the replicated thumbnail tier, opening an original (cached), video playback
  by range reads, one album.
- Done when: a year of camera roll is backed up, the timeline scrolls without a blank tile, a
  4K video starts playing in under 2 s on Wi-Fi from home, and deleting the app and enrolling
  again (recovery phrase) brings it all back.

### Phase 2: devices and sharing

- Second device by QR, library log sync between devices, recovery phrase, device revocation
  with key rotation.
- Arbitrary files: the folder tree, upload/download of any file, versions and the retention
  window, compaction. Desktop sync client with on-demand placeholders; Files-app / document
  picker integration on phones.
- Albums and folders shared with other Ursa users (wrapped keys), viewer/contributor roles,
  revocation by rotation, EXIF stripping, per-recipient renditions, links + the web reader.
- Still on one node per account, which is enough to share between friends who each have one.
- Done when: two people share an album both ways, and a revoked device or person gets nothing
  added after.

### Phase 3: storage across friends

Behind `ObjectStore`, with no app changes:

- `Replicated`: whole copies on friends' nodes. Invites, per-friend quotas, fair-share
  accounting, signed receipts, leases and GC.
- `Coded`: 4-of-6 shards, placement with failure domains, pull-scheduled reads with a hot cache on the home node, audit,
  repair by the home node, draining a node.
- Done when: in a pool of our friends, unplugging any two nodes loses nothing, unplugging the
  home node of a friend loses nothing of anyone else's, and reads of originals are faster than
  from the home node alone.

### Milestone M2: identities, the pool, and losing a home node (done, 2026-11-23)

Pulled forward from Phases 2 and 3 because "what if my node dies?" decides the design. The
question it answers: **if my home node and its storage are destroyed, can I get every photo
back with nothing but my recovery key?** Today the answer is no: everything lives on one node.

**Identity** (`ursa-core::account`). The 32-byte root key is the account. Everything else is
derived from it with BLAKE3 contexts, so the root alone restores all of it:

| Derived | Use |
| --- | --- |
| account signing key (Ed25519 + ML-DSA-65, hybrid) | signs device certificates, account heads |
| account id | the Ed25519 public key; what friends and nodes know you by |
| locator key (Ed25519, separate context) | signs the account's pkarr record, unlinkable to the account id |
| X-Wing key | receives wrapped keys (shares, later) |
| dedup and manifest keys | as before |

**Recovery, offline.** A 24-word BIP-39 phrase of the root, and a key file: the root sealed
under Argon2id(passphrase) + AES-256-GCM, versioned JSON. Either one restores the account.

**Devices.** A device has its own iroh key and a certificate the account signs (account id,
device id, name, platform, time). Nodes authorise *accounts*: a device proves membership by
presenting its certificate on the connection, and the node checks the signature and that the
certificate names the connection's own endpoint id.

**Adding a device, either direction** (`ursa/link/1`). The device that has the account shows a
link code (its endpoint ticket plus a 32-byte one-time secret, as text or a QR code). The new
device dials it, proves it knows the secret, and sends its X-Wing public key. Both screens show
the same 6-digit confirmation code (a hash of the secret and the transcript); after the person
confirms, the account device seals the root key to the new device with X-Wing (post-quantum,
unlike iroh's TLS) and signs its certificate. Windows to Android and Android to Windows are
the same protocol.

**The account head.** One small signed record per account, the root of everything:
`{account id, seq, sealed payload}`. The payload (only the account can open it) lists the
account's devices, home nodes, friends and the current library index object. Nodes store heads
with compare-and-set on `seq`, so two devices updating at once cannot lose each other's change:
the loser re-reads, merges and retries. The library index becomes an object (manifest ids plus
tombstones) that the head points to.

**Where the head lives.** On every home node of the account *and* on every friend node that
holds its shards. Any one of them can hand it back.

**Finding it with no node of yours left, iroh's way.** The locator key publishes a pkarr
record (signed DNS TXT, `SignedPacket`) listing the endpoint ids of the nodes holding the head,
through iroh's pkarr relay (`dns.iroh.link`, configurable; Buttercup can run its own
iroh-dns-server) and optionally the Mainline DHT (`iroh-mainline-address-lookup`). Every home
node republishes it. From a recovery key: derive the locator, resolve, dial any listed node,
fetch the head, open it.

**The pool.** Friends invite each other at the account level (an invite carries the account id
and its node tickets). A node owner grants a friend's account a quota; the node then accepts
that account's shards and head. Nodes account bytes per account and refuse past the quota.

**Erasure coding, now.** Each sealed chunk is Reed-Solomon coded into `n` shards, any `k` of
which rebuild it (256 KiB column slices, S1), each shard on a different node of the pool (a
person's nodes count as one failure domain). With five people that is **3-of-5**: survives any
two nodes gone, 1.67x storage. The manifest records each shard's id and node. Reads use S2's
pull scheduler across whichever `k` answer first. Chunk packing into stripes comes later; for
now a small file costs `n` shards.

**Repair.** A device (or an owner's home node) walks the library, finds shards on nodes that are
gone or answer wrong, rebuilds them from `k` others onto live nodes, and rewrites the
manifests. A new home node is filled the same way.

**Products in this milestone.** The node (v2 protocol: account auth, grants and quotas,
shards, heads), the client library shared by the desktop host and a new `ursa` CLI, the Windows
app's onboarding (create, link, recover; then "add a node you own" is required before the
library opens if the account has none), device linking, friends and nodes screens. The Android
app follows in its own project, on the same core.

**The test.** Five people on Buttercup, each with their own identity and node (its own bucket),
driven by the CLI in an in-cluster Job: onboard all five, befriend, upload, add a second device
by link, then delete one person's node *and its bucket*, recover that person from the key file
alone (pkarr lookup), and check every file re-downloads byte-identical; then repair and check
the pool is back to five shards per chunk.

**Result (2026-11-23, node 0.3.0): the answer is yes.** `scripts/pool-test.py` passes on five
local nodes and on five Buttercup nodes (`apps/ursa/test` in Buttercup: one chart release per
person, one bucket prefix each, driven from a laptop on the LAN rather than an in-cluster Job).
On Buttercup: 30.6 MB uploaded in 3.1 s, a second device linked by code, alice's node taken
away, her account recovered on a fresh device from the key file alone through the pkarr
locator, every file byte-identical from the four survivors (and again after repair), 32 of 32
objects rebuilt onto her replacement node, 0 lost. What the runs taught:

- **A dead node must cost one dial, not one per chunk.** Repair touched it per object and took
  12+ minutes; a node that failed to answer now fails fast for 60 s (`DOWN_BACKOFF`), a fetch
  asks for a spare shard after 2 s with nothing arriving, and loading the head waits 3 s past
  the first answer instead of for every node. Repair now takes about a minute.
- **A file's id must survive repair.** Repair re-seals a manifest whose chunks moved; the index
  entry keeps the file's id and records the new manifest and a placement revision (newest wins a
  merge). Before, every repaired file vanished from other devices' views.
- **Finding an account needs the relays its nodes use.** A device that knows only node ids (the
  locator) reaches them through their home relays, so a client must carry the same relay list
  and token as the nodes. The apps' defaults must match the nodes they will meet.
- Still slow after a loss: a fresh process spends up to ~10 s per download learning which node
  is dead, and the dead node stays in the pool until someone removes it. Repair should offer to
  drop a node that holds nothing it can serve.

### The Android app (2026-11-23, in progress)

`apps/android`: Expo SDK 58 + UniFFI, the Street Cryptid stack. The core is crate `ursa-ffi`,
`ursa-client` behind a typed UniFFI interface on its own multi-threaded tokio runtime (chunking,
sealing and coding are CPU work and must not queue behind the network); `scripts/build-core.sh`
builds it with cargo-ndk and generates the Kotlin bindings from the same library, and the local
Expo module `modules/ursa-core` adds what only Android does (content URIs to paths, thumbnails
through `ThumbnailUtils`, EXIF dates, MediaStore saves, "open with"). The account root lives in
the Android Keystore (expo-secure-store), never in the core's directory.

It looks like Android, not like a port: Material You colours from the wallpaper
(`useMaterialColors`), Android's own navigation bar (`NativeTabs`), Compose switches and
expressive progress indicators (`@expo/ui`), the native stack header, platform ripples, system
dialogs and the system photo picker. Photos is a day-grouped grid; Files a list with filter
chips; Account holds backup, nodes and repair, friends, devices, recovery and network. The same
onboarding as Windows: create, link by code (QR both ways), recover from words or a key file,
then add a node of your own. Camera-roll backup runs while the app is open; in the background
(WorkManager) is next. `release-android.yml` builds the APK and releases `android-v*`.

### Protection you choose, and switching it (2026-11-25)

**The choice.** The account head carries an optional `protection` (k-of-n). Unset, the code is
picked from the pool's size as before (3-of-5 with five people). Set, new chunks use it whenever
enough domains are reachable; manifests, indexes and thumbnails get whole copies, at least three
and enough to survive as many nodes down as the chunks. If too few nodes are up, uploads fall
back to the automatic code and the next re-code moves them.

**What each choice lets you store** (`ursa_client::capacity`). Each domain holds at most `D/k`
of `D` bytes of files. Friends' domains hold at most what each gives you; together at most what
you give them (fair share, 1:1); pieces on your own node do not count towards it. The Pool page
shows this limit for every preset (2 copies, 1.5×, 1.67×, 3 copies, and the wider codes a bigger
pool unlocks), the hours a year a file would be out of reach at the pool's measured uptime, and
recommends the cheapest choice that is readable 99.99% of the time.

**Switching** (`Session::recode`). Re-codes every chunk, thumbnail and manifest not in the chosen
code, commits the index every 16 files, and after each commit deletes the old pieces no live file
references (counted across all manifests, so chunks shared by identical files survive). It runs
in the app, not the page, and picks up again at the next start if the app closed mid-way. A
switch the pool cannot take now (fewer reachable nodes than `n`) is refused and changes nothing.

**Room to switch.** A batch's new pieces sit beside its old ones until the batch commits, so a
switch needs one batch of extra space, not a second copy of the library. Batches stop at 16
files or 1 GB (a bigger file is a batch of its own). The storage report gives each choice's
extra space (the largest batch of the files that would move, times n/k) and whether it fits:
the library plus that batch must fit both the current choice and the new one. The Pool page
shows it as "While switching +X", a band on the chosen option's bar from what is stored to
what the switch needs, and the Switch button reads "Not enough space" when it does not fit.

**Nothing left behind, nothing lost** (`ursa_client::gc`). Unreferenced pieces come from switches
and uploads stopped part-way, deleted files, and rewritten manifests and indexes. Three layers:

- *Fixed placement*: a switch puts an object's pieces where its id says, so a re-run re-uses
  the pieces the stopped run wrote.
- *A journal* on the device of pieces written and not committed. The next switch deletes the
  ones nothing references; the app re-runs a switch at start while the journal is not empty.
- *A sweep* (`ursa gc`, and the app at start): lists every node (`list`, new in the protocol)
  and deletes this account's pieces that no live file, manifest or index references and that
  are older than a grace period (72 h, longer than any upload or switch elsewhere takes to
  commit). It refuses if any file cannot be read, and re-reads the library after listing.

Every delete is conditional on the node: `delete` takes `older_than_ms` and keeps any piece put
since. Putting a piece that already exists refreshes its time. So a sweep or a switch that read
the library before another device committed cannot delete a piece that device just wrote or
re-used. Nodes too old to list are skipped; nodes too old to understand the condition delete
unconditionally, as before.

**Uptime.** Nodes record the spans they were running (`node/uptime.json`, a heartbeat a minute)
and report the share of the last 30 days in `info`, with the disk's size and free space for
owners. Clients remember each node's last answer, so a node that is down still shows its figures.

`scripts/protection-test.py`: six local nodes; 3-of-5 → 2 copies → 4-of-6 → 3 copies → 2-of-4 →
4-of-6 cut short and resumed → 3-of-5, every file byte-identical after each, the pool holding
n/k of the files' bytes, reads with two nodes down at 4-of-6, and a refused switch.

### Phase 4: polish

Everything that makes it feel finished: on-device search (dates, places, faces, CLIP-style
text search), the map, memories, shared-album activity, storage dashboards ("Giving 2 TB ·
Using 1.4 TB"), import from Google Photos / iCloud exports.

### Observability

Same discipline as Street Cryptid from day one: OTLP spans at every decision (`shard.put`,
`shard.get{host, ms, cancelled}`, `stripe.read{k_first_ms}`, `audit`, `repair`), and a
per-node health record so a silent node is a measurable gap. Never log keys, file names or
shard-to-file mappings.

## Decided (2026-11-23)

- **Every account has an always-on home node.** It is the repair agent and the upload fan-out
  point; phones upload to it once. A user without hardware runs one on a VPS or a friend's box
  (it holds their keys, so "a friend's box" means a container they control).
- **Default code: chosen from an availability target and measured uptime**, never below
  4-of-6 (revised 2026-11-23: at 95% node uptime 4-of-6 is unreadable ~20 h a year). Pools under
  6 nodes fall back to replication. Protection levels per folder, per-friend caps, probation
  for new nodes.
- **Standalone repo** at `Z:\ursa`; the iroh/noq patches are copied in, with their NOTICEs.
- **Phone app: Expo + UniFFI**, the Street Cryptid stack.
- **Tahoe-LAFS and Ente are references, not dependencies.** We build our own storage layer and
  our own clients. No Ente + Garage pilot: it is centralized and replicated, and Ursa's reason
  to exist is coded shards, no center, and iroh.
- **Client first.** Phase 1 is a usable app on one home node; distributed storage arrives
  behind `ObjectStore` in Phase 3.

## Open decisions

1. The default availability target (99.99% suggested, about 1 hour a year) and how much
   overhead a small pool may spend to meet it before the app says "add a node" instead.
2. The fair-share ratio (1:1 suggested: place as many raw shard bytes as you host).
3. Who may join a pool: invite-only friends (suggested), or friends-of-friends with lower
   trust?
