# S1 results: x86_64 · AMD64 Family 25 Model 17 Stepping 1, AuthenticAMD · 2 threads

OS `windows`, crypto features: `aes pclmulqdq avx2 vaes`. GB/s is plaintext bytes (10^9) per second.

## Ciphers (8 MiB pieces)

| Cipher | seal 1T | open 1T | seal 2T | open 2T |
| --- | ---: | ---: | ---: | ---: |
| AES-256-GCM (ring) | 4.24 | 5.53 | 4.29 | 5.83 |
| AES-256-GCM (RustCrypto) | 1.02 | 1.03 | 1.36 | 1.46 |
| ChaCha20-Poly1305 (ring) | 1.82 | 1.83 | 2.03 | 2.17 |
| XChaCha20-Poly1305 (RustCrypto) | 1.13 | 1.14 | 1.49 | 1.48 |

## BLAKE3 and FastCDC (one thread)

| Stage | GB/s | Note |
| --- | ---: | --- |
| BLAKE3, 8 MiB pieces | 2.97 | |
| FastCDC avg 256 KiB | 2.16 | mean chunk 312 KiB |
| FastCDC avg 1024 KiB | 2.23 | mean chunk 1219 KiB |
| FastCDC avg 4096 KiB | 2.21 | mean chunk 5349 KiB |

## RS column slicing (4-of-6, 8 MiB stripes, AES-256-GCM (ring))

The coder works on one column slice of every shard at a time, so its working set is (k + m) × slice instead of (k + m) × shard.

| Slice | write 1T | degraded read 1T | write 2T | degraded read 2T |
| --- | ---: | ---: | ---: | ---: |
| whole shard | 1.23 | 0.98 | 1.39 | 1.14 |
| 1024 KiB | 1.27 | 0.95 | 1.50 | 1.08 |
| 256 KiB | 1.27 | 0.85 | 1.52 | 1.05 |
| 64 KiB | 1.31 | 0.57 | 1.53 | 0.66 |
| 16 KiB | 1.29 | 0.24 | 1.51 | 0.27 |
| 4 KiB | 1.31 | 0.07 | 1.52 | 0.08 |

Best for degraded reads: **whole shard**, used below.

## Pipeline: stripe size × code (AES-256-GCM (ring), whole shard)

Write = encrypt + RS encode + hash every shard. Healthy read = verify k data shards + decrypt. Degraded read = verify k shards + RS decode with the first min(m, k) data shards lost + decrypt. Overhead = bytes uploaded and stored per input byte.

| Code | Overhead | Stripe | write 1T | healthy 1T | degraded 1T | write 2T | healthy 2T | degraded 2T |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 3× copies | 3.00× | 1 MiB | 2.00 | 2.12 | 2.11 | 2.52 | 2.39 | 2.76 |
| 3× copies | 3.00× | 4 MiB | 1.96 | 1.95 | 1.95 | 2.43 | 2.36 | 2.38 |
| 3× copies | 3.00× | 8 MiB | 1.94 | 1.85 | 1.87 | 2.17 | 2.27 | 2.27 |
| 3× copies | 3.00× | 16 MiB | 1.83 | 1.79 | 1.76 | 1.92 | 2.07 | 2.07 |
| 3× copies | 3.00× | 32 MiB | 1.70 | 1.79 | 1.78 | 1.72 | 1.87 | 1.87 |
| 3-of-5 | 1.67× | 1 MiB | 1.21 | 2.03 | 0.84 | 1.59 | 2.47 | 0.99 |
| 3-of-5 | 1.67× | 4 MiB | 1.19 | 1.90 | 0.90 | 1.45 | 2.36 | 1.04 |
| 3-of-5 | 1.67× | 8 MiB | 1.07 | 1.80 | 0.84 | 1.30 | 2.22 | 0.97 |
| 3-of-5 | 1.67× | 16 MiB | 1.04 | 1.76 | 0.73 | 1.03 | 1.44 | 0.59 |
| 3-of-5 | 1.67× | 32 MiB | 0.83 | 1.67 | 0.63 | 0.87 | 1.90 | 0.72 |
| 4-of-6 | 1.50× | 1 MiB | 1.22 | 1.60 | 0.66 | 1.01 | 0.99 | 0.81 |
| 4-of-6 | 1.50× | 4 MiB | 1.17 | 1.71 | 0.92 | 1.56 | 2.03 | 0.96 |
| 4-of-6 | 1.50× | 8 MiB | 1.21 | 1.68 | 0.92 | 1.32 | 1.85 | 1.08 |
| 4-of-6 | 1.50× | 16 MiB | 1.13 | 1.70 | 0.90 | 1.22 | 2.16 | 1.01 |
| 4-of-6 | 1.50× | 32 MiB | 1.08 | 1.80 | 0.83 | 1.16 | 2.15 | 0.93 |
| 6-of-9 | 1.50× | 1 MiB | 1.21 | 1.96 | 0.74 | 1.54 | 2.45 | 0.92 |
| 6-of-9 | 1.50× | 4 MiB | 1.21 | 1.88 | 0.83 | 1.37 | 2.37 | 0.93 |
| 6-of-9 | 1.50× | 8 MiB | 1.16 | 1.83 | 0.79 | 1.28 | 2.26 | 0.86 |
| 6-of-9 | 1.50× | 16 MiB | 1.02 | 1.75 | 0.68 | 1.17 | 2.08 | 0.79 |
| 6-of-9 | 1.50× | 32 MiB | 0.98 | 1.79 | 0.65 | 1.03 | 2.16 | 0.72 |
| 10-of-14 | 1.40× | 1 MiB | 1.30 | 1.97 | 0.89 | 1.64 | 2.44 | 1.06 |
| 10-of-14 | 1.40× | 4 MiB | 1.28 | 1.90 | 0.98 | 1.57 | 2.36 | 1.15 |
| 10-of-14 | 1.40× | 8 MiB | 1.25 | 1.82 | 0.97 | 1.41 | 2.27 | 1.12 |
| 10-of-14 | 1.40× | 16 MiB | 1.13 | 1.77 | 0.93 | 1.22 | 2.15 | 1.04 |
| 10-of-14 | 1.40× | 32 MiB | 1.04 | 1.79 | 0.84 | 1.08 | 2.15 | 0.96 |

## Headline: 4 GiB through 4-of-6, 8 MiB stripes, AES-256-GCM (ring), whole shard

| Threads | write GB/s | healthy read GB/s | degraded read GB/s | write time | where time goes: write / degraded read |
| ---: | ---: | ---: | ---: | ---: | --- |
| 1 | 1.27 | 1.81 | 0.99 | 3.4 s | encrypt 25% · RS encode 13% · hash 61% / verify 35% · RS decode 51% · decrypt 14% |
| 2 | 1.55 | 2.33 | 1.17 | 2.8 s | encrypt 29% · RS encode 13% · hash 59% / verify 30% · RS decode 54% · decrypt 16% |

