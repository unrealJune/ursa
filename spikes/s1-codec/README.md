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

| Machine | File |
| --- | --- |
| Ryzen 9 4900HS laptop, **on battery (Balanced)**, quick run | [results/ryzen9-4900hs-battery-quick.md](results/ryzen9-4900hs-battery-quick.md) |
| CI: Linux x86-64, Linux arm64, macOS arm64, Windows | pending (needs the GitHub repo) |
| A real phone (Android arm64) | pending |

## Findings so far (laptop on battery; ratios hold, absolute numbers are low)

1. **The CPU is not the bottleneck on a desktop.** Through 4-of-6 with 8 MiB stripes: write
   0.5 GB/s on one core and 2.0 GB/s on all of them; healthy read 0.7 and 3.6 GB/s; degraded
   read 0.23 and 1.4 GB/s. One core writes at 4 Gbit/s, well past any home uplink. Whether
   that holds on a phone is the open question.
2. **Use ring's AES-256-GCM.** 1.4 GB/s per core against 0.5 for RustCrypto's AES-GCM and 0.6
   for XChaCha20-Poly1305. (aws-lc-rs is likely faster still and worth one row later; it needs
   CMake/NASM on Windows.)
3. **Code in column slices, never whole shards.** Columns of an RS code are independent, so
   the coder can work on a 64 KiB slice of every shard at a time. Whole-shard decoding fell out
   of cache and stopped scaling (0.58 GB/s at 16 threads); 64 KiB slices reach 1.5 GB/s. The
   same property is what lets a degraded read decode only the range it needs.
4. **`reed-solomon-simd` has a fixed cost per decode call** (~0.5–0.8 ms here, independent of
   size, from its error-locator transform over GF(2¹⁶)). Small slices pay it many times: 4 KiB
   slices decode at 0.03 GB/s on one core. For codes this small (n ≤ 14) a GF(2⁸)
   matrix code with SIMD multiply tables (the ISA-L / klauspost approach) computes the decode
   matrix once per loss pattern and reuses it for every slice and stripe; it should be several
   times faster on degraded reads. Worth a row before committing to a library. Encoding has no
   such problem (13% of write time).
5. **Hashing is the biggest write cost** (~half of single-core write time, BLAKE3 at 1.7 GB/s
   on this throttled core). It is also what names every shard and what Bao verification will
   use, so it stays; it parallelizes across shards for free.
6. **Stripe size barely matters between 1 and 16 MiB**; 32 MiB is a little worse. The plan's
   8 MiB stays. The choice can be made on network grounds (request count, tail latency) in S2.
7. **The code barely matters for CPU.** 3-of-5, 4-of-6, 6-of-9 and 10-of-14 are within noise
   of each other on writes and healthy reads. The choice is about overhead, fault tolerance and
   how many hosts a pool has, not about speed.
8. **FastCDC runs at ~1.2 GB/s per core** at every average size tried (256 KiB – 4 MiB). One file
   is chunked on one core, which is still well above network speed. Media files never change
   after capture, so they could skip CDC and use fixed chunks; documents and project trees are
   where it pays.
