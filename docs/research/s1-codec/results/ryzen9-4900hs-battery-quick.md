# S1 results: x86_64 · AMD64 Family 23 Model 96 Stepping 1, AuthenticAMD · 16 threads

OS `windows`, crypto features: `aes pclmulqdq avx2`. GB/s is plaintext bytes (10^9) per second.

**Quick run** (smaller totals; noisier).

## Ciphers (8 MiB pieces)

| Cipher | seal 1T | open 1T | seal 16T | open 16T |
| --- | ---: | ---: | ---: | ---: |
| AES-256-GCM (ring) | 1.40 | 1.36 | 3.24 | 5.36 |
| AES-256-GCM (RustCrypto) | 0.53 | 0.50 | 2.21 | 3.07 |
| ChaCha20-Poly1305 (ring) | 0.98 | 0.90 | 2.79 | 4.05 |
| XChaCha20-Poly1305 (RustCrypto) | 0.62 | 0.61 | 1.98 | 3.52 |

## BLAKE3 and FastCDC (one thread)

| Stage | GB/s | Note |
| --- | ---: | --- |
| BLAKE3, 8 MiB pieces | 1.76 | |
| FastCDC avg 256 KiB | 1.17 | mean chunk 312 KiB |
| FastCDC avg 1024 KiB | 1.26 | mean chunk 1219 KiB |
| FastCDC avg 4096 KiB | 1.19 | mean chunk 5349 KiB |

## RS column slicing (4-of-6, 8 MiB stripes, AES-256-GCM (ring))

The coder works on one column slice of every shard at a time, so its working set is (k + m) × slice instead of (k + m) × shard.

| Slice | write 1T | degraded read 1T | write 16T | degraded read 16T |
| --- | ---: | ---: | ---: | ---: |
| whole shard | 0.50 | 0.31 | 1.29 | 0.58 |
| 1024 KiB | 0.48 | 0.29 | 1.32 | 0.60 |
| 256 KiB | 0.55 | 0.35 | 1.71 | 0.82 |
| 64 KiB | 0.50 | 0.23 | 1.57 | 1.47 |
| 16 KiB | 0.54 | 0.10 | 1.51 | 0.70 |
| 4 KiB | 0.52 | 0.03 | 1.74 | 0.23 |

Best for degraded reads: **64 KiB slices**, used below.

## Pipeline: stripe size × code (AES-256-GCM (ring), 64 KiB slices)

Write = encrypt + RS encode + hash every shard. Healthy read = verify k data shards + decrypt. Degraded read = verify k shards + RS decode with the first min(m, k) data shards lost + decrypt. Overhead = bytes uploaded and stored per input byte.

| Code | Overhead | Stripe | write 1T | healthy 1T | degraded 1T | write 16T | healthy 16T | degraded 16T |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 3× copies | 3.00× | 1 MiB | 0.85 | 0.70 | 0.80 | 2.08 | 3.24 | 3.38 |
| 3× copies | 3.00× | 4 MiB | 0.78 | 0.74 | 0.75 | 2.21 | 3.37 | 4.11 |
| 3× copies | 3.00× | 8 MiB | 0.78 | 0.73 | 0.70 | 2.42 | 3.99 | 3.99 |
| 3× copies | 3.00× | 16 MiB | 0.76 | 0.74 | 0.74 | 2.06 | 3.29 | 3.46 |
| 3× copies | 3.00× | 32 MiB | 0.71 | 0.75 | 0.74 | 2.01 | 3.02 | 2.48 |
| 3-of-5 | 1.67× | 1 MiB | 0.53 | 0.71 | 0.19 | 1.13 | 3.33 | 1.30 |
| 3-of-5 | 1.67× | 4 MiB | 0.49 | 0.66 | 0.20 | 1.77 | 3.22 | 1.40 |
| 3-of-5 | 1.67× | 8 MiB | 0.51 | 0.70 | 0.19 | 1.52 | 2.93 | 1.29 |
| 3-of-5 | 1.67× | 16 MiB | 0.46 | 0.70 | 0.19 | 1.82 | 3.15 | 0.94 |
| 3-of-5 | 1.67× | 32 MiB | 0.44 | 0.71 | 0.20 | 1.47 | 2.61 | 1.00 |
| 4-of-6 | 1.50× | 1 MiB | 0.52 | 0.69 | 0.21 | 1.43 | 3.73 | 1.50 |
| 4-of-6 | 1.50× | 4 MiB | 0.55 | 0.69 | 0.23 | 1.53 | 2.76 | 1.52 |
| 4-of-6 | 1.50× | 8 MiB | 0.54 | 0.69 | 0.24 | 1.59 | 3.14 | 1.50 |
| 4-of-6 | 1.50× | 16 MiB | 0.53 | 0.66 | 0.22 | 1.77 | 3.02 | 0.90 |
| 4-of-6 | 1.50× | 32 MiB | 0.51 | 0.73 | 0.23 | 1.52 | 1.85 | 0.98 |
| 6-of-9 | 1.50× | 1 MiB | 0.53 | 0.74 | 0.23 | 1.22 | 3.37 | 1.17 |
| 6-of-9 | 1.50× | 4 MiB | 0.46 | 0.69 | 0.24 | 1.51 | 3.22 | 1.26 |
| 6-of-9 | 1.50× | 8 MiB | 0.48 | 0.67 | 0.25 | 1.88 | 2.96 | 1.15 |
| 6-of-9 | 1.50× | 16 MiB | 0.46 | 0.68 | 0.25 | 1.64 | 3.05 | 1.00 |
| 6-of-9 | 1.50× | 32 MiB | 0.48 | 0.72 | 0.25 | 1.59 | 2.48 | 1.11 |
| 10-of-14 | 1.40× | 1 MiB | 0.52 | 0.76 | 0.28 | 1.22 | 3.41 | 1.41 |
| 10-of-14 | 1.40× | 4 MiB | 0.52 | 0.68 | 0.33 | 1.43 | 3.17 | 1.51 |
| 10-of-14 | 1.40× | 8 MiB | 0.52 | 0.67 | 0.32 | 1.34 | 2.91 | 1.36 |
| 10-of-14 | 1.40× | 16 MiB | 0.51 | 0.66 | 0.32 | 1.78 | 3.03 | 1.27 |
| 10-of-14 | 1.40× | 32 MiB | 0.45 | 0.69 | 0.32 | 1.47 | 2.29 | 1.14 |

## Headline: 1 GiB through 4-of-6, 8 MiB stripes, AES-256-GCM (ring), 64 KiB slices

| Threads | write GB/s | healthy read GB/s | degraded read GB/s | write time | where time goes: write / degraded read |
| ---: | ---: | ---: | ---: | ---: | --- |
| 1 | 0.51 | 0.71 | 0.23 | 2.1 s | encrypt 38% · RS encode 13% · hash 49% / verify 15% · RS decode 72% · decrypt 14% |
| 2 | 0.94 | 1.30 | 0.42 | 1.1 s | encrypt 40% · RS encode 13% · hash 47% / verify 14% · RS decode 72% · decrypt 14% |
| 4 | 1.46 | 2.05 | 0.77 | 0.7 s | encrypt 44% · RS encode 12% · hash 43% / verify 14% · RS decode 72% · decrypt 14% |
| 16 | 2.00 | 3.58 | 1.44 | 0.5 s | encrypt 56% · RS encode 13% · hash 30% / verify 15% · RS decode 75% · decrypt 10% |

