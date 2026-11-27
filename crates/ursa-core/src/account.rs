//! The account: everything derived from the 32-byte root, and what proves things about it.
//!
//! | Derived from the root            | For                                                    |
//! | -------------------------------- | ------------------------------------------------------ |
//! | Ed25519 + ML-DSA-65 signing keys | device certificates and account heads (hybrid: both)   |
//! | account id = the Ed25519 public  | what nodes and friends know the account by             |
//! | locator key (separate Ed25519)   | the account's pkarr record, unlinkable to the account  |
//! | X-Wing key                       | receiving wrapped keys                                 |
//! | head key                         | sealing the account head's payload                     |
//!
//! Because all of it is derived, the root alone (a recovery phrase or key file) restores the
//! account on a new device.

use std::fmt;

use ed25519_dalek::{Signer, Verifier};
use serde::{Deserialize, Serialize};

use crate::crypto::AccountKey;
use crate::{Error, Result};

const ML_DSA_PK: usize = 1952;
const ML_DSA_SIG: usize = 3309;

/// An account id: the account's Ed25519 public key, shown as hex.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AccountId(pub [u8; 32]);

impl AccountId {
    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }

    /// The first 10 hex characters, for people to compare by eye.
    pub fn short(&self) -> String {
        self.to_hex()[..10].to_string()
    }
}

impl fmt::Display for AccountId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_hex())
    }
}

impl fmt::Debug for AccountId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "AccountId({})", self.short())
    }
}

impl std::str::FromStr for AccountId {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self> {
        let mut b = [0u8; 32];
        hex::decode_to_slice(s.trim(), &mut b).map_err(|_| Error::Malformed("account id"))?;
        Ok(Self(b))
    }
}

impl Serialize for AccountId {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for AccountId {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        String::deserialize(d)?.parse().map_err(serde::de::Error::custom)
    }
}

/// What anyone may know about an account: its public keys.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountPublic {
    pub id: AccountId,
    /// ML-DSA-65 verification key, base64.
    #[serde(with = "b64")]
    pub ml_dsa: Vec<u8>,
    /// X-Wing encapsulation key, base64.
    #[serde(with = "b64")]
    pub xwing: Vec<u8>,
}

/// A hybrid signature: Ed25519 and ML-DSA-65 over the same domain-separated message. Both must
/// verify; forging one is not enough.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HybridSig {
    #[serde(with = "b64")]
    pub ed25519: Vec<u8>,
    #[serde(with = "b64")]
    pub ml_dsa: Vec<u8>,
}

fn domain(context: &str, msg: &[u8]) -> Vec<u8> {
    let mut m = Vec::with_capacity(context.len() + 1 + msg.len());
    m.extend_from_slice(context.as_bytes());
    m.push(0);
    m.extend_from_slice(msg);
    m
}

impl AccountPublic {
    /// Checks a hybrid signature over `msg` in `context`.
    pub fn verify(&self, context: &str, msg: &[u8], sig: &HybridSig) -> bool {
        let m = domain(context, msg);
        let Ok(ed_pk) = ed25519_dalek::VerifyingKey::from_bytes(&self.id.0) else { return false };
        let Ok(ed_sig) = <[u8; 64]>::try_from(sig.ed25519.as_slice()) else { return false };
        if ed_pk.verify(&m, &ed25519_dalek::Signature::from_bytes(&ed_sig)).is_err() {
            return false;
        }
        let (Ok(pk), Ok(s)) =
            (<[u8; ML_DSA_PK]>::try_from(self.ml_dsa.as_slice()), <[u8; ML_DSA_SIG]>::try_from(sig.ml_dsa.as_slice()))
        else {
            return false;
        };
        libcrux_ml_dsa::ml_dsa_65::verify(
            &libcrux_ml_dsa::ml_dsa_65::MLDSA65VerificationKey::new(pk),
            &m,
            b"",
            &libcrux_ml_dsa::ml_dsa_65::MLDSA65Signature::new(s),
        )
        .is_ok()
    }
}

/// An account, opened from its root.
pub struct Account {
    root: AccountKey,
    ed: ed25519_dalek::SigningKey,
    ml: libcrux_ml_dsa::ml_dsa_65::MLDSA65KeyPair,
    xwing_seed: [u8; 32],
    public: AccountPublic,
}

impl Account {
    pub fn generate() -> Self {
        Self::from_root(AccountKey::generate())
    }

