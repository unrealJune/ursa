# S1 results: aarch64 · Apple M2 Pro (Virtual) · 5 threads

OS `macos`, crypto features: `cfg:aes_armv8 cfg:polyval_armv8 aes pmull neon sha3`. GB/s is plaintext bytes (10^9) per second.

## Ciphers (8 MiB pieces)

| Cipher | seal 1T | open 1T | seal 5T | open 5T |
| --- | ---: | ---: | ---: | ---: |
| AES-256-GCM (ring) | 4.81 | 4.92 | 13.07 | 19.88 |
| AES-256-GCM (aws-lc-rs) | 6.44 | 7.47 | 12.45 | 24.23 |
| AES-256-GCM (RustCrypto) | 2.01 | 1.93 | 6.25 | 8.27 |
| ChaCha20-Poly1305 (ring) | 1.65 | 1.71 | 6.56 | 7.24 |
| XChaCha20-Poly1305 (RustCrypto) | 0.43 | 0.41 | 1.92 | 2.06 |

## BLAKE3 and FastCDC (one thread)

| Stage | GB/s | Note |
| --- | ---: | --- |
| BLAKE3, 8 MiB pieces | 1.76 | |
| FastCDC avg 256 KiB | 2.44 | mean chunk 312 KiB |
| FastCDC avg 1024 KiB | 2.59 | mean chunk 1219 KiB |
| FastCDC avg 4096 KiB | 2.73 | mean chunk 5349 KiB |

## RS column slicing (4-of-6, 8 MiB stripes, AES-256-GCM (ring))

The coder works on one column slice of every shard at a time, so its working set is (k + m) × slice instead of (k + m) × shard.

| Slice | write 1T | degraded read 1T | write 5T | degraded read 5T |
| --- | ---: | ---: | ---: | ---: |
| whole shard | 0.98 | 0.91 | 3.38 | 3.21 |
| 1024 KiB | 0.96 | 0.90 | 3.62 | 3.73 |
| 256 KiB | 1.02 | 0.84 | 3.78 | 3.79 |
| 64 KiB | 1.01 | 0.60 | 3.76 | 2.86 |
| 16 KiB | 0.99 | 0.28 | 2.76 | 1.01 |
| 4 KiB | 0.92 | 0.09 | 3.64 | 0.46 |

Best for degraded reads: **1024 KiB slices**, used below.

## Pipeline: stripe size × code (AES-256-GCM (ring), 1024 KiB slices)

Write = encrypt + RS encode + hash every shard. Healthy read = verify k data shards + decrypt. Degraded read = verify k shards + RS decode with the first min(m, k) data shards lost + decrypt. Overhead = bytes uploaded and stored per input byte.

| Code | Overhead | Stripe | write 1T | healthy 1T | degraded 1T | write 5T | healthy 5T | degraded 5T |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 3× copies | 3.00× | 1 MiB | 1.35 | 1.40 | 1.41 | 6.82 | 7.16 | 5.78 |
| 3× copies | 3.00× | 4 MiB | 1.44 | 1.49 | 1.39 | 5.38 | 6.56 | 6.60 |
| 3× copies | 3.00× | 8 MiB | 1.45 | 1.35 | 1.36 | 4.98 | 5.83 | 6.00 |
| 3× copies | 3.00× | 16 MiB | 1.28 | 1.30 | 1.37 | 4.51 | 5.97 | 5.80 |
| 3× copies | 3.00× | 32 MiB | 1.35 | 1.42 | 1.45 | 4.16 | 5.58 | 5.50 |
| 3-of-5 | 1.67× | 1 MiB | 0.90 | 1.47 | 0.79 | 4.20 | 7.03 | 2.94 |
| 3-of-5 | 1.67× | 4 MiB | 0.84 | 1.51 | 0.78 | 3.70 | 6.72 | 3.38 |
| 3-of-5 | 1.67× | 8 MiB | 0.87 | 1.48 | 0.83 | 3.32 | 6.74 | 3.44 |
| 3-of-5 | 1.67× | 16 MiB | 0.90 | 1.44 | 0.82 | 2.87 | 5.96 | 3.31 |
| 3-of-5 | 1.67× | 32 MiB | 0.89 | 1.39 | 0.80 | 2.74 | 5.69 | 3.19 |
| 4-of-6 | 1.50× | 1 MiB | 1.01 | 1.48 | 0.88 | 4.77 | 7.20 | 4.03 |
| 4-of-6 | 1.50× | 4 MiB | 0.99 | 1.41 | 0.80 | 3.76 | 5.66 | 2.70 |
| 4-of-6 | 1.50× | 8 MiB | 0.98 | 1.45 | 0.89 | 3.54 | 6.21 | 3.57 |
| 4-of-6 | 1.50× | 16 MiB | 0.99 | 1.48 | 0.89 | 3.26 | 5.33 | 3.55 |
| 4-of-6 | 1.50× | 32 MiB | 0.94 | 1.32 | 0.92 | 2.87 | 4.41 | 3.48 |
| 6-of-9 | 1.50× | 1 MiB | 0.94 | 1.48 | 0.73 | 4.33 | 6.98 | 3.31 |
| 6-of-9 | 1.50× | 4 MiB | 0.96 | 1.50 | 0.77 | 1.72 | 6.56 | 2.93 |
| 6-of-9 | 1.50× | 8 MiB | 0.89 | 1.51 | 0.74 | 3.21 | 6.64 | 2.91 |
| 6-of-9 | 1.50× | 16 MiB | 0.94 | 1.39 | 0.71 | 2.32 | 5.00 | 1.96 |
| 6-of-9 | 1.50× | 32 MiB | 0.87 | 1.44 | 0.67 | 2.19 | 4.87 | 2.22 |
| 10-of-14 | 1.40× | 1 MiB | 0.90 | 1.43 | 0.75 | 3.79 | 6.05 | 3.62 |
| 10-of-14 | 1.40× | 4 MiB | 0.90 | 1.28 | 0.76 | 3.38 | 5.33 | 3.59 |
| 10-of-14 | 1.40× | 8 MiB | 0.88 | 1.41 | 0.81 | 2.89 | 6.11 | 3.25 |
| 10-of-14 | 1.40× | 16 MiB | 0.90 | 1.33 | 0.78 | 2.26 | 5.32 | 2.91 |
| 10-of-14 | 1.40× | 32 MiB | 0.87 | 1.35 | 0.81 | 2.51 | 4.98 | 2.98 |

## Headline: 4 GiB through 4-of-6, 8 MiB stripes, AES-256-GCM (ring), 1024 KiB slices

| Threads | write GB/s | healthy read GB/s | degraded read GB/s | write time | where time goes: write / degraded read |
| ---: | ---: | ---: | ---: | ---: | --- |
| 1 | 0.94 | 1.36 | 0.82 | 4.6 s | encrypt 17% · RS encode 8% · hash 75% / verify 45% · RS decode 42% · decrypt 13% |
| 2 | 1.76 | 2.66 | 1.64 | 2.4 s | encrypt 18% · RS encode 8% · hash 74% / verify 45% · RS decode 42% · decrypt 13% |
| 4 | 3.31 | 4.98 | 2.41 | 1.3 s | encrypt 19% · RS encode 9% · hash 72% / verify 42% · RS decode 45% · decrypt 13% |
| 5 | 3.47 | 5.78 | 3.28 | 1.2 s | encrypt 22% · RS encode 10% · hash 67% / verify 42% · RS decode 46% · decrypt 12% |

