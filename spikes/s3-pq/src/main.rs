//! Spike S3: which post-quantum libraries, and are they fast and portable enough?
//!
//! Ursa needs exactly two public-key operations (content encryption is symmetric):
//!
//! - **Wrap a key to someone**: seal a 32-byte key (an album key, the account root for a new
//!   device) to a recipient's X-Wing public key (X25519 + ML-KEM-768, draft-connolly-cfrg-xwing
//!   -kem-06). The KEM's shared secret goes through HKDF-SHA256 into AES-256-GCM, as HPKE does.
//! - **Sign**: manifests, device certificates and storage receipts carry a hybrid signature,
//!   Ed25519 and ML-DSA-65 (FIPS 204), both of which must verify.
//!
//! For each library that provides them this checks that implementations agree with each other
//! from the same seeds (a ciphertext from one opens with another, a signature from one verifies
//! with another), and times the operations. Exits non-zero if any check fails.

use std::fmt::Write as _;
use std::hint::black_box;
use std::time::Instant;

use aes_gcm::aead::{Aead, KeyInit, Payload};
use hkdf::Hkdf;
use sha2::Sha256;

const XWING_PK: usize = 1216;
const XWING_CT: usize = 1120;
const MLKEM768_PK: usize = 1184;
const MLKEM768_CT: usize = 1088;
const XWING_LABEL: &[u8; 6] = br"\.//^\";

fn random<const N: usize>() -> [u8; N] {
    let mut b = [0u8; N];
    getrandom::fill(&mut b).expect("getrandom");
    b
}

// ---------------------------------------------------------------------------------------------
// The wrap construction, independent of which library produced the shared secret.

/// AEAD key from the KEM shared secret, bound to the KEM ciphertext and the recipient's key.
fn wrap_key(ss: &[u8], kem_ct: &[u8], recipient_pk: &[u8]) -> [u8; 32] {
    let hk = Hkdf::<Sha256>::new(None, ss);
    let mut info = Vec::with_capacity(32 + kem_ct.len() + recipient_pk.len());
    info.extend_from_slice(b"ursa key-wrap v1 x-wing");
    info.extend_from_slice(&blake_ish(kem_ct));
    info.extend_from_slice(&blake_ish(recipient_pk));
    let mut okm = [0u8; 32];
    hk.expand(&info, &mut okm).expect("hkdf");
    okm
}

fn blake_ish(b: &[u8]) -> [u8; 32] {
    use sha2::Digest;
    Sha256::digest(b).into()
}

/// The sealed payload: AES-256-GCM under a key used exactly once, so a fixed nonce is safe.
fn seal(key: &[u8; 32], plaintext: &[u8], aad: &[u8]) -> Vec<u8> {
    let c = aes_gcm::Aes256Gcm::new_from_slice(key).unwrap();
    c.encrypt(&[0u8; 12].into(), Payload { msg: plaintext, aad }).unwrap()
}

fn open(key: &[u8; 32], sealed: &[u8], aad: &[u8]) -> Option<Vec<u8>> {
    let c = aes_gcm::Aes256Gcm::new_from_slice(key).unwrap();
    c.decrypt(&[0u8; 12].into(), Payload { msg: sealed, aad }).ok()
}

// ---------------------------------------------------------------------------------------------
// X-Wing backends. Private keys are the draft's 32-byte seed in every backend.

trait XWing {
    fn name(&self) -> &'static str;
    /// Public key from the seed, if the library exposes that.
    fn public(&self, seed: &[u8; 32]) -> Option<Vec<u8>>;
    fn encaps(&self, pk: &[u8]) -> (Vec<u8>, [u8; 32]);
    /// None if this backend cannot decapsulate from a seed.
    fn decaps(&self, seed: &[u8; 32], ct: &[u8]) -> Option<[u8; 32]>;
}

#[cfg(feature = "rustcrypto")]
struct RcXWing;

