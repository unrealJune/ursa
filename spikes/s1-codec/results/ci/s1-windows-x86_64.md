# S1 results: x86_64 · Intel64 Family 6 Model 207 Stepping 2, GenuineIntel · 2 threads

OS `windows`, crypto features: `aes pclmulqdq avx2 avx512f vaes`. GB/s is plaintext bytes (10^9) per second.

## Ciphers (8 MiB pieces)

| Cipher | seal 1T | open 1T | seal 2T | open 2T |
| --- | ---: | ---: | ---: | ---: |
| AES-256-GCM (ring) | 3.61 | 3.85 | 3.70 | 4.88 |
| AES-256-GCM (RustCrypto) | 1.01 | 1.02 | 1.30 | 1.42 |
| ChaCha20-Poly1305 (ring) | 1.48 | 1.53 | 1.78 | 1.80 |
| XChaCha20-Poly1305 (RustCrypto) | 1.00 | 1.00 | 1.32 | 1.28 |

## BLAKE3 and FastCDC (one thread)

| Stage | GB/s | Note |
| --- | ---: | --- |
| BLAKE3, 8 MiB pieces | 4.86 | |
| FastCDC avg 256 KiB | 1.84 | mean chunk 312 KiB |
| FastCDC avg 1024 KiB | 1.88 | mean chunk 1219 KiB |
| FastCDC avg 4096 KiB | 1.82 | mean chunk 5349 KiB |

## RS column slicing (4-of-6, 8 MiB stripes, AES-256-GCM (ring))

The coder works on one column slice of every shard at a time, so its working set is (k + m) × slice instead of (k + m) × shard.

| Slice | write 1T | degraded read 1T | write 2T | degraded read 2T |
| --- | ---: | ---: | ---: | ---: |
| whole shard | 1.11 | 0.61 | 1.16 | 0.51 |
| 1024 KiB | 0.79 | 0.66 | 1.28 | 0.72 |
| 256 KiB | 1.35 | 0.76 | 1.40 | 0.77 |
| 64 KiB | 1.38 | 0.53 | 1.45 | 0.59 |
| 16 KiB | 1.39 | 0.22 | 1.40 | 0.24 |
| 4 KiB | 1.40 | 0.07 | 1.54 | 0.08 |

Best for degraded reads: **256 KiB slices**, used below.

## Pipeline: stripe size × code (AES-256-GCM (ring), 256 KiB slices)

Write = encrypt + RS encode + hash every shard. Healthy read = verify k data shards + decrypt. Degraded read = verify k shards + RS decode with the first min(m, k) data shards lost + decrypt. Overhead = bytes uploaded and stored per input byte.

| Code | Overhead | Stripe | write 1T | healthy 1T | degraded 1T | write 2T | healthy 2T | degraded 2T |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 3× copies | 3.00× | 1 MiB | 2.58 | 2.80 | 2.85 | 2.63 | 2.78 | 2.65 |
| 3× copies | 3.00× | 4 MiB | 2.12 | 2.41 | 2.41 | 2.55 | 2.66 | 2.56 |
| 3× copies | 3.00× | 8 MiB | 2.00 | 2.31 | 2.22 | 2.26 | 2.50 | 2.67 |
| 3× copies | 3.00× | 16 MiB | 2.01 | 2.15 | 2.11 | 2.03 | 2.28 | 2.36 |
| 3× copies | 3.00× | 32 MiB | 1.94 | 2.07 | 2.06 | 1.77 | 2.12 | 2.16 |
| 3-of-5 | 1.67× | 1 MiB | 1.38 | 2.63 | 0.65 | 1.41 | 2.43 | 0.65 |
| 3-of-5 | 1.67× | 4 MiB | 1.25 | 2.28 | 0.68 | 1.40 | 2.41 | 0.70 |
| 3-of-5 | 1.67× | 8 MiB | 1.27 | 1.74 | 0.66 | 1.26 | 2.05 | 0.69 |
| 3-of-5 | 1.67× | 16 MiB | 1.19 | 1.71 | 0.65 | 1.13 | 2.00 | 0.66 |
| 3-of-5 | 1.67× | 32 MiB | 1.06 | 1.67 | 0.63 | 1.06 | 1.90 | 0.66 |
| 4-of-6 | 1.50× | 1 MiB | 1.48 | 2.83 | 0.70 | 1.48 | 2.62 | 0.72 |
| 4-of-6 | 1.50× | 4 MiB | 1.35 | 2.41 | 0.79 | 1.51 | 2.40 | 0.81 |
| 4-of-6 | 1.50× | 8 MiB | 1.33 | 1.78 | 0.75 | 1.35 | 2.08 | 0.79 |
| 4-of-6 | 1.50× | 16 MiB | 1.26 | 1.72 | 0.77 | 1.27 | 1.97 | 0.77 |
| 4-of-6 | 1.50× | 32 MiB | 1.18 | 1.67 | 0.74 | 1.10 | 1.92 | 0.75 |
| 6-of-9 | 1.50× | 1 MiB | 1.34 | 2.58 | 0.66 | 1.38 | 2.40 | 0.66 |
| 6-of-9 | 1.50× | 4 MiB | 1.17 | 2.27 | 0.61 | 1.25 | 2.49 | 0.62 |
| 6-of-9 | 1.50× | 8 MiB | 1.19 | 2.21 | 0.59 | 1.26 | 2.24 | 0.61 |
| 6-of-9 | 1.50× | 16 MiB | 1.14 | 1.71 | 0.57 | 1.16 | 1.98 | 0.60 |
| 6-of-9 | 1.50× | 32 MiB | 1.07 | 1.66 | 0.56 | 1.02 | 1.91 | 0.59 |
| 10-of-14 | 1.40× | 1 MiB | 1.42 | 2.59 | 0.85 | 1.45 | 2.39 | 0.89 |
| 10-of-14 | 1.40× | 4 MiB | 1.21 | 2.25 | 0.79 | 1.22 | 2.35 | 0.84 |
| 10-of-14 | 1.40× | 8 MiB | 1.19 | 2.13 | 0.76 | 1.20 | 2.27 | 0.77 |
| 10-of-14 | 1.40× | 16 MiB | 1.19 | 1.68 | 0.72 | 1.16 | 1.96 | 0.75 |
| 10-of-14 | 1.40× | 32 MiB | 1.13 | 1.69 | 0.71 | 1.08 | 1.85 | 0.73 |

## Headline: 4 GiB through 4-of-6, 8 MiB stripes, AES-256-GCM (ring), 256 KiB slices

| Threads | write GB/s | healthy read GB/s | degraded read GB/s | write time | where time goes: write / degraded read |
| ---: | ---: | ---: | ---: | ---: | --- |
| 1 | 1.36 | 1.77 | 0.74 | 3.2 s | encrypt 36% · RS encode 22% · hash 42% / verify 16% · RS decode 73% · decrypt 11% |
| 2 | 1.49 | 2.08 | 0.81 | 2.9 s | encrypt 33% · RS encode 24% · hash 44% / verify 16% · RS decode 75% · decrypt 10% |

