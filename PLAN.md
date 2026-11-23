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
| Content encryption | AES-256-GCM (ring), one key per chunk (derived from an account secret and the chunk's content) | Hardware AES on every phone and desktop. Each key encrypts exactly one plaintext, so nonces cannot repeat. S1: ring is ~3× RustCrypto's AES-GCM per core. XChaCha20-Poly1305 is the fallback if a platform lacks AES instructions. |
| Hashing / content IDs | BLAKE3 | Fast (SIMD, parallel), and **Bao** gives verified streaming: a range of a file can be checked without the whole thing. iroh-blobs is built on it. |
| Key derivation | BLAKE3 `derive_key` / HKDF-SHA256 | |
| Key wrapping across identities | HPKE (RFC 9180) with **X-Wing** (X25519 + ML-KEM-768) | Hybrid: breaks only if both break. X-Wing is the IETF combiner, small and audited implementations exist (libcrux). |
| Signatures (manifests, device certs, quota receipts) | Hybrid **Ed25519 + ML-DSA-65**, both must verify | Forgery needs a quantum computer *at signing-verification time*, which is not a harvest-later threat, so this is lower priority than the KEM; still cheap enough to do from day one and painful to retrofit. |
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

Default `k`/`n`: **4-of-6** (decided): 1.5× storage overhead (vs 3× for
triple replication), survives any 2 nodes gone, reads from 4 hosts in parallel. A pool of
fewer than 6 nodes falls back to replication. Make it a per-library (or per-folder) setting:
"Originals 4-of-6, thumbnails replicated to everyone."

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

## Speed

- **Reads**: request from `k + 1` (or more) hosts, use the first `k` shards to arrive per
  stripe, cancel the rest. Pipeline several stripes ahead for video. Throughput then scales
  with the number of hosts instead of the slowest one.
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
| S2 | iroh bulk throughput | Parallel fetch from 6 nodes over WAN (two homes + a VPS), long-tail cancellation on; MB/s vs a single HTTPS download |
| S3 | PQ libraries | X-Wing HPKE and ML-DSA-65 crates chosen, building on iOS, Android, wasm; wrap/unwrap timings |
| S4 | PQ transport | Can iroh's rustls config take `X25519MLKEM768`? Patch size if not |
| S5 | iOS photo upload | How much a `BGProcessingTask` + background `URLSession` upload can move per night, and whether it must go over HTTP to the home node |

**S1 done (2026-11-23), except a real-phone run.** `spikes/s1-codec`, results on Apple
silicon, Linux arm64/x86-64, Windows and the laptop. The CPU is never the bottleneck: one Apple
M2 core writes 4-of-6 at ~1 GB/s and reads at 1.4 GB/s, against home uplinks of 0.02–1 Gbit/s.
Decided: ring AES-256-GCM (RustCrypto's runs in software on ARM, 35× slower); 8 MiB stripes,
4-of-6, coded in 256 KiB column slices; BLAKE3 shard ids, which are the largest cost on ARM.
Details and tables: `spikes/s1-codec/README.md`.

S0, S3 and S5 shape Phase 1. S1 and S2 are not built on until Phase 3, but they run first
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
- `Coded`: 4-of-6 shards, placement with failure domains, long-tail-cancelled reads, audit,
  repair by the home node, draining a node.
- Done when: in a pool of our friends, unplugging any two nodes loses nothing, unplugging the
  home node of a friend loses nothing of anyone else's, and reads of originals are faster than
  from the home node alone.

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
- **Default code for originals: 4-of-6.** Pools under 6 nodes fall back to replication.
- **Standalone repo** at `Z:\ursa`; the iroh/noq patches are copied in, with their NOTICEs.
- **Phone app: Expo + UniFFI**, the Street Cryptid stack.
- **Tahoe-LAFS and Ente are references, not dependencies.** We build our own storage layer and
  our own clients. No Ente + Garage pilot: it is centralized and replicated, and Ursa's reason
  to exist is coded shards, no center, and iroh.
- **Client first.** Phase 1 is a usable app on one home node; distributed storage arrives
  behind `ObjectStore` in Phase 3.

## Open decisions

1. The fair-share ratio (1:1 suggested: place as many raw shard bytes as you host).
2. Who may join a pool: invite-only friends (suggested), or friends-of-friends with lower
   trust?