#[cfg(feature = "rustcrypto")]
impl XWing for RcXWing {
    fn name(&self) -> &'static str {
        "x-wing (RustCrypto)"
    }
    fn public(&self, seed: &[u8; 32]) -> Option<Vec<u8>> {
        use x_wing::{Decapsulator, KeyExport};
        let dk = x_wing::DecapsulationKey::from(*seed);
        Some(dk.encapsulation_key().to_bytes().to_vec())
    }
    fn encaps(&self, pk: &[u8]) -> (Vec<u8>, [u8; 32]) {
        use x_wing::Encapsulate;
        let ek = x_wing::EncapsulationKey::try_from(pk).expect("x-wing pk");
        let (ct, ss) = ek.encapsulate();
        (ct.to_vec(), ss.into())
    }
    fn decaps(&self, seed: &[u8; 32], ct: &[u8]) -> Option<[u8; 32]> {
        use x_wing::Decapsulate;
        let dk = x_wing::DecapsulationKey::from(*seed);
        Some(dk.decapsulate_slice(ct).expect("x-wing ct").into())
    }
}

#[cfg(feature = "libcrux")]
struct LcXWing;

#[cfg(feature = "libcrux")]
impl XWing for LcXWing {
    fn name(&self) -> &'static str {
        "libcrux-kem"
    }
    fn public(&self, _seed: &[u8; 32]) -> Option<Vec<u8>> {
        None // libcrux derives X-Wing public keys only inside key_gen(rng)
    }
    fn encaps(&self, pk: &[u8]) -> (Vec<u8>, [u8; 32]) {
        use libcrux_kem::{Algorithm, PublicKey};
        let pk = PublicKey::decode(Algorithm::XWingKemDraft06, pk).expect("libcrux pk");
        let (ss, ct) = pk.encapsulate_derand(&random::<64>()).expect("libcrux encaps");
        (ct.encode(), ss.encode().try_into().expect("32-byte ss"))
    }
    fn decaps(&self, seed: &[u8; 32], ct: &[u8]) -> Option<[u8; 32]> {
        use libcrux_kem::{Algorithm, Ct, PrivateKey};
        let sk = PrivateKey::decode(Algorithm::XWingKemDraft06, seed).expect("libcrux sk");
        let ct = Ct::decode(Algorithm::XWingKemDraft06, ct).expect("libcrux ct");
        Some(ct.decapsulate(&sk).expect("libcrux decaps").encode().try_into().expect("32-byte ss"))
    }
}

/// X-Wing assembled from aws-lc-rs's ML-KEM-768 and X25519 with the draft's SHA3-256 combiner.
/// aws-lc-rs cannot import an ML-KEM key from a seed, so this backend only encapsulates.
#[cfg(feature = "aws-lc")]
struct AwsXWing;

#[cfg(feature = "aws-lc")]
impl XWing for AwsXWing {
    fn name(&self) -> &'static str {
        "aws-lc-rs (ML-KEM-768 + X25519, combined here)"
    }
    fn public(&self, _seed: &[u8; 32]) -> Option<Vec<u8>> {
        None
    }
    fn encaps(&self, pk: &[u8]) -> (Vec<u8>, [u8; 32]) {
        use aws_lc_rs::agreement::{self, EphemeralPrivateKey, UnparsedPublicKey, X25519};
        use aws_lc_rs::kem::{EncapsulationKey, ML_KEM_768};
        let (pk_m, pk_x) = pk.split_at(MLKEM768_PK);
        let ek = EncapsulationKey::new(&ML_KEM_768, pk_m).expect("aws ek");
        let (ct_m, ss_m) = ek.encapsulate().expect("aws encaps");
        let rng = aws_lc_rs::rand::SystemRandom::new();
        let eph = EphemeralPrivateKey::generate(&X25519, &rng).expect("x25519");
        let ct_x = eph.compute_public_key().expect("x25519 pub");
        let ss_x = agreement::agree_ephemeral(eph, UnparsedPublicKey::new(&X25519, pk_x), (), |k| Ok(k.to_vec()))
            .expect("x25519 agree");
        let ss = xwing_combiner(ss_m.as_ref(), &ss_x, ct_x.as_ref(), pk_x);
        let mut ct = ct_m.as_ref().to_vec();
        ct.extend_from_slice(ct_x.as_ref());
        (ct, ss)
    }
    fn decaps(&self, _seed: &[u8; 32], _ct: &[u8]) -> Option<[u8; 32]> {
        None
    }
}

