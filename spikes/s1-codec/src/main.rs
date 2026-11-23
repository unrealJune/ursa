//! Spike S1: how fast can a device turn plaintext into placed shards, and back?
//!
//! Write path per stripe: encrypt (AES-256-GCM) → Reed–Solomon encode into k + m shards →
//! BLAKE3 every shard (its id). Read path, degraded on purpose: the first min(m, k) data shards
//! are "lost", so every read decodes: verify k shards' hashes → RS decode → decrypt.
//!
//! The data is random (media is incompressible). Stripes are encrypted as one unit here; the
//! real design encrypts per chunk, which costs the same per byte plus a 16-byte tag per chunk.
//!
//! Usage: s1-codec [--quick] [--json PATH]

use std::collections::HashMap;
use std::fmt::Write as _;
use std::hint::black_box;
use std::ops::{Add, AddAssign};
use std::time::{Duration, Instant};

use rayon::prelude::*;
use reed_solomon_simd::{ReedSolomonDecoder, ReedSolomonEncoder};
use serde::Serialize;

const MIB: usize = 1 << 20;
const GB: f64 = 1e9;
const SOURCE_BYTES: usize = 256 * MIB;
/// Encoded stripes kept for the read benchmark, reused cyclically.
const READ_POOL: usize = 4;

// ---------------------------------------------------------------------------------------------
// Ciphers

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
enum CipherKind {
    RingAes256Gcm,
    RustCryptoAes256Gcm,
    RingChaCha20Poly1305,
    RustCryptoXChaCha20Poly1305,
}

impl CipherKind {
    const ALL: [CipherKind; 4] = [
        CipherKind::RingAes256Gcm,
        CipherKind::RustCryptoAes256Gcm,
        CipherKind::RingChaCha20Poly1305,
        CipherKind::RustCryptoXChaCha20Poly1305,
    ];

    fn label(self) -> &'static str {
        match self {
            CipherKind::RingAes256Gcm => "AES-256-GCM (ring)",
            CipherKind::RustCryptoAes256Gcm => "AES-256-GCM (RustCrypto)",
            CipherKind::RingChaCha20Poly1305 => "ChaCha20-Poly1305 (ring)",
            CipherKind::RustCryptoXChaCha20Poly1305 => "XChaCha20-Poly1305 (RustCrypto)",
        }
    }
}

enum Cipher {
    Ring(ring::aead::LessSafeKey),
    Aes(Box<aes_gcm::Aes256Gcm>),
    XChaCha(Box<chacha20poly1305::XChaCha20Poly1305>),
}

impl Cipher {
    fn new(kind: CipherKind) -> Self {
        use aes_gcm::aead::KeyInit;
        let key = [7u8; 32];
        match kind {
            CipherKind::RingAes256Gcm => Cipher::Ring(ring::aead::LessSafeKey::new(
                ring::aead::UnboundKey::new(&ring::aead::AES_256_GCM, &key).unwrap(),
            )),
            CipherKind::RingChaCha20Poly1305 => Cipher::Ring(ring::aead::LessSafeKey::new(
                ring::aead::UnboundKey::new(&ring::aead::CHACHA20_POLY1305, &key).unwrap(),
            )),
            CipherKind::RustCryptoAes256Gcm => {
                Cipher::Aes(Box::new(aes_gcm::Aes256Gcm::new_from_slice(&key).unwrap()))
            }
            CipherKind::RustCryptoXChaCha20Poly1305 => Cipher::XChaCha(Box::new(
                chacha20poly1305::XChaCha20Poly1305::new_from_slice(&key).unwrap(),
            )),
        }
    }

    fn nonce12(index: u64) -> [u8; 12] {
        let mut n = [0u8; 12];
        n[..8].copy_from_slice(&index.to_le_bytes());
        n
    }

    fn nonce24(index: u64) -> [u8; 24] {
        let mut n = [0u8; 24];
        n[..8].copy_from_slice(&index.to_le_bytes());
        n
    }

