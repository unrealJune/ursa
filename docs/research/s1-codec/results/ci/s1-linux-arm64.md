# S1 results: aarch64 · 0xd49 · 2 threads

OS `linux`, crypto features: `cfg:aes_armv8 cfg:polyval_armv8 aes pmull neon sha3`. GB/s is plaintext bytes (10^9) per second.

## Ciphers (8 MiB pieces)

| Cipher | seal 1T | open 1T | seal 2T | open 2T |
| --- | ---: | ---: | ---: | ---: |
| AES-256-GCM (ring) | 2.61 | 2.76 | 5.13 | 5.44 |
| AES-256-GCM (aws-lc-rs) | 2.66 | 2.73 | 5.23 | 5.33 |
| AES-256-GCM (RustCrypto) | 0.67 | 0.67 | 1.31 | 1.31 |
| ChaCha20-Poly1305 (ring) | 1.10 | 1.10 | 2.19 | 2.16 |
| XChaCha20-Poly1305 (RustCrypto) | 0.43 | 0.43 | 0.82 | 0.83 |

## BLAKE3 and FastCDC (one thread)

| Stage | GB/s | Note |
| --- | ---: | --- |
| BLAKE3, 8 MiB pieces | 1.41 | |
| FastCDC avg 256 KiB | 2.43 | mean chunk 312 KiB |
| FastCDC avg 1024 KiB | 2.49 | mean chunk 1219 KiB |
| FastCDC avg 4096 KiB | 2.44 | mean chunk 5349 KiB |

## RS column slicing (4-of-6, 8 MiB stripes, AES-256-GCM (ring))

The coder works on one column slice of every shard at a time, so its working set is (k + m) × slice instead of (k + m) × shard.

| Slice | write 1T | degraded read 1T | write 2T | degraded read 2T |
| --- | ---: | ---: | ---: | ---: |
| whole shard | 0.64 | 0.55 | 1.24 | 1.07 |
| 1024 KiB | 0.64 | 0.53 | 1.26 | 1.04 |
| 256 KiB | 0.65 | 0.48 | 1.27 | 0.96 |
| 64 KiB | 0.65 | 0.36 | 1.26 | 0.71 |
| 16 KiB | 0.64 | 0.18 | 1.27 | 0.35 |
| 4 KiB | 0.65 | 0.06 | 1.28 | 0.12 |

Best for degraded reads: **whole shard**, used below.

## Pipeline: stripe size × code (AES-256-GCM (ring), whole shard)

Write = encrypt + RS encode + hash every shard. Healthy read = verify k data shards + decrypt. Degraded read = verify k shards + RS decode with the first min(m, k) data shards lost + decrypt. Overhead = bytes uploaded and stored per input byte.

| Code | Overhead | Stripe | write 1T | healthy 1T | degraded 1T | write 2T | healthy 2T | degraded 2T |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 3× copies | 3.00× | 1 MiB | 0.94 | 0.97 | 0.97 | 1.78 | 1.90 | 1.90 |
| 3× copies | 3.00× | 4 MiB | 0.94 | 0.96 | 0.95 | 1.82 | 1.90 | 1.89 |
| 3× copies | 3.00× | 8 MiB | 0.94 | 0.94 | 0.94 | 1.84 | 1.88 | 1.90 |
| 3× copies | 3.00× | 16 MiB | 0.92 | 0.94 | 0.94 | 1.66 | 1.85 | 1.86 |
| 3× copies | 3.00× | 32 MiB | 0.91 | 0.90 | 0.91 | 1.61 | 1.63 | 1.60 |
| 3-of-5 | 1.67× | 1 MiB | 0.58 | 0.95 | 0.45 | 1.11 | 1.86 | 0.88 |
| 3-of-5 | 1.67× | 4 MiB | 0.59 | 0.94 | 0.49 | 1.15 | 1.86 | 0.96 |
| 3-of-5 | 1.67× | 8 MiB | 0.58 | 0.92 | 0.49 | 1.13 | 1.84 | 0.95 |
| 3-of-5 | 1.67× | 16 MiB | 0.57 | 0.93 | 0.49 | 1.08 | 1.87 | 0.87 |
| 3-of-5 | 1.67× | 32 MiB | 0.56 | 0.91 | 0.47 | 0.98 | 1.63 | 0.77 |
| 4-of-6 | 1.50× | 1 MiB | 0.65 | 0.96 | 0.50 | 1.26 | 1.90 | 0.96 |
| 4-of-6 | 1.50× | 4 MiB | 0.65 | 0.95 | 0.55 | 1.26 | 1.85 | 1.07 |
| 4-of-6 | 1.50× | 8 MiB | 0.64 | 0.94 | 0.55 | 1.27 | 1.86 | 1.04 |
| 4-of-6 | 1.50× | 16 MiB | 0.64 | 0.93 | 0.54 | 1.18 | 1.87 | 0.99 |
| 4-of-6 | 1.50× | 32 MiB | 0.61 | 0.91 | 0.51 | 1.06 | 1.63 | 0.85 |
| 6-of-9 | 1.50× | 1 MiB | 0.59 | 0.95 | 0.41 | 1.16 | 1.84 | 0.81 |
| 6-of-9 | 1.50× | 4 MiB | 0.59 | 0.95 | 0.45 | 1.16 | 1.86 | 0.86 |
| 6-of-9 | 1.50× | 8 MiB | 0.59 | 0.94 | 0.45 | 1.13 | 1.84 | 0.87 |
| 6-of-9 | 1.50× | 16 MiB | 0.58 | 0.93 | 0.44 | 1.08 | 1.82 | 0.79 |
| 6-of-9 | 1.50× | 32 MiB | 0.57 | 0.91 | 0.41 | 0.96 | 1.59 | 0.67 |
| 10-of-14 | 1.40× | 1 MiB | 0.62 | 0.94 | 0.47 | 1.21 | 1.84 | 0.92 |
| 10-of-14 | 1.40× | 4 MiB | 0.63 | 0.94 | 0.52 | 1.23 | 1.87 | 1.02 |
| 10-of-14 | 1.40× | 8 MiB | 0.63 | 0.93 | 0.51 | 1.23 | 1.84 | 1.03 |
| 10-of-14 | 1.40× | 16 MiB | 0.62 | 0.93 | 0.52 | 1.14 | 1.85 | 1.02 |
| 10-of-14 | 1.40× | 32 MiB | 0.61 | 0.93 | 0.50 | 1.10 | 1.84 | 0.89 |

## Headline: 4 GiB through 4-of-6, 8 MiB stripes, AES-256-GCM (ring), whole shard

| Threads | write GB/s | healthy read GB/s | degraded read GB/s | write time | where time goes: write / degraded read |
| ---: | ---: | ---: | ---: | ---: | --- |
| 1 | 0.64 | 0.94 | 0.54 | 6.7 s | encrypt 24% · RS encode 9% · hash 67% / verify 38% · RS decode 45% · decrypt 17% |
| 2 | 1.25 | 1.83 | 1.07 | 3.4 s | encrypt 24% · RS encode 9% · hash 66% / verify 38% · RS decode 45% · decrypt 17% |