#[cfg_attr(not(feature = "aws-lc"), allow(dead_code))]
fn xwing_combiner(ss_m: &[u8], ss_x: &[u8], ct_x: &[u8], pk_x: &[u8]) -> [u8; 32] {
    use sha3::{Digest, Sha3_256};
    let mut h = Sha3_256::new();
    h.update(ss_m);
    h.update(ss_x);
    h.update(ct_x);
    h.update(pk_x);
    h.update(XWING_LABEL);
    h.finalize().into()
}

// ---------------------------------------------------------------------------------------------
// ML-DSA-65 backends. Keys come from the FIPS 204 32-byte seed; context is empty.

trait MlDsa {
    fn name(&self) -> &'static str;
    fn public(&self, seed: &[u8; 32]) -> Vec<u8>;
    fn sign(&self, seed: &[u8; 32], msg: &[u8]) -> Vec<u8>;
    fn verify(&self, pk: &[u8], msg: &[u8], sig: &[u8]) -> bool;
    /// Signing with a key already expanded (what a long-lived signer would hold).
    fn signer(&self, seed: &[u8; 32]) -> Box<dyn Fn(&[u8]) -> Vec<u8>>;
}

#[cfg(feature = "rustcrypto")]
struct RcMlDsa;

#[cfg(feature = "rustcrypto")]
impl MlDsa for RcMlDsa {
    fn name(&self) -> &'static str {
        "ml-dsa (RustCrypto)"
    }
    fn public(&self, seed: &[u8; 32]) -> Vec<u8> {
        let sk = ml_dsa::SigningKey::<ml_dsa::MlDsa65>::from_seed(&(*seed).into());
        sk.expanded_key().verifying_key().encode().to_vec()
    }
    fn sign(&self, seed: &[u8; 32], msg: &[u8]) -> Vec<u8> {
        let sk = ml_dsa::SigningKey::<ml_dsa::MlDsa65>::from_seed(&(*seed).into());
        sk.expanded_key().sign_deterministic(msg, b"").expect("sign").encode().to_vec()
    }
    fn verify(&self, pk: &[u8], msg: &[u8], sig: &[u8]) -> bool {
        let Ok(pk) = pk.try_into() else { return false };
        let vk = ml_dsa::VerifyingKey::<ml_dsa::MlDsa65>::decode(pk);
        let Ok(sig) = sig.try_into() else { return false };
        let Some(sig) = ml_dsa::Signature::<ml_dsa::MlDsa65>::decode(sig) else { return false };
        vk.verify_with_context(msg, b"", &sig)
    }
    fn signer(&self, seed: &[u8; 32]) -> Box<dyn Fn(&[u8]) -> Vec<u8>> {
        let sk = ml_dsa::SigningKey::<ml_dsa::MlDsa65>::from_seed(&(*seed).into());
        let esk = sk.expanded_key().clone();
        Box::new(move |m| esk.sign_deterministic(m, b"").expect("sign").encode().to_vec())
    }
}

#[cfg(feature = "libcrux")]
struct LcMlDsa;

