# Spike S1: chunking, encryption, erasure coding, hashing

**Question:** how fast can a device turn plaintext into placed shards and back, and which stripe
size, code, cipher and chunking should Ursa use?

```sh
cargo run --release -- [--quick] [--json out.json]
```

CI runs it on Linux x86-64, Linux arm64, macOS (Apple silicon, the nearest thing to an iPhone)
and Windows, and builds an `aarch64-linux-android` binary for a real phone
(`.github/workflows/s1-codec.yml`). To run it on a phone:

```sh
adb push s1-codec /data/local/tmp/ && adb shell 'cd /data/local/tmp && chmod +x s1-codec && ./s1-codec --quick'
```

## What it measures

- **Write** = AES-256-GCM seal → Reed–Solomon encode (`reed-solomon-simd`) → BLAKE3 every shard.
- **Healthy read** = verify the `k` data shards → concatenate → open. The normal case.
- **Degraded read** = the first `min(m, k)` data shards are lost: verify `k` survivors → RS
  decode → open. The worst case.
- Ciphers compared alone, BLAKE3 and FastCDC alone, a stripe-size × code matrix, a column-slice
  sweep, and a 4 GiB "video" through 4-of-6 at 1, 2, 4 and all threads.

Data is random (media is incompressible). Stripes are encrypted as one unit; the design encrypts
per chunk, which costs the same per byte plus a 16-byte tag per chunk.

## Results

| Machine | Cores | File |
| --- | ---: | --- |
| Apple M2 Pro VM (macOS CI): the stand-in for an iPhone's cores | 5 | [results/ci/s1-macos-arm64.md](results/ci/s1-macos-arm64.md) |
| Azure Cobalt 100 / Neoverse N2 (Linux arm64 CI) | 2 | [results/ci/s1-linux-arm64.md](results/ci/s1-linux-arm64.md) |
| Xeon Platinum 8573C (Linux x86-64 CI) | 2 | [results/ci/s1-linux-x86_64.md](results/ci/s1-linux-x86_64.md) |
| Intel, Windows CI | 2 | [results/ci/s1-windows-x86_64.md](results/ci/s1-windows-x86_64.md) |
| Ryzen 9 4900HS laptop, **on battery (Balanced)**, quick run | 16 | [results/ryzen9-4900hs-battery-quick.md](results/ryzen9-4900hs-battery-quick.md) |
| A real phone (Android arm64) | | pending: CI builds the binary (`s1-codec-android-arm64` artifact) |

4 GiB through 4-of-6, 8 MiB stripes, ring AES-256-GCM, GB/s of plaintext:

| Machine | write 1 core | healthy read 1 core | degraded read 1 core | write all cores | healthy read all | degraded read all |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Apple M2 Pro | 0.96 | 1.41 | 0.81 | 3.70 (5) | 5.79 (5) | 3.78 (5) |
| Neoverse N2 | 0.64 | 0.93 | 0.54 | 1.26 (2) | 1.85 (2) | 1.08 (2) |
| Xeon 8573C | 1.65 | 2.44 | 0.87 | 1.75 (2) | 2.68 (2) | 0.90 (2) |
| Ryzen laptop, battery | 0.51 | 0.71 | 0.23 | 2.00 (16) | 3.58 (16) | 1.44 (16) |

AES-256-GCM, one core, seal GB/s:

| Machine | ring | RustCrypto `aes-gcm` | ring ChaCha20-Poly1305 | RustCrypto XChaCha20-Poly1305 |
| --- | ---: | ---: | ---: | ---: |
| Apple M2 Pro | **5.25** | 0.15 | 1.60 | 0.44 |
| Neoverse N2 | **2.61** | 0.14 | 1.09 | 0.43 |
| Xeon 8573C | **4.02** | 1.14 | 1.64 | 1.10 |
| Ryzen laptop, battery | **1.42** | 0.51 | 0.97 | 0.61 |

## Findings

1. **The CPU is not the bottleneck, ARM included.** One Apple M2 core (the same family of
   core as a recent iPhone's performance cores) writes 4-of-6 at 0.96 GB/s (7.7 Gbit/s), reads
   at 1.4 GB/s, and reads with two shards lost at 0.8 GB/s; five cores give 3.7 / 5.8 / 3.8.
   Home uplinks are 0.02–1 Gbit/s. A phone will spend its battery on the radio, not on this.
   A real phone run is still owed (the Android binary is built).
2. **ring's AES-256-GCM, decisively.** RustCrypto's `aes-gcm` runs in **software on ARM**
   (0.15 GB/s: its ARMv8 backend is opt-in behind a `cfg` flag), 35× slower than ring's 5.3
   GB/s on the same core. On x86 ring is still 3.5× faster. ring's ChaCha20 is the fallback
   for a device without AES instructions. (aws-lc-rs may be faster again; one row later.)
3. **On ARM, BLAKE3 is the write cost**: 67–76% of single-core write time and ~40% of a read
   (1.4–1.8 GB/s per ARM core vs 5 on x86). It names every shard and backs verification, so it
   stays; it already runs per shard in parallel. If writes ever need to be cheaper on phones,
   this is the stage to look at, not the cipher or the code.
4. **Code in column slices; 256 KiB is the default.** Whole-shard decoding fell out of cache on
   the laptop (0.58 GB/s at 16 threads vs 1.47 sliced). 256 KiB was best on the Apple, Xeon and
   Windows runners, 64 KiB on the laptop, whole shards on Neoverse N2 (large caches). Make it
   a tuned constant, not a format decision: slicing never changes the bytes on disk.
5. **Degraded reads cost 1.7–3× a healthy read.** `reed-solomon-simd` pays a fixed cost per
   decode call (its GF(2¹⁶) error-locator transform), which is why small slices hurt. It is
   acceptable: degraded reads are rare and still exceed 0.5 GB/s per core everywhere. A GF(2⁸)
   matrix code with the decode matrix cached per loss pattern remains a later optimization,
   not a blocker.
6. **Stripe size: 8 MiB stays.** 1–16 MiB are flat on every machine; 32 MiB is slower
   everywhere. The final choice can rest on network behaviour (S2).
7. **The code is a fault-tolerance choice, not a speed one.** 3-of-5 through 10-of-14 are within
   noise of each other on writes and healthy reads.
8. **FastCDC runs at 1.9–2.6 GB/s per core** on CI machines (1.2 on the throttled laptop),
   at every average chunk size tried (256 KiB – 4 MiB). Chunking is never the bottleneck.

## Decisions

- Cipher: ring AES-256-GCM; ring ChaCha20-Poly1305 only where AES instructions are missing.
  Never RustCrypto `aes-gcm` on ARM.
- Stripes: 8 MiB, 4-of-6, coded and decoded in 256 KiB column slices (tunable).
- Shard ids: BLAKE3, kept despite being the main ARM cost.
- Reed–Solomon: `reed-solomon-simd` for now; revisit with a cached-matrix GF(2⁸) code if
  degraded reads matter in practice.