    pub fn from_root(root: AccountKey) -> Self {
        let r = root.as_bytes();
        let ed = ed25519_dalek::SigningKey::from_bytes(&blake3::derive_key("ursa v1 account ed25519", r));
        let ml = libcrux_ml_dsa::ml_dsa_65::generate_key_pair(blake3::derive_key("ursa v1 account ml-dsa-65", r));
        let xwing_seed = blake3::derive_key("ursa v1 account x-wing", r);
        let (_, xpk) = libcrux_kem::key_gen_derand(libcrux_kem::Algorithm::XWingKemDraft06, &xwing_seed)
            .expect("x-wing from a 32-byte seed");
        let public = AccountPublic {
            id: AccountId(ed.verifying_key().to_bytes()),
            ml_dsa: ml.verification_key.as_slice().to_vec(),
            xwing: xpk.encode(),
        };
        Self { root, ed, ml, xwing_seed, public }
    }

    pub fn root(&self) -> &AccountKey {
        &self.root
    }

    pub fn id(&self) -> AccountId {
        self.public.id
    }

    pub fn public(&self) -> &AccountPublic {
        &self.public
    }

    /// The seed of the account's locator key: an Ed25519 key that signs its pkarr record. A
    /// separate key, so the record does not reveal the account id to whoever reads the DHT.
    pub fn locator_seed(&self) -> [u8; 32] {
        blake3::derive_key("ursa v1 account locator", self.root.as_bytes())
    }

    /// Key for the account head's sealed payload.
    pub(crate) fn head_key(&self) -> [u8; 32] {
        blake3::derive_key("ursa v1 account head", self.root.as_bytes())
    }

    pub(crate) fn xwing_seed(&self) -> &[u8; 32] {
        &self.xwing_seed
    }

    pub fn sign(&self, context: &str, msg: &[u8]) -> HybridSig {
        let m = domain(context, msg);
        let mut rnd = [0u8; 32];
        getrandom::fill(&mut rnd).expect("OS randomness");
        let ml = libcrux_ml_dsa::ml_dsa_65::sign(&self.ml.signing_key, &m, b"", rnd).expect("ml-dsa sign");
        HybridSig { ed25519: self.ed.sign(&m).to_bytes().to_vec(), ml_dsa: ml.as_slice().to_vec() }
    }
}

impl fmt::Debug for Account {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Account({})", self.id().short())
    }
}

// ---------------------------------------------------------------------------------------------
// Device certificates

/// "This device belongs to this account", signed by the account. Nodes accept a device's
/// requests on the account's behalf only when its certificate names the device's own endpoint
/// id and verifies against the account's keys.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceCert {
    pub v: u32,
    pub account: AccountPublic,
    /// The device's iroh endpoint id, hex.
    pub device: String,
    pub name: String,
    pub platform: String,
    pub issued_ms: i64,
    pub sig: HybridSig,
}

#[derive(Serialize)]
struct DeviceCertBody<'a> {
    v: u32,
    account: &'a AccountPublic,
    device: &'a str,
    name: &'a str,
    platform: &'a str,
    issued_ms: i64,
}

const DEVICE_CERT_CONTEXT: &str = "ursa device certificate v1";

impl DeviceCert {
    pub fn issue(account: &Account, device: &str, name: &str, platform: &str, issued_ms: i64) -> Self {
        let body = DeviceCertBody { v: 1, account: account.public(), device, name, platform, issued_ms };
        let sig = account.sign(DEVICE_CERT_CONTEXT, &serde_json::to_vec(&body).expect("json"));
        Self {
            v: 1,
            account: account.public().clone(),
            device: device.to_string(),
            name: name.to_string(),
            platform: platform.to_string(),
            issued_ms,
            sig,
        }
    }

    /// Checks the signature, and that the certificate is for `device`.
    pub fn verify_for(&self, device: &str) -> bool {
        if self.v != 1 || !self.device.eq_ignore_ascii_case(device) {
            return false;
        }
        let body = DeviceCertBody {
            v: self.v,
            account: &self.account,
            device: &self.device,
            name: &self.name,
            platform: &self.platform,
            issued_ms: self.issued_ms,
        };
        self.account.verify(DEVICE_CERT_CONTEXT, &serde_json::to_vec(&body).expect("json"), &self.sig)
    }
}

// ---------------------------------------------------------------------------------------------
// Recovery: a 24-word phrase, and a passphrase-sealed key file.

/// The root as 24 BIP-39 words (256 bits plus an 8-bit checksum).
pub fn recovery_phrase(root: &AccountKey) -> String {
    bip39::Mnemonic::from_entropy(root.as_bytes()).expect("32 bytes").to_string()
}

