# S1 results: x86_64 · Intel(R) Xeon(R) 6973P-C · 2 threads

OS `linux`, crypto features: `aes pclmulqdq avx2 avx512f vaes`. GB/s is plaintext bytes (10^9) per second.

## Ciphers (8 MiB pieces)

| Cipher | seal 1T | open 1T | seal 2T | open 2T |
| --- | ---: | ---: | ---: | ---: |
| AES-256-GCM (ring) | 4.94 | 5.93 | 6.93 | 7.10 |
| AES-256-GCM (aws-lc-rs) | 5.92 | 7.54 | 7.22 | 7.80 |
| AES-256-GCM (RustCrypto) | 1.42 | 1.50 | 1.99 | 2.03 |
| ChaCha20-Poly1305 (ring) | 2.00 | 2.13 | 2.51 | 2.60 |
| XChaCha20-Poly1305 (RustCrypto) | 1.30 | 1.38 | 1.77 | 1.78 |

## BLAKE3 and FastCDC (one thread)

| Stage | GB/s | Note |
| --- | ---: | --- |
| BLAKE3, 8 MiB pieces | 5.85 | |
| FastCDC avg 256 KiB | 1.85 | mean chunk 312 KiB |
| FastCDC avg 1024 KiB | 1.89 | mean chunk 1219 KiB |
| FastCDC avg 4096 KiB | 1.85 | mean chunk 5349 KiB |

## RS column slicing (4-of-6, 8 MiB stripes, AES-256-GCM (ring))

The coder works on one column slice of every shard at a time, so its working set is (k + m) × slice instead of (k + m) × shard.

| Slice | write 1T | degraded read 1T | write 2T | degraded read 2T |
| --- | ---: | ---: | ---: | ---: |
| whole shard | 1.64 | 0.86 | 1.99 | 0.96 |
| 1024 KiB | 1.67 | 0.85 | 1.87 | 0.89 |
| 256 KiB | 1.98 | 1.08 | 2.11 | 1.10 |
| 64 KiB | 2.05 | 0.75 | 2.37 | 0.85 |
| 16 KiB | 2.03 | 0.32 | 2.34 | 0.35 |
| 4 KiB | 2.12 | 0.10 | 2.37 | 0.11 |

Best for degraded reads: **256 KiB slices**, used below.

## Pipeline: stripe size × code (AES-256-GCM (ring), 256 KiB slices)

Write = encrypt + RS encode + hash every shard. Healthy read = verify k data shards + decrypt. Degraded read = verify k shards + RS decode with the first min(m, k) data shards lost + decrypt. Overhead = bytes uploaded and stored per input byte.

| Code | Overhead | Stripe | write 1T | healthy 1T | degraded 1T | write 2T | healthy 2T | degraded 2T |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 3× copies | 3.00× | 1 MiB | 3.29 | 4.18 | 4.19 | 3.99 | 3.56 | 3.58 |
| 3× copies | 3.00× | 4 MiB | 3.11 | 3.50 | 3.52 | 3.62 | 3.67 | 3.61 |
| 3× copies | 3.00× | 8 MiB | 3.05 | 3.46 | 3.46 | 3.55 | 3.85 | 3.83 |
| 3× copies | 3.00× | 16 MiB | 2.91 | 3.22 | 3.20 | 3.05 | 3.65 | 3.36 |
| 3× copies | 3.00× | 32 MiB | 2.84 | 2.68 | 2.67 | 2.92 | 2.67 | 2.66 |
| 3-of-5 | 1.67× | 1 MiB | 1.77 | 3.85 | 0.87 | 1.85 | 3.34 | 0.87 |
| 3-of-5 | 1.67× | 4 MiB | 1.79 | 3.12 | 0.92 | 1.97 | 3.33 | 0.91 |
| 3-of-5 | 1.67× | 8 MiB | 1.77 | 3.08 | 0.93 | 1.97 | 3.31 | 0.93 |
| 3-of-5 | 1.67× | 16 MiB | 1.76 | 2.85 | 0.91 | 1.73 | 3.23 | 0.88 |
| 3-of-5 | 1.67× | 32 MiB | 1.67 | 2.46 | 0.85 | 1.58 | 2.42 | 0.82 |
| 4-of-6 | 1.50× | 1 MiB | 1.98 | 3.89 | 0.95 | 2.04 | 3.39 | 1.00 |
| 4-of-6 | 1.50× | 4 MiB | 2.02 | 3.11 | 1.05 | 2.12 | 3.30 | 1.10 |
| 4-of-6 | 1.50× | 8 MiB | 1.97 | 3.08 | 1.08 | 2.09 | 3.31 | 1.11 |
| 4-of-6 | 1.50× | 16 MiB | 1.94 | 2.79 | 1.05 | 1.93 | 3.13 | 1.10 |
| 4-of-6 | 1.50× | 32 MiB | 1.86 | 2.44 | 0.97 | 1.78 | 2.38 | 0.95 |
| 6-of-9 | 1.50× | 1 MiB | 1.76 | 3.77 | 0.88 | 1.81 | 3.27 | 0.89 |
| 6-of-9 | 1.50× | 4 MiB | 1.68 | 3.08 | 0.78 | 1.72 | 3.32 | 0.82 |
| 6-of-9 | 1.50× | 8 MiB | 1.63 | 3.07 | 0.76 | 1.77 | 3.30 | 0.79 |
| 6-of-9 | 1.50× | 16 MiB | 1.68 | 2.81 | 0.76 | 1.65 | 3.26 | 0.80 |
| 6-of-9 | 1.50× | 32 MiB | 1.64 | 2.41 | 0.69 | 1.53 | 2.51 | 0.73 |
| 10-of-14 | 1.40× | 1 MiB | 1.88 | 3.73 | 1.22 | 1.97 | 3.42 | 1.25 |
| 10-of-14 | 1.40× | 4 MiB | 1.76 | 3.14 | 1.05 | 1.79 | 3.34 | 1.08 |
| 10-of-14 | 1.40× | 8 MiB | 1.72 | 3.10 | 1.00 | 1.75 | 3.32 | 1.10 |
| 10-of-14 | 1.40× | 16 MiB | 1.76 | 2.91 | 0.99 | 1.74 | 3.23 | 1.00 |
| 10-of-14 | 1.40× | 32 MiB | 1.67 | 2.49 | 0.91 | 1.61 | 2.42 | 0.92 |

## Headline: 4 GiB through 4-of-6, 8 MiB stripes, AES-256-GCM (ring), 256 KiB slices

| Threads | write GB/s | healthy read GB/s | degraded read GB/s | write time | where time goes: write / degraded read |
| ---: | ---: | ---: | ---: | ---: | --- |
| 1 | 2.00 | 3.07 | 1.07 | 2.1 s | encrypt 40% · RS encode 20% · hash 40% / verify 16% · RS decode 74% · decrypt 10% |
| 2 | 2.10 | 3.28 | 1.09 | 2.0 s | encrypt 33% · RS encode 26% · hash 41% / verify 15% · RS decode 76% · decrypt 9% |

