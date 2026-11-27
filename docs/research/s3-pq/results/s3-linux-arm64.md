# S3 results: aarch64 linux (rustcrypto, libcrux, aws-lc)

## Interoperability

| Check | Result |
| --- | --- |
| X-Wing public key from seed: x-wing (RustCrypto) matches | pass |
| X-Wing: x-wing (RustCrypto) encapsulates, x-wing (RustCrypto) decapsulates | pass |
| X-Wing: x-wing (RustCrypto) encapsulates, libcrux-kem decapsulates | pass |
| X-Wing: libcrux-kem encapsulates, x-wing (RustCrypto) decapsulates | pass |
| X-Wing: libcrux-kem encapsulates, libcrux-kem decapsulates | pass |
| X-Wing: aws-lc-rs (ML-KEM-768 + X25519, combined here) encapsulates, x-wing (RustCrypto) decapsulates | pass |
| X-Wing: aws-lc-rs (ML-KEM-768 + X25519, combined here) encapsulates, libcrux-kem decapsulates | pass |
| Key wrap: sealed via aws-lc-rs (ML-KEM-768 + X25519, combined here), opened via libcrux-kem | pass |
| Key wrap: wrong associated data is rejected | pass |
| ML-DSA-65 public key from seed: ml-dsa (RustCrypto) matches | pass |
| ML-DSA-65 public key from seed: libcrux-ml-dsa matches | pass |
| ML-DSA-65 public key from seed: aws-lc-rs matches | pass |
| ML-DSA-65: ml-dsa (RustCrypto) signs, ml-dsa (RustCrypto) verifies | pass |
| ML-DSA-65: ml-dsa (RustCrypto) signs, libcrux-ml-dsa verifies | pass |
| ML-DSA-65: ml-dsa (RustCrypto) signs, aws-lc-rs verifies | pass |
| ML-DSA-65: ml-dsa (RustCrypto) signature rejected on another message | pass |
| ML-DSA-65: libcrux-ml-dsa signs, ml-dsa (RustCrypto) verifies | pass |
| ML-DSA-65: libcrux-ml-dsa signs, libcrux-ml-dsa verifies | pass |
| ML-DSA-65: libcrux-ml-dsa signs, aws-lc-rs verifies | pass |
| ML-DSA-65: libcrux-ml-dsa signature rejected on another message | pass |
| ML-DSA-65: aws-lc-rs signs, ml-dsa (RustCrypto) verifies | pass |
| ML-DSA-65: aws-lc-rs signs, libcrux-ml-dsa verifies | pass |
| ML-DSA-65: aws-lc-rs signs, aws-lc-rs verifies | pass |
| ML-DSA-65: aws-lc-rs signature rejected on another message | pass |

## X-Wing (µs per operation)

Wrap = encapsulate + HKDF + AES-256-GCM over a 32-byte key; unwrap = decapsulate from the seed + HKDF + open.

| Library | Public key from seed | Encapsulate | Decapsulate | Wrap a key | Unwrap a key |
| --- | ---: | ---: | ---: | ---: | ---: |
| x-wing (RustCrypto) | 72 | 128 | 271 | 136 | 279 |
| libcrux-kem | n/a | 116 | 150 | 124 | 157 |
| aws-lc-rs (ML-KEM-768 + X25519, combined here) | n/a | 63 | n/a | 71 | n/a |

## ML-DSA-65 and Ed25519 (µs per operation)

Sign from a key already expanded in memory (a long-lived signer); 'sign from seed' includes key expansion.

| Library | Key from seed | Sign | Sign from seed | Verify |
| --- | ---: | ---: | ---: | ---: |
| ml-dsa (RustCrypto) | 389 | 823 | 1117 | 227 |
| libcrux-ml-dsa | 109 | 352 | 462 | 110 |
| aws-lc-rs | 73 | 200 | 267 | 66 |
| Ed25519 (ed25519-dalek) | 14 | 15 | 15 | 41 |

## Sizes (bytes)

| Thing | Size |
| --- | ---: |
| X-Wing private key (seed) | 32 |
| X-Wing public key | 1216 (ML-KEM-768 1184 + X25519 32) |
| X-Wing ciphertext | 1120 (ML-KEM-768 1088 + X25519 32) |
| A wrapped 32-byte key | 1168 (ciphertext + 32 + 16 tag) |
| ML-DSA-65 private key (seed) | 32 |
| ML-DSA-65 public key | 1952 |
| ML-DSA-65 signature | 3309 |
| Hybrid signature (Ed25519 + ML-DSA-65) | 3373 |
| Hybrid public key | 1984 |