#[cfg(feature = "libcrux")]
impl MlDsa for LcMlDsa {
    fn name(&self) -> &'static str {
        "libcrux-ml-dsa"
    }
    fn public(&self, seed: &[u8; 32]) -> Vec<u8> {
        libcrux_ml_dsa::ml_dsa_65::generate_key_pair(*seed).verification_key.as_slice().to_vec()
    }
    fn sign(&self, seed: &[u8; 32], msg: &[u8]) -> Vec<u8> {
        let kp = libcrux_ml_dsa::ml_dsa_65::generate_key_pair(*seed);
        libcrux_ml_dsa::ml_dsa_65::sign(&kp.signing_key, msg, b"", random::<32>())
            .expect("sign")
            .as_slice()
            .to_vec()
    }
    fn verify(&self, pk: &[u8], msg: &[u8], sig: &[u8]) -> bool {
        let Ok(pk) = <[u8; 1952]>::try_from(pk) else { return false };
        let Ok(sig) = <[u8; 3309]>::try_from(sig) else { return false };
        let vk = libcrux_ml_dsa::ml_dsa_65::MLDSA65VerificationKey::new(pk);
        let sig = libcrux_ml_dsa::ml_dsa_65::MLDSA65Signature::new(sig);
        libcrux_ml_dsa::ml_dsa_65::verify(&vk, msg, b"", &sig).is_ok()
    }
    fn signer(&self, seed: &[u8; 32]) -> Box<dyn Fn(&[u8]) -> Vec<u8>> {
        let kp = libcrux_ml_dsa::ml_dsa_65::generate_key_pair(*seed);
        Box::new(move |m| {
            libcrux_ml_dsa::ml_dsa_65::sign(&kp.signing_key, m, b"", random::<32>())
                .expect("sign")
                .as_slice()
                .to_vec()
        })
    }
}

#[cfg(feature = "aws-lc")]
struct AwsMlDsa;

#[cfg(feature = "aws-lc")]
impl MlDsa for AwsMlDsa {
    fn name(&self) -> &'static str {
        "aws-lc-rs"
    }
    fn public(&self, seed: &[u8; 32]) -> Vec<u8> {
        use aws_lc_rs::signature::{KeyPair, ML_DSA_65_SIGNING, PqdsaKeyPair};
        PqdsaKeyPair::from_seed(&ML_DSA_65_SIGNING, seed).expect("aws ml-dsa").public_key().as_ref().to_vec()
    }
    fn sign(&self, seed: &[u8; 32], msg: &[u8]) -> Vec<u8> {
        (self.signer(seed))(msg)
    }
    fn verify(&self, pk: &[u8], msg: &[u8], sig: &[u8]) -> bool {
        use aws_lc_rs::signature::{ML_DSA_65, UnparsedPublicKey};
        UnparsedPublicKey::new(&ML_DSA_65, pk).verify(msg, sig).is_ok()
    }
    fn signer(&self, seed: &[u8; 32]) -> Box<dyn Fn(&[u8]) -> Vec<u8>> {
        use aws_lc_rs::signature::{ML_DSA_65_SIGNING, PqdsaKeyPair};
        let kp = PqdsaKeyPair::from_seed(&ML_DSA_65_SIGNING, seed).expect("aws ml-dsa");
        Box::new(move |m| {
            let mut sig = vec![0u8; 3309];
            let n = kp.sign(m, &mut sig).expect("aws sign");
            sig.truncate(n);
            sig
        })
    }
}

// ---------------------------------------------------------------------------------------------

/// Microseconds per call: runs `f` for at least `min_ms` (and at least 5 times).
fn time_us(min_ms: u64, mut f: impl FnMut()) -> f64 {
    f(); // warm up
    let start = Instant::now();
    let mut n = 0u64;
    while n < 5 || start.elapsed().as_millis() < min_ms as u128 {
        f();
        n += 1;
    }
    start.elapsed().as_secs_f64() * 1e6 / n as f64
}

