# S1 results: aarch64 · 0xd49 · 2 threads

OS `linux`, crypto features: `aes pmull neon sha3`. GB/s is plaintext bytes (10^9) per second.

## Ciphers (8 MiB pieces)

| Cipher | seal 1T | open 1T | seal 2T | open 2T |
| --- | ---: | ---: | ---: | ---: |
| AES-256-GCM (ring) | 2.61 | 2.76 | 5.23 | 5.42 |
| AES-256-GCM (RustCrypto) | 0.14 | 0.14 | 0.28 | 0.28 |
| ChaCha20-Poly1305 (ring) | 1.09 | 1.10 | 2.13 | 2.17 |
| XChaCha20-Poly1305 (RustCrypto) | 0.43 | 0.43 | 0.84 | 0.84 |

## BLAKE3 and FastCDC (one thread)

| Stage | GB/s | Note |
| --- | ---: | --- |
| BLAKE3, 8 MiB pieces | 1.40 | |
| FastCDC avg 256 KiB | 2.45 | mean chunk 312 KiB |
| FastCDC avg 1024 KiB | 2.47 | mean chunk 1219 KiB |
| FastCDC avg 4096 KiB | 2.41 | mean chunk 5349 KiB |

## RS column slicing (4-of-6, 8 MiB stripes, AES-256-GCM (ring))

The coder works on one column slice of every shard at a time, so its working set is (k + m) × slice instead of (k + m) × shard.

| Slice | write 1T | degraded read 1T | write 2T | degraded read 2T |
| --- | ---: | ---: | ---: | ---: |
| whole shard | 0.64 | 0.54 | 1.24 | 1.06 |
| 1024 KiB | 0.64 | 0.52 | 1.26 | 1.04 |
| 256 KiB | 0.65 | 0.48 | 1.25 | 0.95 |
| 64 KiB | 0.65 | 0.36 | 1.26 | 0.70 |
| 16 KiB | 0.64 | 0.18 | 1.27 | 0.35 |
| 4 KiB | 0.65 | 0.06 | 1.27 | 0.12 |

Best for degraded reads: **whole shard**, used below.

## Pipeline: stripe size × code (AES-256-GCM (ring), whole shard)

Write = encrypt + RS encode + hash every shard. Healthy read = verify k data shards + decrypt. Degraded read = verify k shards + RS decode with the first min(m, k) data shards lost + decrypt. Overhead = bytes uploaded and stored per input byte.

| Code | Overhead | Stripe | write 1T | healthy 1T | degraded 1T | write 2T | healthy 2T | degraded 2T |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 3× copies | 3.00× | 1 MiB | 0.93 | 0.97 | 0.97 | 1.84 | 1.87 | 1.84 |
| 3× copies | 3.00× | 4 MiB | 0.94 | 0.97 | 0.96 | 1.86 | 1.87 | 1.90 |
| 3× copies | 3.00× | 8 MiB | 0.93 | 0.95 | 0.95 | 1.83 | 1.89 | 1.86 |
| 3× copies | 3.00× | 16 MiB | 0.92 | 0.93 | 0.94 | 1.71 | 1.85 | 1.83 |
| 3× copies | 3.00× | 32 MiB | 0.91 | 0.91 | 0.91 | 1.61 | 1.62 | 1.62 |
| 3-of-5 | 1.67× | 1 MiB | 0.58 | 0.95 | 0.45 | 1.15 | 1.84 | 0.88 |
| 3-of-5 | 1.67× | 4 MiB | 0.58 | 0.94 | 0.49 | 1.15 | 1.84 | 0.96 |
| 3-of-5 | 1.67× | 8 MiB | 0.58 | 0.93 | 0.49 | 1.13 | 1.82 | 0.97 |
| 3-of-5 | 1.67× | 16 MiB | 0.57 | 0.92 | 0.48 | 1.07 | 1.81 | 0.84 |
| 3-of-5 | 1.67× | 32 MiB | 0.55 | 0.91 | 0.44 | 0.93 | 1.62 | 0.69 |
| 4-of-6 | 1.50× | 1 MiB | 0.64 | 0.96 | 0.50 | 1.26 | 1.89 | 0.98 |
| 4-of-6 | 1.50× | 4 MiB | 0.65 | 0.94 | 0.54 | 1.26 | 1.81 | 1.05 |
| 4-of-6 | 1.50× | 8 MiB | 0.64 | 0.92 | 0.54 | 1.24 | 1.85 | 1.06 |
| 4-of-6 | 1.50× | 16 MiB | 0.63 | 0.92 | 0.53 | 1.14 | 1.84 | 0.96 |
| 4-of-6 | 1.50× | 32 MiB | 0.60 | 0.90 | 0.50 | 1.03 | 1.58 | 0.83 |
| 6-of-9 | 1.50× | 1 MiB | 0.58 | 0.95 | 0.41 | 1.14 | 1.87 | 0.80 |
| 6-of-9 | 1.50× | 4 MiB | 0.59 | 0.94 | 0.44 | 1.15 | 1.83 | 0.87 |
| 6-of-9 | 1.50× | 8 MiB | 0.59 | 0.92 | 0.44 | 1.15 | 1.86 | 0.87 |
| 6-of-9 | 1.50× | 16 MiB | 0.57 | 0.92 | 0.43 | 1.09 | 1.85 | 0.77 |
| 6-of-9 | 1.50× | 32 MiB | 0.56 | 0.90 | 0.41 | 0.98 | 1.61 | 0.67 |
| 10-of-14 | 1.40× | 1 MiB | 0.62 | 0.94 | 0.47 | 1.21 | 1.85 | 0.92 |
| 10-of-14 | 1.40× | 4 MiB | 0.63 | 0.94 | 0.51 | 1.22 | 1.84 | 1.00 |
| 10-of-14 | 1.40× | 8 MiB | 0.63 | 0.93 | 0.51 | 1.22 | 1.81 | 1.01 |
| 10-of-14 | 1.40× | 16 MiB | 0.62 | 0.92 | 0.51 | 1.14 | 1.80 | 1.00 |
| 10-of-14 | 1.40× | 32 MiB | 0.60 | 0.92 | 0.50 | 1.06 | 1.83 | 0.88 |

## Headline: 4 GiB through 4-of-6, 8 MiB stripes, AES-256-GCM (ring), whole shard

| Threads | write GB/s | healthy read GB/s | degraded read GB/s | write time | where time goes: write / degraded read |
| ---: | ---: | ---: | ---: | ---: | --- |
| 1 | 0.64 | 0.93 | 0.54 | 6.7 s | encrypt 24% · RS encode 9% · hash 67% / verify 38% · RS decode 45% · decrypt 17% |
| 2 | 1.26 | 1.85 | 1.08 | 3.4 s | encrypt 24% · RS encode 9% · hash 67% / verify 38% · RS decode 45% · decrypt 17% |

