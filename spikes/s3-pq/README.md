# Spike S3: post-quantum key wrapping and signatures

**Question:** which libraries give Ursa X-Wing (X25519 + ML-KEM-768) and ML-DSA-65, do they agree
with each other, do they build for every platform the apps ship to, and are they fast enough?

Ursa needs exactly two public-key operations (everything else is symmetric):

- **Wrap a key to someone**: seal a 32-byte key (an album or folder key, or the account root key
  for a new device) to a recipient's X-Wing public key. The X-Wing shared secret goes through
  HKDF-SHA256 (bound to the KEM ciphertext and the recipient's key) into AES-256-GCM, the way
  HPKE single-shot does.
- **Sign** manifests, library-log commits, device certificates and storage receipts with a hybrid
  of Ed25519 and ML-DSA-65; both must verify.

```sh
cargo run --release [--features aws-lc] [-- --quick]
```

Prints interoperability checks (and exits non-zero if any fails), then timings and sizes. CI
(`.github/workflows/s3-pq.yml`) runs it on Linux x86-64 and arm64, macOS (Apple silicon) and
Windows, runs it as WebAssembly under wasmtime, and builds it for iOS, Android and the browser.

## Libraries compared

| | X-Wing | ML-DSA-65 | Notes |
| --- | --- | --- | --- |
| RustCrypto | `x-wing` 0.1.1 (draft-06) | `ml-dsa` 0.1.2 | Pure Rust, pre-1.0, unaudited |
| libcrux (Cryspen) | `libcrux-kem` 0.0.11 (`XWingKemDraft06`) | `libcrux-ml-dsa` 0.0.12 | Pure Rust with SIMD; formally verified (hax/F*); 0.0.x versions |
| aws-lc-rs | assembled here: its ML-KEM-768 + X25519, with the draft-06 SHA3-256 combiner | `signature::ML_DSA_65` (1.19) | C (AWS-LC, FIPS-validated module); the AES-GCM Ursa already uses (S1) |

## Interoperability: all pass

From the same 32-byte seeds, on every platform: both X-Wing implementations open each other's
ciphertexts, and aws-lc-rs's assembled X-Wing is opened by both (aws-lc-rs cannot import an
ML-KEM key from a seed, so it only encapsulates). All three ML-DSA-65 implementations derive the
same public key from a seed and verify each other's signatures, and reject a signature on another
message. A key wrapped with aws-lc-rs unwraps with libcrux; wrong associated data is rejected.

## Platforms

| Target | RustCrypto + libcrux | aws-lc-rs |
| --- | --- | --- |
| Linux x86-64, Linux arm64, macOS, Windows | runs | runs (Windows not tried: needs NASM) |
| iOS (aarch64-apple-ios) | builds | builds, with `IPHONEOS_DEPLOYMENT_TARGET` set |
| Android (aarch64-linux-android) | builds | builds |
| Browser (wasm32-unknown-unknown) | builds | **cannot**: its C needs a libc the target does not have |
| WebAssembly under wasmtime | runs | n/a |

## Timings (µs per operation)

X-Wing: encapsulate / decapsulate; "unwrap" is decapsulating from the seed + HKDF + AES-GCM open.

| Machine | RustCrypto encaps / unwrap | libcrux encaps / unwrap | aws-lc-rs encaps |
| --- | ---: | ---: | ---: |
| Apple M2 Pro (iPhone-class core) | 82 / 176 | 70 / 90 | 38 |
| Neoverse N2 (ARM) | 128 / 279 | 116 / 157 | 63 |
| Xeon (x86-64) | 186 / 403 | 148 / 164 | 51 |
| Windows CI (x86-64) | 226 / 486 | 127 / 156 | n/a |
| WebAssembly (wasmtime) | **176 / 388** | 443 / 501 | n/a |

ML-DSA-65: sign (key already expanded) / verify; Ed25519 for comparison.

| Machine | RustCrypto | libcrux | aws-lc-rs | Ed25519 |
| --- | ---: | ---: | ---: | ---: |
| Apple M2 Pro | 663 / 132 | 210 / 52 | **85 / 30** | 12 / 29 |
| Neoverse N2 | 823 / 227 | 352 / 110 | **200 / 66** | 15 / 41 |
| Xeon | 1720 / 294 | 183 / 73 | **159 / 46** | 20 / 39 |
| Windows CI | 543 / 355 | 192 / 75 | n/a | 23 / 38 |
| WebAssembly | 679 / 479 | **417 / 169** | n/a | 28 / 58 |

(ML-DSA signing time varies with the message because of rejection sampling; read these as
magnitudes. Result files: [results/](results/).)

Sizes: X-Wing public key 1216 B, ciphertext 1120 B, a wrapped key 1168 B; ML-DSA-65 public key
1952 B, signature 3309 B; hybrid signature 3373 B, hybrid public key 1984 B. Private keys are
32-byte seeds for both.

## Findings

1. **Everything is fast enough everywhere.** The slowest operation measured anywhere is under
   2 ms. Ursa wraps a key when sharing or adding a device and signs when committing a batch of
   changes; neither happens per photo. Speed does not decide this.
2. **Portability does.** aws-lc-rs cannot build for the browser, and the web reader (share links)
   must unwrap keys and verify signatures. Using aws-lc-rs natively would mean a second
   implementation of the same security-critical code for the web.
3. **The implementations agree**, so the choice is reversible: keys and seeds are the standard's
   own formats, and a library can be swapped without re-keying anyone.
4. **RustCrypto's ML-DSA is the slow one** (3-9x libcrux natively) and both RustCrypto crates are
   pre-1.0 and unaudited. **libcrux is formally verified**, fastest of the portable two natively,
   and 2.5x slower than RustCrypto at X-Wing only under WebAssembly (0.4 ms: irrelevant).
5. **Signatures are big** (3.4 KB hybrid). Sign the library log's commits and device certificates,
   never each file: a manifest is covered by the commit that adds it.

## Decision

- **libcrux for X-Wing and ML-DSA-65 on every platform**, native and web: one implementation,
  formally verified, interoperable with the others.
- **aws-lc-rs stays for bulk AES-256-GCM on native platforms** (S1); the web reader, which only
  decrypts what it is shown, uses RustCrypto `aes-gcm` or WebCrypto.
- **Ed25519 from `ed25519-dalek`** for the classical half of the signature.
- Behind Ursa's own `crypto` module, so callers never name a library, and with this spike's
  interop checks kept as tests (RustCrypto and aws-lc-rs as the cross-checkers).

Caveats, each with an answer:

- **X-Wing is still a draft** (draft-connolly-cfrg-xwing-kem-06). Every wrapped key and public
  key carries a version byte, and private keys are seeds, so a final RFC that changes the
  combiner or label means re-deriving keys under a new version, not re-keying from scratch.
- **libcrux is 0.0.x.** Pin exact versions and upgrade deliberately; the interop tests catch a
  behaviour change.
- **An X-Wing key pair comes from its seed with libcrux `key_gen_derand`** (the seed as its
  randomness); there is no separate public-key-from-seed call. `ursa-core` derives it that way on
  every open, and the seeds match RustCrypto's in the interop tests.