fn main() {
    let quick = std::env::args().any(|a| a == "--quick");
    let min_ms = if quick { 100 } else { 600 };

    let xwings: Vec<Box<dyn XWing>> = vec![
        #[cfg(feature = "rustcrypto")]
        Box::new(RcXWing),
        #[cfg(feature = "libcrux")]
        Box::new(LcXWing),
        #[cfg(feature = "aws-lc")]
        Box::new(AwsXWing),
    ];
    let mldsas: Vec<Box<dyn MlDsa>> = vec![
        #[cfg(feature = "rustcrypto")]
        Box::new(RcMlDsa),
        #[cfg(feature = "libcrux")]
        Box::new(LcMlDsa),
        #[cfg(feature = "aws-lc")]
        Box::new(AwsMlDsa),
    ];

    let mut out = String::new();
    let _ = writeln!(
        out,
        "# S3 results: {} {} ({})\n",
        std::env::consts::ARCH,
        std::env::consts::OS,
        [
            #[cfg(feature = "rustcrypto")]
            "rustcrypto",
            #[cfg(feature = "libcrux")]
            "libcrux",
            #[cfg(feature = "aws-lc")]
            "aws-lc",
        ]
        .join(", ")
    );

    // -----------------------------------------------------------------------------------------
    // Interop: the same seed must produce interchangeable keys, ciphertexts and signatures.
    let mut failures = 0;
    let mut checks: Vec<(String, bool)> = Vec::new();
    let seed = random::<32>();
    let pk = xwings.iter().find_map(|x| x.public(&seed));
    if let Some(pk) = &pk {
        for a in &xwings {
            if let Some(p) = a.public(&seed) {
                checks.push((format!("X-Wing public key from seed: {} matches", a.name()), &p == pk));
            }
        }
        for enc in &xwings {
            for dec in &xwings {
                let (ct, ss) = enc.encaps(pk);
                if let Some(ss2) = dec.decaps(&seed, &ct) {
                    checks.push((format!("X-Wing: {} encapsulates, {} decapsulates", enc.name(), dec.name()), ss == ss2 && ct.len() == XWING_CT));
                }
            }
        }
        // The full wrap: seal a key with one library's KEM, open it with another's.
        if let (Some(enc), Some(dec)) = (xwings.first(), xwings.last()) {
            let secret = random::<32>();
            let (ct, ss) = enc.encaps(pk);
            let sealed = seal(&wrap_key(&ss, &ct, pk), &secret, b"album 42");
            let opened = dec
                .decaps(&seed, &ct)
                .and_then(|ss2| open(&wrap_key(&ss2, &ct, pk), &sealed, b"album 42"));
            checks.push((format!("Key wrap: sealed via {}, opened via {}", enc.name(), dec.name()), opened.as_deref() == Some(&secret[..])));
            let tampered = open(&wrap_key(&ss, &ct, pk), &sealed, b"album 43");
            checks.push(("Key wrap: wrong associated data is rejected".into(), tampered.is_none()));
        }
    }
    let dsa_seed = random::<32>();
    let msg = b"ursa manifest v1: stripes, keys, renditions";
    if let Some(first) = mldsas.first() {
        let pk = first.public(&dsa_seed);
        for a in &mldsas {
            checks.push((format!("ML-DSA-65 public key from seed: {} matches", a.name()), a.public(&dsa_seed) == pk));
        }
        for s in &mldsas {
            let sig = s.sign(&dsa_seed, msg);
            for v in &mldsas {
                checks.push((format!("ML-DSA-65: {} signs, {} verifies", s.name(), v.name()), v.verify(&pk, msg, &sig)));
            }
            checks.push((format!("ML-DSA-65: {} signature rejected on another message", s.name()), !first.verify(&pk, b"something else", &sig)));
        }
    }
    let _ = writeln!(out, "## Interoperability\n\n| Check | Result |\n| --- | --- |");
    for (name, ok) in &checks {
        if !ok {
            failures += 1;
        }
        let _ = writeln!(out, "| {name} | {} |", if *ok { "pass" } else { "**FAIL**" });
    }

    // -----------------------------------------------------------------------------------------
    // Timings
    let _ = writeln!(out, "\n## X-Wing (µs per operation)\n\nWrap = encapsulate + HKDF + AES-256-GCM over a 32-byte key; unwrap = decapsulate from the seed + HKDF + open.\n\n| Library | Public key from seed | Encapsulate | Decapsulate | Wrap a key | Unwrap a key |\n| --- | ---: | ---: | ---: | ---: | ---: |");
    let seed = random::<32>();
    if let Some(pk) = xwings.iter().find_map(|x| x.public(&seed)) {
        let secret = random::<32>();
        for x in &xwings {
            let kg = x.public(&seed).map(|_| time_us(min_ms, || {
                black_box(x.public(black_box(&seed)));
            }));
            let enc = time_us(min_ms, || {
                black_box(x.encaps(&pk));
            });
            let (ct, _) = x.encaps(&pk);
            let dec = x.decaps(&seed, &ct).map(|_| time_us(min_ms, || {
                black_box(x.decaps(&seed, &ct));
            }));
            let wrap = time_us(min_ms, || {
                let (ct, ss) = x.encaps(&pk);
                black_box(seal(&wrap_key(&ss, &ct, &pk), &secret, b"album"));
            });
            let (ct, ss) = x.encaps(&pk);
            let sealed = seal(&wrap_key(&ss, &ct, &pk), &secret, b"album");
            let unwrap = x.decaps(&seed, &ct).map(|_| time_us(min_ms, || {
                let ss = x.decaps(&seed, &ct).unwrap();
                black_box(open(&wrap_key(&ss, &ct, &pk), &sealed, b"album"));
            }));
            let f = |v: Option<f64>| v.map_or("n/a".to_string(), |v| format!("{v:.0}"));
            let _ = writeln!(out, "| {} | {} | {enc:.0} | {} | {wrap:.0} | {} |", x.name(), f(kg), f(dec), f(unwrap));
        }
    }

    let _ = writeln!(out, "\n## ML-DSA-65 and Ed25519 (µs per operation)\n\nSign from a key already expanded in memory (a long-lived signer); 'sign from seed' includes key expansion.\n\n| Library | Key from seed | Sign | Sign from seed | Verify |\n| --- | ---: | ---: | ---: | ---: |");
    let seed = random::<32>();
    for d in &mldsas {
        let kg = time_us(min_ms, || {
            black_box(d.public(black_box(&seed)));
        });
        let signer = d.signer(&seed);
        let sign = time_us(min_ms, || {
            black_box(signer(black_box(msg)));
        });
        let sign_seed = time_us(min_ms, || {
            black_box(d.sign(&seed, black_box(msg)));
        });
        let pk = d.public(&seed);
        let sig = signer(msg);
        let verify = time_us(min_ms, || {
            black_box(d.verify(&pk, black_box(msg), &sig));
        });
        let _ = writeln!(out, "| {} | {kg:.0} | {sign:.0} | {sign_seed:.0} | {verify:.0} |", d.name());
    }
    {
        use ed25519_dalek::{Signer, Verifier};
        let sk = ed25519_dalek::SigningKey::from_bytes(&random::<32>());
        let vk = sk.verifying_key();
        let kg = time_us(min_ms, || {
            black_box(ed25519_dalek::SigningKey::from_bytes(black_box(&seed)).verifying_key());
        });
        let sign = time_us(min_ms, || {
            black_box(sk.sign(black_box(msg)));
        });
        let sig = sk.sign(msg);
        let verify = time_us(min_ms, || {
            black_box(vk.verify(black_box(msg), &sig).is_ok());
        });
        let _ = writeln!(out, "| Ed25519 (ed25519-dalek) | {kg:.0} | {sign:.0} | {sign:.0} | {verify:.0} |");
    }

    let _ = writeln!(out, "\n## Sizes (bytes)\n\n| Thing | Size |\n| --- | ---: |\n| X-Wing private key (seed) | 32 |\n| X-Wing public key | {XWING_PK} (ML-KEM-768 {MLKEM768_PK} + X25519 32) |\n| X-Wing ciphertext | {XWING_CT} (ML-KEM-768 {MLKEM768_CT} + X25519 32) |\n| A wrapped 32-byte key | {} (ciphertext + 32 + 16 tag) |\n| ML-DSA-65 private key (seed) | 32 |\n| ML-DSA-65 public key | 1952 |\n| ML-DSA-65 signature | 3309 |\n| Hybrid signature (Ed25519 + ML-DSA-65) | 3373 |\n| Hybrid public key | 1984 |", XWING_CT + 48);

    print!("{out}");
    if failures > 0 {
        eprintln!("{failures} interoperability check(s) failed");
        std::process::exit(1);
    }
}