pub fn root_from_phrase(phrase: &str) -> Result<AccountKey> {
    let words = phrase.split_whitespace().map(str::to_lowercase).collect::<Vec<_>>().join(" ");
    let m =
        bip39::Mnemonic::parse_in(bip39::Language::English, &words).map_err(|_| Error::Malformed("recovery phrase"))?;
    let entropy = m.to_entropy();
    let bytes: [u8; 32] = entropy.as_slice().try_into().map_err(|_| Error::Malformed("recovery phrase length"))?;
    Ok(AccountKey::from_bytes(bytes))
}

/// The key file: the root sealed under a key stretched from a passphrase with Argon2id.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KeyFile {
    pub format: String,
    pub v: u32,
    /// The account it opens, so a person can tell key files apart.
    pub account: AccountId,
    pub kdf: String,
    pub m_kib: u32,
    pub t: u32,
    pub p: u32,
    #[serde(with = "b64")]
    pub salt: Vec<u8>,
    #[serde(with = "b64")]
    pub sealed_root: Vec<u8>,
}

const KEY_FILE_FORMAT: &str = "ursa-key";
const KEY_FILE_AAD: &[u8] = b"ursa key file v1";

impl KeyFile {
    /// Seals the root under `passphrase`. 64 MiB and 3 passes: about a second on a laptop.
    pub fn create(account: &Account, passphrase: &str) -> Result<Self> {
        Self::create_with(account, passphrase, 64 * 1024, 3)
    }

    pub fn create_with(account: &Account, passphrase: &str, m_kib: u32, t: u32) -> Result<Self> {
        let mut salt = [0u8; 16];
        getrandom::fill(&mut salt).expect("OS randomness");
        let key = stretch(passphrase, &salt, m_kib, t, 1)?;
        Ok(Self {
            format: KEY_FILE_FORMAT.into(),
            v: 1,
            account: account.id(),
            kdf: "argon2id".into(),
            m_kib,
            t,
            p: 1,
            salt: salt.to_vec(),
            sealed_root: crate::crypto::seal_with(&key, account.root().as_bytes(), KEY_FILE_AAD),
        })
    }

    pub fn open(&self, passphrase: &str) -> Result<Account> {
        if self.format != KEY_FILE_FORMAT || self.v != 1 || self.kdf != "argon2id" {
            return Err(Error::Malformed("key file"));
        }
        let key = stretch(passphrase, &self.salt, self.m_kib, self.t, self.p)?;
        let root = crate::crypto::open_with(&key, &self.sealed_root, KEY_FILE_AAD)?;
        let root: [u8; 32] = root.as_slice().try_into().map_err(|_| Error::Malformed("key file root"))?;
        let account = Account::from_root(AccountKey::from_bytes(root));
        if account.id() != self.account {
            return Err(Error::Malformed("key file account"));
        }
        Ok(account)
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("json")
    }

    pub fn from_json(s: &str) -> Result<Self> {
        serde_json::from_str(s).map_err(|_| Error::Malformed("key file"))
    }
}

fn stretch(passphrase: &str, salt: &[u8], m_kib: u32, t: u32, p: u32) -> Result<[u8; 32]> {
    let params = argon2::Params::new(m_kib, t, p, Some(32)).map_err(|_| Error::Malformed("argon2 parameters"))?;
    let a = argon2::Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params);
    let mut out = [0u8; 32];
    a.hash_password_into(passphrase.as_bytes(), salt, &mut out).map_err(|_| Error::Malformed("argon2"))?;
    Ok(out)
}

// ---------------------------------------------------------------------------------------------
// Sealing a key to a device (X-Wing, post-quantum), for linking.

/// Seals `secret` to an X-Wing public key: X-Wing encapsulate, then HKDF-style derive into
/// AES-256-GCM bound to `context`. Returns ciphertext ‖ sealed bytes.
pub fn seal_to(xwing_public: &[u8], secret: &[u8], context: &[u8]) -> Result<Vec<u8>> {
    let pk = libcrux_kem::PublicKey::decode(libcrux_kem::Algorithm::XWingKemDraft06, xwing_public)
        .map_err(|_| Error::Malformed("x-wing public key"))?;
    let mut rnd = [0u8; 64];
    getrandom::fill(&mut rnd).expect("OS randomness");
    let (ss, ct) = pk.encapsulate_derand(&rnd).map_err(|_| Error::Malformed("x-wing encapsulate"))?;
    let ct = ct.encode();
    let key = wrap_key(&ss.encode(), &ct, xwing_public, context);
    let mut out = ct;
    out.extend_from_slice(&crate::crypto::seal_with(&key, secret, context));
    Ok(out)
}

