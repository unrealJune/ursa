# S1 results: aarch64 · Apple M2 Pro (Virtual) · 5 threads

OS `macos`, crypto features: `aes pmull neon sha3`. GB/s is plaintext bytes (10^9) per second.

## Ciphers (8 MiB pieces)

| Cipher | seal 1T | open 1T | seal 5T | open 5T |
| --- | ---: | ---: | ---: | ---: |
| AES-256-GCM (ring) | 5.25 | 5.59 | 11.57 | 16.43 |
| AES-256-GCM (RustCrypto) | 0.15 | 0.15 | 0.56 | 0.67 |
| ChaCha20-Poly1305 (ring) | 1.60 | 1.60 | 5.78 | 6.41 |
| XChaCha20-Poly1305 (RustCrypto) | 0.44 | 0.44 | 1.87 | 1.94 |

## BLAKE3 and FastCDC (one thread)

| Stage | GB/s | Note |
| --- | ---: | --- |
| BLAKE3, 8 MiB pieces | 1.76 | |
| FastCDC avg 256 KiB | 2.56 | mean chunk 312 KiB |
| FastCDC avg 1024 KiB | 2.56 | mean chunk 1219 KiB |
| FastCDC avg 4096 KiB | 2.54 | mean chunk 5349 KiB |

## RS column slicing (4-of-6, 8 MiB stripes, AES-256-GCM (ring))

The coder works on one column slice of every shard at a time, so its working set is (k + m) × slice instead of (k + m) × shard.

| Slice | write 1T | degraded read 1T | write 5T | degraded read 5T |
| --- | ---: | ---: | ---: | ---: |
| whole shard | 0.88 | 0.80 | 3.00 | 2.70 |
| 1024 KiB | 0.88 | 0.83 | 2.97 | 2.82 |
| 256 KiB | 0.92 | 0.79 | 3.30 | 3.61 |
| 64 KiB | 0.92 | 0.58 | 3.58 | 2.64 |
| 16 KiB | 0.91 | 0.30 | 3.27 | 1.27 |
| 4 KiB | 0.93 | 0.09 | 3.15 | 0.34 |

Best for degraded reads: **256 KiB slices**, used below.

## Pipeline: stripe size × code (AES-256-GCM (ring), 256 KiB slices)

Write = encrypt + RS encode + hash every shard. Healthy read = verify k data shards + decrypt. Degraded read = verify k shards + RS decode with the first min(m, k) data shards lost + decrypt. Overhead = bytes uploaded and stored per input byte.

| Code | Overhead | Stripe | write 1T | healthy 1T | degraded 1T | write 5T | healthy 5T | degraded 5T |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 3× copies | 3.00× | 1 MiB | 1.39 | 1.40 | 1.42 | 5.68 | 5.16 | 5.97 |
| 3× copies | 3.00× | 4 MiB | 1.40 | 1.39 | 1.40 | 5.08 | 6.32 | 5.10 |
| 3× copies | 3.00× | 8 MiB | 1.17 | 1.26 | 1.36 | 4.88 | 5.70 | 5.40 |
| 3× copies | 3.00× | 16 MiB | 1.32 | 1.37 | 1.39 | 4.55 | 5.32 | 6.03 |
| 3× copies | 3.00× | 32 MiB | 1.35 | 1.38 | 1.38 | 4.03 | 4.41 | 5.45 |
| 3-of-5 | 1.67× | 1 MiB | 0.85 | 1.37 | 0.66 | 4.07 | 6.83 | 2.44 |
| 3-of-5 | 1.67× | 4 MiB | 0.85 | 1.39 | 0.70 | 2.96 | 5.71 | 2.92 |
| 3-of-5 | 1.67× | 8 MiB | 0.85 | 1.38 | 0.70 | 2.89 | 5.91 | 3.00 |
| 3-of-5 | 1.67× | 16 MiB | 0.83 | 1.38 | 0.70 | 2.49 | 5.43 | 2.56 |
| 3-of-5 | 1.67× | 32 MiB | 0.81 | 1.33 | 0.69 | 2.47 | 4.73 | 2.48 |
| 4-of-6 | 1.50× | 1 MiB | 0.91 | 1.40 | 0.70 | 4.47 | 5.67 | 2.83 |
| 4-of-6 | 1.50× | 4 MiB | 0.95 | 1.41 | 0.78 | 3.70 | 6.66 | 3.72 |
| 4-of-6 | 1.50× | 8 MiB | 0.95 | 1.38 | 0.79 | 3.58 | 6.48 | 3.20 |
| 4-of-6 | 1.50× | 16 MiB | 0.94 | 1.40 | 0.79 | 3.21 | 6.04 | 3.69 |
| 4-of-6 | 1.50× | 32 MiB | 0.95 | 1.39 | 0.77 | 2.69 | 5.64 | 3.13 |
| 6-of-9 | 1.50× | 1 MiB | 0.89 | 1.39 | 0.69 | 3.56 | 6.90 | 3.25 |
| 6-of-9 | 1.50× | 4 MiB | 0.90 | 1.40 | 0.69 | 3.43 | 5.93 | 2.46 |
| 6-of-9 | 1.50× | 8 MiB | 0.88 | 1.39 | 0.68 | 3.38 | 6.43 | 2.76 |
| 6-of-9 | 1.50× | 16 MiB | 0.88 | 1.39 | 0.70 | 3.11 | 6.01 | 3.15 |
| 6-of-9 | 1.50× | 32 MiB | 0.86 | 1.43 | 0.71 | 2.52 | 5.46 | 2.99 |
| 10-of-14 | 1.40× | 1 MiB | 0.96 | 1.42 | 0.79 | 4.43 | 6.65 | 3.68 |
| 10-of-14 | 1.40× | 4 MiB | 0.89 | 1.28 | 0.74 | 3.71 | 5.45 | 3.51 |
| 10-of-14 | 1.40× | 8 MiB | 0.89 | 1.32 | 0.79 | 3.32 | 5.54 | 3.85 |
| 10-of-14 | 1.40× | 16 MiB | 0.96 | 1.43 | 0.80 | 3.22 | 5.80 | 3.25 |
| 10-of-14 | 1.40× | 32 MiB | 0.96 | 1.40 | 0.83 | 2.65 | 4.95 | 3.09 |

## Headline: 4 GiB through 4-of-6, 8 MiB stripes, AES-256-GCM (ring), 256 KiB slices

| Threads | write GB/s | healthy read GB/s | degraded read GB/s | write time | where time goes: write / degraded read |
| ---: | ---: | ---: | ---: | ---: | --- |
| 1 | 0.96 | 1.41 | 0.81 | 4.5 s | encrypt 17% · RS encode 7% · hash 76% / verify 43% · RS decode 45% · decrypt 12% |
| 2 | 1.84 | 2.77 | 1.57 | 2.3 s | encrypt 17% · RS encode 7% · hash 75% / verify 42% · RS decode 46% · decrypt 12% |
| 4 | 3.62 | 5.13 | 3.06 | 1.2 s | encrypt 19% · RS encode 7% · hash 74% / verify 42% · RS decode 46% · decrypt 12% |
| 5 | 3.70 | 5.79 | 3.78 | 1.2 s | encrypt 21% · RS encode 8% · hash 70% / verify 42% · RS decode 47% · decrypt 12% |

