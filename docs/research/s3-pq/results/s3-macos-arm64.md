# S3 results: aarch64 macos (rustcrypto, libcrux, aws-lc)

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
| x-wing (RustCrypto) | 44 | 82 | 173 | 94 | 176 |
| libcrux-kem | n/a | 70 | 79 | 80 | 90 |
| aws-lc-rs (ML-KEM-768 + X25519, combined here) | n/a | 38 | n/a | 46 | n/a |

## ML-DSA-65 and Ed25519 (µs per operation)

Sign from a key already expanded in memory (a long-lived signer); 'sign from seed' includes key expansion.

| Library | Key from seed | Sign | Sign from seed | Verify |
| --- | ---: | ---: | ---: | ---: |
| ml-dsa (RustCrypto) | 235 | 663 | 818 | 132 |
| libcrux-ml-dsa | 50 | 210 | 255 | 52 |
| aws-lc-rs | 32 | 85 | 122 | 30 |
| Ed25519 (ed25519-dalek) | 11 | 12 | 12 | 29 |

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
