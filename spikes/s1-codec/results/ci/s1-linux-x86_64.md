# S1 results: x86_64 · INTEL(R) XEON(R) PLATINUM 8573C · 2 threads

OS `linux`, crypto features: `aes pclmulqdq avx2 avx512f vaes`. GB/s is plaintext bytes (10^9) per second.

## Ciphers (8 MiB pieces)

| Cipher | seal 1T | open 1T | seal 2T | open 2T |
| --- | ---: | ---: | ---: | ---: |
| AES-256-GCM (ring) | 4.02 | 4.84 | 5.14 | 6.09 |
| AES-256-GCM (RustCrypto) | 1.14 | 1.14 | 1.52 | 1.51 |
| ChaCha20-Poly1305 (ring) | 1.64 | 1.64 | 2.00 | 1.98 |
| XChaCha20-Poly1305 (RustCrypto) | 1.10 | 1.10 | 1.49 | 1.43 |

## BLAKE3 and FastCDC (one thread)

| Stage | GB/s | Note |
| --- | ---: | --- |
| BLAKE3, 8 MiB pieces | 5.17 | |
| FastCDC avg 256 KiB | 2.09 | mean chunk 312 KiB |
| FastCDC avg 1024 KiB | 2.08 | mean chunk 1219 KiB |
| FastCDC avg 4096 KiB | 2.05 | mean chunk 5349 KiB |

## RS column slicing (4-of-6, 8 MiB stripes, AES-256-GCM (ring))

The coder works on one column slice of every shard at a time, so its working set is (k + m) × slice instead of (k + m) × shard.

| Slice | write 1T | degraded read 1T | write 2T | degraded read 2T |
| --- | ---: | ---: | ---: | ---: |
| whole shard | 1.40 | 0.72 | 1.53 | 0.77 |
| 1024 KiB | 1.42 | 0.72 | 1.50 | 0.77 |
| 256 KiB | 1.63 | 0.84 | 1.70 | 0.88 |
| 64 KiB | 1.66 | 0.59 | 1.84 | 0.65 |
| 16 KiB | 1.68 | 0.25 | 1.85 | 0.27 |
| 4 KiB | 1.72 | 0.08 | 1.86 | 0.09 |

Best for degraded reads: **256 KiB slices**, used below.

## Pipeline: stripe size × code (AES-256-GCM (ring), 256 KiB slices)

Write = encrypt + RS encode + hash every shard. Healthy read = verify k data shards + decrypt. Degraded read = verify k shards + RS decode with the first min(m, k) data shards lost + decrypt. Overhead = bytes uploaded and stored per input byte.

| Code | Overhead | Stripe | write 1T | healthy 1T | degraded 1T | write 2T | healthy 2T | degraded 2T |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 3× copies | 3.00× | 1 MiB | 2.68 | 3.19 | 3.21 | 2.89 | 3.02 | 2.90 |
| 3× copies | 3.00× | 4 MiB | 2.55 | 2.78 | 2.79 | 2.97 | 3.15 | 2.83 |
| 3× copies | 3.00× | 8 MiB | 2.54 | 2.69 | 2.70 | 2.86 | 2.96 | 3.07 |
| 3× copies | 3.00× | 16 MiB | 2.42 | 2.48 | 2.49 | 2.38 | 2.92 | 2.91 |
| 3× copies | 3.00× | 32 MiB | 2.35 | 2.07 | 2.22 | 2.25 | 2.15 | 2.20 |
| 3-of-5 | 1.67× | 1 MiB | 1.49 | 2.89 | 0.69 | 1.51 | 2.74 | 0.70 |
| 3-of-5 | 1.67× | 4 MiB | 1.47 | 2.51 | 0.73 | 1.60 | 2.70 | 0.75 |
| 3-of-5 | 1.67× | 8 MiB | 1.50 | 2.44 | 0.75 | 1.59 | 2.60 | 0.75 |
| 3-of-5 | 1.67× | 16 MiB | 1.43 | 2.23 | 0.73 | 1.37 | 2.56 | 0.72 |
| 3-of-5 | 1.67× | 32 MiB | 1.29 | 2.01 | 0.68 | 1.28 | 1.92 | 0.69 |
| 4-of-6 | 1.50× | 1 MiB | 1.66 | 2.85 | 0.80 | 1.68 | 2.81 | 0.82 |
| 4-of-6 | 1.50× | 4 MiB | 1.65 | 2.53 | 0.84 | 1.74 | 2.80 | 0.91 |
| 4-of-6 | 1.50× | 8 MiB | 1.66 | 2.42 | 0.88 | 1.72 | 2.62 | 0.89 |
| 4-of-6 | 1.50× | 16 MiB | 1.59 | 2.27 | 0.84 | 1.54 | 2.68 | 0.88 |
| 4-of-6 | 1.50× | 32 MiB | 1.49 | 2.02 | 0.80 | 1.47 | 1.93 | 0.80 |
| 6-of-9 | 1.50× | 1 MiB | 1.49 | 2.87 | 0.72 | 1.48 | 2.73 | 0.71 |
| 6-of-9 | 1.50× | 4 MiB | 1.41 | 2.45 | 0.64 | 1.43 | 2.68 | 0.69 |
| 6-of-9 | 1.50× | 8 MiB | 1.38 | 2.39 | 0.64 | 1.41 | 2.61 | 0.67 |
| 6-of-9 | 1.50× | 16 MiB | 1.36 | 2.24 | 0.63 | 1.31 | 2.64 | 0.65 |
| 6-of-9 | 1.50× | 32 MiB | 1.25 | 2.00 | 0.59 | 1.21 | 1.92 | 0.62 |
| 10-of-14 | 1.40× | 1 MiB | 1.56 | 2.84 | 0.95 | 1.61 | 2.68 | 0.98 |
| 10-of-14 | 1.40× | 4 MiB | 1.45 | 2.46 | 0.85 | 1.51 | 2.65 | 0.89 |
| 10-of-14 | 1.40× | 8 MiB | 1.44 | 2.39 | 0.82 | 1.42 | 2.65 | 0.85 |
| 10-of-14 | 1.40× | 16 MiB | 1.40 | 2.19 | 0.81 | 1.37 | 2.57 | 0.81 |
| 10-of-14 | 1.40× | 32 MiB | 1.34 | 2.01 | 0.75 | 1.28 | 1.98 | 0.79 |

## Headline: 4 GiB through 4-of-6, 8 MiB stripes, AES-256-GCM (ring), 256 KiB slices

| Threads | write GB/s | healthy read GB/s | degraded read GB/s | write time | where time goes: write / degraded read |
| ---: | ---: | ---: | ---: | ---: | --- |
| 1 | 1.65 | 2.44 | 0.87 | 2.6 s | encrypt 37% · RS encode 21% · hash 43% / verify 16% · RS decode 73% · decrypt 10% |
| 2 | 1.75 | 2.68 | 0.90 | 2.5 s | encrypt 32% · RS encode 24% · hash 44% / verify 15% · RS decode 75% · decrypt 10% |