    /// Encrypts `buf` in place and appends the tag.
    fn seal(&self, index: u64, buf: &mut Vec<u8>) {
        use aes_gcm::aead::AeadInPlace;
        match self {
            Cipher::Ring(key) => key
                .seal_in_place_append_tag(
                    ring::aead::Nonce::assume_unique_for_key(Self::nonce12(index)),
                    ring::aead::Aad::empty(),
                    buf,
                )
                .unwrap(),
            Cipher::Aes(c) => c
                .encrypt_in_place(aes_gcm::Nonce::from_slice(&Self::nonce12(index)), b"", buf)
                .unwrap(),
            Cipher::XChaCha(c) => c
                .encrypt_in_place(
                    chacha20poly1305::XNonce::from_slice(&Self::nonce24(index)),
                    b"",
                    buf,
                )
                .unwrap(),
        }
    }

    /// Decrypts `buf` in place, checks and strips the tag. Panics on a bad tag.
    fn open(&self, index: u64, buf: &mut Vec<u8>) {
        use aes_gcm::aead::AeadInPlace;
        match self {
            Cipher::Ring(key) => {
                let len = key
                    .open_in_place(
                        ring::aead::Nonce::assume_unique_for_key(Self::nonce12(index)),
                        ring::aead::Aad::empty(),
                        buf,
                    )
                    .expect("tag")
                    .len();
                buf.truncate(len);
            }
            Cipher::Aes(c) => c
                .decrypt_in_place(aes_gcm::Nonce::from_slice(&Self::nonce12(index)), b"", buf)
                .expect("tag"),
            Cipher::XChaCha(c) => c
                .decrypt_in_place(
                    chacha20poly1305::XNonce::from_slice(&Self::nonce24(index)),
                    b"",
                    buf,
                )
                .expect("tag"),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Codes

#[derive(Clone, Copy, Debug, Serialize)]
struct Code {
    k: usize,
    n: usize,
}

impl Code {
    const fn new(k: usize, n: usize) -> Self {
        Code { k, n }
    }
    fn m(self) -> usize {
        self.n - self.k
    }
    fn is_replication(self) -> bool {
        self.k == 1
    }
    fn label(self) -> String {
        if self.is_replication() {
            format!("{}× copies", self.n)
        } else {
            format!("{}-of-{}", self.k, self.n)
        }
    }
    /// Bytes stored (and uploaded) per byte of input.
    fn overhead(self) -> f64 {
        self.n as f64 / self.k as f64
    }
}

const CODES: [Code; 5] = [
    Code::new(1, 3),
    Code::new(3, 5),
    Code::new(4, 6),
    Code::new(6, 9),
    Code::new(10, 14),
];

/// Shard size for a ciphertext of `len` bytes cut into `k` shards. reed-solomon-simd wants an
/// even size; 64 keeps every shard SIMD-aligned for a negligible amount of padding.
fn shard_bytes(len: usize, k: usize) -> usize {
    len.div_ceil(k).div_ceil(64) * 64
}

// ---------------------------------------------------------------------------------------------
// Stage timing

#[derive(Clone, Copy, Default, Debug, Serialize)]
struct Stages {
    encrypt: Duration,
    code: Duration,
    hash: Duration,
    verify: Duration,
    decode: Duration,
    decrypt: Duration,
}

impl Add for Stages {
    type Output = Stages;
    fn add(self, o: Stages) -> Stages {
        Stages {
            encrypt: self.encrypt + o.encrypt,
            code: self.code + o.code,
            hash: self.hash + o.hash,
            verify: self.verify + o.verify,
            decode: self.decode + o.decode,
            decrypt: self.decrypt + o.decrypt,
        }
    }
}

impl AddAssign for Stages {
    fn add_assign(&mut self, o: Stages) {
        *self = *self + o;
    }
}

impl Stages {
    fn total(&self) -> Duration {
        self.encrypt + self.code + self.hash + self.verify + self.decode + self.decrypt
    }
    fn shares(&self) -> String {
        let t = self.total().as_secs_f64().max(1e-12);
        let mut s = String::new();
        for (name, d) in [
            ("encrypt", self.encrypt),
            ("RS encode", self.code),
            ("hash", self.hash),
            ("verify", self.verify),
            ("RS decode", self.decode),
            ("decrypt", self.decrypt),
        ] {
            if d > Duration::ZERO {
                let _ = write!(s, "{name} {:.0}% · ", 100.0 * d.as_secs_f64() / t);
            }
        }
        s.trim_end_matches(" · ").to_string()
    }
}

fn timed<T>(slot: &mut Duration, f: impl FnOnce() -> T) -> T {
    let t = Instant::now();
    let r = f();
    *slot += t.elapsed();
    r
}

// ---------------------------------------------------------------------------------------------
// Write path

struct Stored {
    index: u64,
    len: usize,
    shard_bytes: usize,
    shards: Vec<Vec<u8>>,
    hashes: Vec<blake3::Hash>,
}

#[derive(Default)]
struct WriteScratch {
    buf: Vec<u8>,
    recovery: Vec<Vec<u8>>,
    encoder: Option<ReedSolomonEncoder>,
}

fn write_stripe(
    cipher: &Cipher,
    code: Code,
    index: u64,
    plain: &[u8],
    s: &mut WriteScratch,
    keep: bool,
    col: usize,
) -> (Stages, Option<Stored>) {
    let mut st = Stages::default();

    timed(&mut st.encrypt, || {
        s.buf.clear();
        s.buf.extend_from_slice(plain);
        cipher.seal(index, &mut s.buf);
    });
    let len = s.buf.len();

    if code.is_replication() {
        // Copies are identical: one hash names them all.
        let h = timed(&mut st.hash, || blake3::hash(&s.buf));
        let stored = keep.then(|| Stored {
            index,
            len,
            shard_bytes: len,
            shards: vec![s.buf.clone()],
            hashes: vec![h],
        });
        black_box(h);
        return (st, stored);
    }

    let sb = shard_bytes(len, code.k);
    let col = column(col, sb);
    let m = code.m();
    let mut hashes = Vec::with_capacity(code.n);
    {
        let WriteScratch { buf, recovery, encoder } = s;
        buf.resize(code.k * sb, 0);
        recovery.resize_with(m, Vec::new);
        for r in recovery.iter_mut() {
            r.resize(sb, 0);
        }
        // Columns are independent, so the stripe is coded one column slice at a time: the
        // encoder's working set is (k + m) × col, not (k + m) × shard.
        let t = Instant::now();
        let mut off = 0;
        while off < sb {
            let w = col.min(sb - off);
            match encoder {
                Some(e) => e.reset(code.k, m, w).unwrap(),
                None => *encoder = Some(ReedSolomonEncoder::new(code.k, m, w).unwrap()),
            }
            let enc = encoder.as_mut().unwrap();
            for i in 0..code.k {
                enc.add_original_shard(&buf[i * sb + off..i * sb + off + w]).unwrap();
            }
            let result = enc.encode().unwrap();
            for (r, slice) in recovery.iter_mut().zip(result.recovery_iter()) {
                r[off..off + w].copy_from_slice(slice);
            }
            off += w;
        }
        st.code += t.elapsed();

        let t = Instant::now();
        for shard in buf.chunks_exact(sb).chain(recovery.iter().map(Vec::as_slice)) {
            hashes.push(blake3::hash(shard));
        }
        st.hash += t.elapsed();
    }

    let stored = keep.then(|| {
        let mut shards: Vec<Vec<u8>> = s.buf.chunks_exact(sb).map(<[u8]>::to_vec).collect();
        shards.extend(s.recovery.iter().cloned());
        Stored { index, len, shard_bytes: sb, shards, hashes: hashes.clone() }
    });
    black_box(&hashes);
    (st, stored)
}

/// Column slice width: `col` bytes (a multiple of 64), or the whole shard when `col` is 0.
fn column(col: usize, sb: usize) -> usize {
    if col == 0 { sb } else { col.min(sb) }
}

// ---------------------------------------------------------------------------------------------
// Read path. Degraded: the first min(m, k) data shards are gone, so every read decodes.
// Healthy: all k data shards answer, so a read is verify + concatenate + decrypt.

#[derive(Default)]
struct ReadScratch {
    buf: Vec<u8>,
    decoder: Option<ReedSolomonDecoder>,
}

fn read_stripe(
    cipher: &Cipher,
    code: Code,
    stored: &Stored,
    s: &mut ReadScratch,
    col: usize,
    degraded: bool,
) -> Stages {
    let mut st = Stages::default();

    if code.is_replication() {
        let shard = &stored.shards[0];
        timed(&mut st.verify, || assert_eq!(blake3::hash(shard), stored.hashes[0]));
        timed(&mut st.decrypt, || {
            s.buf.clear();
            s.buf.extend_from_slice(shard);
            cipher.open(stored.index, &mut s.buf);
        });
        black_box(&s.buf);
        return st;
    }

    let lost = if degraded { code.m().min(code.k) } else { 0 };
    // Exactly k survivors: data shards lost..k, then recovery shards 0..lost.
    let available: Vec<usize> = (lost..code.n).take(code.k).collect();

    timed(&mut st.verify, || {
        for &i in &available {
            assert_eq!(blake3::hash(&stored.shards[i]), stored.hashes[i]);
        }
    });

    let ReadScratch { buf, decoder } = s;
    let sb = stored.shard_bytes;
    let t = Instant::now();
    buf.clear();
    buf.resize(code.k * sb, 0);
    for &i in available.iter().filter(|&&i| i < code.k) {
        buf[i * sb..(i + 1) * sb].copy_from_slice(&stored.shards[i]);
    }
    if lost > 0 {
        let col = column(col, sb);
        let mut off = 0;
        while off < sb {
            let w = col.min(sb - off);
            match decoder {
                Some(d) => d.reset(code.k, code.m(), w).unwrap(),
                None => *decoder = Some(ReedSolomonDecoder::new(code.k, code.m(), w).unwrap()),
            }
            let dec = decoder.as_mut().unwrap();
            for &i in &available {
                let slice = &stored.shards[i][off..off + w];
                if i < code.k {
                    dec.add_original_shard(i, slice).unwrap();
                } else {
                    dec.add_recovery_shard(i - code.k, slice).unwrap();
                }
            }
            let result = dec.decode().unwrap();
            for (i, slice) in result.restored_original_iter() {
                buf[i * sb + off..i * sb + off + w].copy_from_slice(slice);
            }
            off += w;
        }
    }
    buf.truncate(stored.len);
    st.decode += t.elapsed();

    timed(&mut st.decrypt, || cipher.open(stored.index, buf));
    black_box(&buf);
    st
}

// ---------------------------------------------------------------------------------------------
// Runs

#[derive(Serialize)]
struct Record {
    bench: &'static str,
    op: &'static str,
    cipher: CipherKind,
    code: String,
    stripe_mib: usize,
    /// Column slice the RS coder works on, KiB (0 = whole shard).
    column_kib: usize,
    threads: usize,
    bytes: usize,
    seconds: f64,
    gb_per_s: f64,
    /// Bytes uploaded or stored per byte of input.
    overhead: f64,
    stages: Stages,
}

struct Bench {
    source: Vec<u8>,
    records: Vec<Record>,
    pools: HashMap<usize, rayon::ThreadPool>,
    /// Column slice for RS encode/decode, bytes (0 = whole shard).
    col: usize,
}

impl Bench {
    fn pool(&mut self, threads: usize) -> &rayon::ThreadPool {
        self.pools.entry(threads).or_insert_with(|| {
            rayon::ThreadPoolBuilder::new().num_threads(threads).build().unwrap()
        })
    }

    fn plain(&self, stripe: usize, i: usize) -> &[u8] {
        let slots = (self.source.len() - stripe) / stripe + 1;
        let off = (i % slots) * stripe;
        &self.source[off..off + stripe]
    }

    #[allow(clippy::too_many_arguments)]
    fn write(
        &mut self,
        bench: &'static str,
        kind: CipherKind,
        code: Code,
        stripe: usize,
        threads: usize,
        total: usize,
    ) -> &Record {
        let cipher = Cipher::new(kind);
        let count = (total / stripe).max(1);
        // Warm up allocators, caches and the encoder tables.
        let mut warm = WriteScratch::default();
        for i in 0..2 {
            black_box(write_stripe(&cipher, code, i, self.plain(stripe, i as usize), &mut warm, false, self.col));
        }
        let this = &*self;
        let col = this.col;
        let pool = &this.pools[&threads];
        let t = Instant::now();
        let stages = pool.install(|| {
            (0..count)
                .into_par_iter()
                .map_init(WriteScratch::default, |s, i| {
                    write_stripe(&cipher, code, i as u64, this.plain(stripe, i), s, false, col).0
                })
                .reduce(Stages::default, Add::add)
        });
        let seconds = t.elapsed().as_secs_f64();
        self.push(bench, "write", kind, code, stripe, threads, count * stripe, seconds, stages)
    }

    #[allow(clippy::too_many_arguments)]
    fn read(
        &mut self,
        bench: &'static str,
        kind: CipherKind,
        code: Code,
        stripe: usize,
        threads: usize,
        total: usize,
        degraded: bool,
    ) -> &Record {
        let cipher = Cipher::new(kind);
        let count = (total / stripe).max(1);
        let col = self.col;
        let mut ws = WriteScratch::default();
        let stored: Vec<Stored> = (0..READ_POOL)
            .map(|i| {
                write_stripe(&cipher, code, i as u64, self.plain(stripe, i), &mut ws, true, col)
                    .1
                    .unwrap()
            })
            .collect();
        let mut warm = ReadScratch::default();
        black_box(read_stripe(&cipher, code, &stored[0], &mut warm, col, degraded));
        let pool = &self.pools[&threads];
        let t = Instant::now();
        let stages = pool.install(|| {
            (0..count)
                .into_par_iter()
                .map_init(ReadScratch::default, |s, i| {
                    read_stripe(&cipher, code, &stored[i % READ_POOL], s, col, degraded)
                })
                .reduce(Stages::default, Add::add)
        });
        let seconds = t.elapsed().as_secs_f64();
        let op = if degraded { "read-degraded" } else { "read-healthy" };
        self.push(bench, op, kind, code, stripe, threads, count * stripe, seconds, stages)
    }

    #[allow(clippy::too_many_arguments)]
    fn push(
        &mut self,
        bench: &'static str,
        op: &'static str,
        cipher: CipherKind,
        code: Code,
        stripe: usize,
        threads: usize,
        bytes: usize,
        seconds: f64,
        stages: Stages,
    ) -> &Record {
        self.records.push(Record {
            bench,
            op,
            cipher,
            code: code.label(),
            stripe_mib: stripe / MIB,
            column_kib: self.col / 1024,
            threads,
            bytes,
            seconds,
            gb_per_s: bytes as f64 / seconds / GB,
            overhead: code.overhead(),
            stages,
        });
        self.records.last().unwrap()
    }
}

/// Seals (or opens) `total` bytes in `stripe`-sized pieces with no coding.
fn cipher_only(b: &mut Bench, kind: CipherKind, stripe: usize, threads: usize, total: usize, open: bool) -> f64 {
    let cipher = Cipher::new(kind);
    let count = (total / stripe).max(1);
    let sealed: Vec<Vec<u8>> = (0..READ_POOL)
        .map(|i| {
            let mut v = b.plain(stripe, i).to_vec();
            cipher.seal(i as u64, &mut v);
            v
        })
        .collect();
    b.pool(threads);
    let this = &*b;
    let pool = &this.pools[&threads];
    let t = Instant::now();
    pool.install(|| {
        (0..count).into_par_iter().for_each_init(Vec::new, |buf: &mut Vec<u8>, i| {
            buf.clear();
            if open {
                buf.extend_from_slice(&sealed[i % READ_POOL]);
                cipher.open((i % READ_POOL) as u64, buf);
            } else {
                buf.extend_from_slice(this.plain(stripe, i));
                cipher.seal(i as u64, buf);
            }
            black_box(&buf);
        })
    });
    (count * stripe) as f64 / t.elapsed().as_secs_f64() / GB
}

fn hash_only(b: &Bench, stripe: usize, total: usize) -> f64 {
    let count = (total / stripe).max(1);
    let t = Instant::now();
    for i in 0..count {
        black_box(blake3::hash(b.plain(stripe, i)));
    }
    (count * stripe) as f64 / t.elapsed().as_secs_f64() / GB
}

/// FastCDC over the source, single thread (chunking one file is sequential).
fn cdc_only(b: &Bench, avg: u32, total: usize) -> (f64, usize) {
    let (min, max) = (avg / 4, avg * 4);
    let mut bytes = 0;
    let mut chunks = 0;
    let t = Instant::now();
    while bytes < total {
        for c in fastcdc::v2020::FastCDC::new(&b.source, min, avg, max) {
            chunks += 1;
            black_box(c.hash);
        }
        bytes += b.source.len();
    }
    (bytes as f64 / t.elapsed().as_secs_f64() / GB, bytes / chunks.max(1))
}

// ---------------------------------------------------------------------------------------------
// Platform description

fn cpu_name() -> String {
    #[cfg(target_os = "linux")]
    if let Ok(s) = std::fs::read_to_string("/proc/cpuinfo") {
        for key in ["model name", "Hardware", "CPU part"] {
            if let Some(l) = s.lines().find(|l| l.starts_with(key)) {
                return l.split(':').nth(1).unwrap_or("").trim().to_string();
            }
        }
    }
    #[cfg(target_os = "macos")]
    if let Ok(o) = std::process::Command::new("sysctl").args(["-n", "machdep.cpu.brand_string"]).output() {
        return String::from_utf8_lossy(&o.stdout).trim().to_string();
    }
    #[cfg(target_os = "windows")]
    if let Ok(s) = std::env::var("PROCESSOR_IDENTIFIER") {
        return s;
    }
    "unknown".into()
}

fn crypto_features() -> String {
    let mut f = Vec::new();
    #[cfg(target_arch = "x86_64")]
    {
        for (n, on) in [
            ("aes", std::arch::is_x86_feature_detected!("aes")),
            ("pclmulqdq", std::arch::is_x86_feature_detected!("pclmulqdq")),
            ("avx2", std::arch::is_x86_feature_detected!("avx2")),
            ("avx512f", std::arch::is_x86_feature_detected!("avx512f")),
            ("vaes", std::arch::is_x86_feature_detected!("vaes")),
        ] {
            if on {
                f.push(n);
            }
        }
    }
    #[cfg(target_arch = "aarch64")]
    {
        for (n, on) in [
            ("aes", std::arch::is_aarch64_feature_detected!("aes")),
            ("pmull", std::arch::is_aarch64_feature_detected!("pmull")),
            ("neon", std::arch::is_aarch64_feature_detected!("neon")),
            ("sha3", std::arch::is_aarch64_feature_detected!("sha3")),
        ] {
            if on {
                f.push(n);
            }
        }
    }
    f.join(" ")
}

// ---------------------------------------------------------------------------------------------

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let quick = args.iter().any(|a| a == "--quick");
    let json_path = args.iter().position(|a| a == "--json").and_then(|i| args.get(i + 1)).cloned();

    let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
    let (total, headline_total) = if quick { (256 * MIB, 1024 * MIB) } else { (1024 * MIB, 4096 * MIB) };
    let stripes_mib: &[usize] = &[1, 4, 8, 16, 32];
    let headline_threads: Vec<usize> = {
        let mut t = vec![1, 2, 4, cores];
        t.retain(|&n| n <= cores);
        t.dedup();
        t
    };

    let mut source = vec![0u8; SOURCE_BYTES];
    blake3::Hasher::new().update(b"ursa s1").finalize_xof().fill(&mut source);
    let mut b = Bench { source, records: Vec::new(), pools: HashMap::new(), col: 0 };
    for &t in &headline_threads {
        b.pool(t);
    }

    let mut out = String::new();
    let _ = writeln!(out, "# S1 results: {} · {} · {} threads", std::env::consts::ARCH, cpu_name(), cores);
    let _ = writeln!(out, "\nOS `{}`, crypto features: `{}`. GB/s is plaintext bytes (10^9) per second.", std::env::consts::OS, crypto_features());
    if quick {
        let _ = writeln!(out, "\n**Quick run** (smaller totals; noisier).");
    }

    // 1. Ciphers
    let _ = writeln!(out, "\n## Ciphers (8 MiB pieces)\n\n| Cipher | seal 1T | open 1T | seal {cores}T | open {cores}T |\n| --- | ---: | ---: | ---: | ---: |");
    for kind in CipherKind::ALL {
        let s1 = cipher_only(&mut b, kind, 8 * MIB, 1, total, false);
        let o1 = cipher_only(&mut b, kind, 8 * MIB, 1, total, true);
        let sn = cipher_only(&mut b, kind, 8 * MIB, cores, total, false);
        let on = cipher_only(&mut b, kind, 8 * MIB, cores, total, true);
        let _ = writeln!(out, "| {} | {s1:.2} | {o1:.2} | {sn:.2} | {on:.2} |", kind.label());
        eprintln!("cipher {} done", kind.label());
    }

    // 2. Hashing and chunking
    let h = hash_only(&b, 8 * MIB, total);
    let _ = writeln!(out, "\n## BLAKE3 and FastCDC (one thread)\n\n| Stage | GB/s | Note |\n| --- | ---: | --- |\n| BLAKE3, 8 MiB pieces | {h:.2} | |");
    for avg in [256 * 1024u32, 1024 * 1024, 4 * 1024 * 1024] {
        let (g, mean) = cdc_only(&b, avg, total);
        let _ = writeln!(out, "| FastCDC avg {} KiB | {g:.2} | mean chunk {} KiB |", avg / 1024, mean / 1024);
    }
    eprintln!("hash + cdc done");

    // 3. Column slicing: RS columns are independent, so code a stripe in slices.
    let kind = CipherKind::RingAes256Gcm;
    let code = Code::new(4, 6);
    let stripe = 8 * MIB;
    let _ = writeln!(out, "
## RS column slicing (4-of-6, 8 MiB stripes, {})

The coder works on one column slice of every shard at a time, so its working set is (k + m) × slice instead of (k + m) × shard.

| Slice | write 1T | degraded read 1T | write {cores}T | degraded read {cores}T |
| --- | ---: | ---: | ---: | ---: |", kind.label());
    let mut best = (0usize, 0.0f64);
    for col in [0usize, 1024 * 1024, 256 * 1024, 64 * 1024, 16 * 1024, 4 * 1024] {
        b.col = col;
        let w1 = b.write("column", kind, code, stripe, 1, total).gb_per_s;
        let r1 = b.read("column", kind, code, stripe, 1, total, true).gb_per_s;
        let wn = b.write("column", kind, code, stripe, cores, total).gb_per_s;
        let rn = b.read("column", kind, code, stripe, cores, total, true).gb_per_s;
        let label = if col == 0 { "whole shard".to_string() } else { format!("{} KiB", col / 1024) };
        let _ = writeln!(out, "| {label} | {w1:.2} | {r1:.2} | {wn:.2} | {rn:.2} |");
        if r1 + rn > best.1 {
            best = (col, r1 + rn);
        }
    }
    b.col = best.0;
    let col_label = if b.col == 0 { "whole shard".to_string() } else { format!("{} KiB slices", b.col / 1024) };
    let _ = writeln!(out, "
Best for degraded reads: **{col_label}**, used below.");
    eprintln!("column sweep done: {col_label}");

    // 4. Pipeline matrix
    let _ = writeln!(out, "
## Pipeline: stripe size × code ({}, {col_label})

Write = encrypt + RS encode + hash every shard. Healthy read = verify k data shards + decrypt. Degraded read = verify k shards + RS decode with the first min(m, k) data shards lost + decrypt. Overhead = bytes uploaded and stored per input byte.

| Code | Overhead | Stripe | write 1T | healthy 1T | degraded 1T | write {cores}T | healthy {cores}T | degraded {cores}T |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |", kind.label());
    for code in CODES {
        for &mib in stripes_mib {
            let stripe = mib * MIB;
            let w1 = b.write("matrix", kind, code, stripe, 1, total).gb_per_s;
            let h1 = b.read("matrix", kind, code, stripe, 1, total, false).gb_per_s;
            let d1 = b.read("matrix", kind, code, stripe, 1, total, true).gb_per_s;
            let wn = b.write("matrix", kind, code, stripe, cores, total).gb_per_s;
            let hn = b.read("matrix", kind, code, stripe, cores, total, false).gb_per_s;
            let dn = b.read("matrix", kind, code, stripe, cores, total, true).gb_per_s;
            let _ = writeln!(out, "| {} | {:.2}× | {mib} MiB | {w1:.2} | {h1:.2} | {d1:.2} | {wn:.2} | {hn:.2} | {dn:.2} |", code.label(), code.overhead());
        }
        eprintln!("matrix {} done", code.label());
    }

    // 5. Headline: a 4 GB video, 4-of-6, 8 MiB stripes
    let code = Code::new(4, 6);
    let _ = writeln!(out, "
## Headline: {} GiB through 4-of-6, 8 MiB stripes, {}, {col_label}

| Threads | write GB/s | healthy read GB/s | degraded read GB/s | write time | where time goes: write / degraded read |
| ---: | ---: | ---: | ---: | ---: | --- |", headline_total / (1024 * MIB), kind.label());
    for &t in &headline_threads {
        let w = b.write("headline", kind, code, stripe, t, headline_total);
        let (wg, ws, wst) = (w.gb_per_s, w.seconds, w.stages);
        let hg = b.read("headline", kind, code, stripe, t, headline_total, false).gb_per_s;
        let r = b.read("headline", kind, code, stripe, t, headline_total, true);
        let (rg, rst) = (r.gb_per_s, r.stages);
        let _ = writeln!(out, "| {t} | {wg:.2} | {hg:.2} | {rg:.2} | {ws:.1} s | {} / {} |", wst.shares(), rst.shares());
    }

    println!("{out}");
    if let Some(p) = json_path {
        let doc = serde_json::json!({
            "arch": std::env::consts::ARCH,
            "os": std::env::consts::OS,
            "cpu": cpu_name(),
            "cores": cores,
            "features": crypto_features(),
            "quick": quick,
            "records": b.records,
        });
        std::fs::write(&p, serde_json::to_string_pretty(&doc).unwrap()).unwrap();
    }
}