/// Opens what [`seal_to`] sealed, with the X-Wing seed of the receiving key.
pub fn open_sealed(xwing_seed: &[u8; 32], sealed: &[u8], context: &[u8]) -> Result<Vec<u8>> {
    const CT: usize = 1120;
    if sealed.len() < CT {
        return Err(Error::Malformed("sealed key"));
    }
    let (sk, pk) = libcrux_kem::key_gen_derand(libcrux_kem::Algorithm::XWingKemDraft06, xwing_seed)
        .map_err(|_| Error::Malformed("x-wing seed"))?;
    let ct = libcrux_kem::Ct::decode(libcrux_kem::Algorithm::XWingKemDraft06, &sealed[..CT])
        .map_err(|_| Error::Malformed("x-wing ciphertext"))?;
    let ss = ct.decapsulate(&sk).map_err(|_| Error::Decrypt)?;
    let key = wrap_key(&ss.encode(), &sealed[..CT], &pk.encode(), context);
    crate::crypto::open_with(&key, &sealed[CT..], context)
}

/// The public X-Wing key for a seed.
pub fn xwing_public(seed: &[u8; 32]) -> Vec<u8> {
    libcrux_kem::key_gen_derand(libcrux_kem::Algorithm::XWingKemDraft06, seed).expect("x-wing seed").1.encode()
}

fn wrap_key(ss: &[u8], ct: &[u8], pk: &[u8], context: &[u8]) -> [u8; 32] {
    let mut h = blake3::Hasher::new_derive_key("ursa v1 x-wing wrap");
    h.update(ss);
    h.update(blake3::hash(ct).as_bytes());
    h.update(blake3::hash(pk).as_bytes());
    h.update(context);
    *h.finalize().as_bytes()
}

impl Account {
    /// Opens something sealed to this account's own X-Wing key.
    pub fn open_sealed(&self, sealed: &[u8], context: &[u8]) -> Result<Vec<u8>> {
        open_sealed(self.xwing_seed(), sealed, context)
    }
}

pub(crate) mod b64 {
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(b: &[u8], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&STANDARD.encode(b))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        STANDARD.decode(String::deserialize(d)?).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_root_restores_the_same_account() {
        let a = Account::generate();
        let b = Account::from_root(a.root().clone());
        assert_eq!(a.public(), b.public());
        assert_eq!(a.locator_seed(), b.locator_seed());
    }

    #[test]
    fn hybrid_signatures_verify_and_reject() {
        let a = Account::generate();
        let sig = a.sign("test", b"hello");
        assert!(a.public().verify("test", b"hello", &sig));
        assert!(!a.public().verify("test", b"hellO", &sig));
        assert!(!a.public().verify("other context", b"hello", &sig));
        assert!(!Account::generate().public().verify("test", b"hello", &sig));
    }

    #[test]
    fn device_certificates_bind_the_device() {
        let a = Account::generate();
        let cert = DeviceCert::issue(&a, "abcd", "Laptop", "windows", 1);
        assert!(cert.verify_for("abcd"));
        assert!(!cert.verify_for("abce"));
        let mut forged = cert.clone();
        forged.name = "Evil".into();
        assert!(!forged.verify_for("abcd"));
        // Swapping in another account's ML-DSA key breaks the Ed25519 half.
        let mut swapped = cert.clone();
        swapped.account.ml_dsa = Account::generate().public().ml_dsa.clone();
        assert!(!swapped.verify_for("abcd"));
    }

    #[test]
    fn recovery_phrase_round_trips() {
        let a = Account::generate();
        let phrase = recovery_phrase(a.root());
        assert_eq!(phrase.split_whitespace().count(), 24);
        let b = Account::from_root(root_from_phrase(&phrase.to_uppercase()).unwrap());
        assert_eq!(a.id(), b.id());
        assert!(root_from_phrase("abandon abandon abandon").is_err());
    }

    #[test]
    fn key_file_round_trips_and_needs_the_passphrase() {
        let a = Account::generate();
        let kf = KeyFile::create_with(&a, "correct horse", 1024, 1).unwrap();
        let back = KeyFile::from_json(&kf.to_json()).unwrap();
        assert_eq!(back.open("correct horse").unwrap().id(), a.id());
        assert!(back.open("wrong").is_err());
    }

    #[test]
    fn sealing_to_an_account_round_trips() {
        let a = Account::generate();
        let sealed = seal_to(&a.public().xwing, b"album key", b"ctx").unwrap();
        assert_eq!(a.open_sealed(&sealed, b"ctx").unwrap(), b"album key");
        assert!(a.open_sealed(&sealed, b"other").is_err());
        assert!(Account::generate().open_sealed(&sealed, b"ctx").is_err());
    }
}
