//! Reed-Solomon erasure coding: a sealed chunk becomes `n` shards, any `k` of which rebuild it.
//!
//! Systematic (the first `k` shards are the ciphertext itself, cut in order), so a read with
//! every data shard present needs no decoding at all. Coded in 256 KiB column slices: columns
//! are independent, and slicing keeps the coder's working set in cache (S1: whole-shard decoding
//! stopped scaling past one thread).

use reed_solomon_simd::{ReedSolomonDecoder, ReedSolomonEncoder};
use serde::{Deserialize, Serialize};

use crate::{Error, Result};

const COLUMN: usize = 256 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Code {
    pub k: u16,
    pub n: u16,
}

impl Code {
    pub fn new(k: u16, n: u16) -> Result<Self> {
        if k == 0 || n < k || n > 32 {
            return Err(Error::Malformed("code"));
        }
        Ok(Self { k, n })
    }

    pub fn m(self) -> usize {
        (self.n - self.k) as usize
    }

    /// The shard size for `len` bytes: a multiple of 64, as the coder wants.
    pub fn shard_len(self, len: usize) -> usize {
        len.div_ceil(self.k as usize).max(1).div_ceil(64) * 64
    }
}

/// Splits `data` into `n` shards of equal length. Shards `0..k` are the data, padded with zeros.
pub fn encode(code: Code, data: &[u8]) -> Vec<Vec<u8>> {
    let (k, m) = (code.k as usize, code.m());
    let sb = code.shard_len(data.len());
    let mut shards: Vec<Vec<u8>> = (0..k)
        .map(|i| {
            let start = (i * sb).min(data.len());
            let end = ((i + 1) * sb).min(data.len());
            let mut s = data[start..end].to_vec();
            s.resize(sb, 0);
            s
        })
        .collect();
    if m == 0 {
        return shards;
    }
    let mut recovery = vec![vec![0u8; sb]; m];
    let mut off = 0;
    while off < sb {
        let w = COLUMN.min(sb - off);
        let mut enc = ReedSolomonEncoder::new(k, m, w).expect("encoder");
        for s in &shards {
            enc.add_original_shard(&s[off..off + w]).expect("shard");
        }
        let result = enc.encode().expect("encode");
        for (r, slice) in recovery.iter_mut().zip(result.recovery_iter()) {
            r[off..off + w].copy_from_slice(slice);
        }
        off += w;
    }
    shards.extend(recovery);
    shards
}

/// Rebuilds the original `len` bytes from any `k` shards (index, bytes).
pub fn decode(code: Code, len: usize, available: &[(usize, &[u8])]) -> Result<Vec<u8>> {
    let (k, m, n) = (code.k as usize, code.m(), code.n as usize);
    let sb = code.shard_len(len);
    let mut have: Vec<Option<&[u8]>> = vec![None; n];
    for &(i, s) in available {
        if i < n && s.len() == sb {
            have[i] = Some(s);
        }
    }
    if have.iter().filter(|s| s.is_some()).count() < k {
        return Err(Error::Malformed("not enough shards"));
    }
    let mut data = vec![0u8; k * sb];
    // Data shards that arrived go straight in.
    for i in 0..k {
        if let Some(s) = have[i] {
            data[i * sb..(i + 1) * sb].copy_from_slice(s);
        }
    }
    if (0..k).any(|i| have[i].is_none()) {
        let mut off = 0;
        while off < sb {
            let w = COLUMN.min(sb - off);
            let mut dec = ReedSolomonDecoder::new(k, m, w).expect("decoder");
            for (i, s) in have.iter().enumerate() {
                let Some(s) = s else { continue };
                if i < k {
                    dec.add_original_shard(i, &s[off..off + w]).expect("shard");
                } else {
                    dec.add_recovery_shard(i - k, &s[off..off + w]).expect("shard");
                }
            }
            let result = dec.decode().map_err(|_| Error::Malformed("shards do not decode"))?;
            for (i, slice) in result.restored_original_iter() {
                data[i * sb + off..i * sb + off + w].copy_from_slice(slice);
            }
            off += w;
        }
    }
    data.truncate(len);
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data(len: usize) -> Vec<u8> {
        let mut d = vec![0u8; len];
        blake3::Hasher::new().update(b"coding").finalize_xof().fill(&mut d);
        d
    }

    #[test]
    fn any_k_of_n_rebuild_the_data() {
        let code = Code::new(3, 5).unwrap();
        for len in [1usize, 63, 1000, 700_000, 3 * 1024 * 1024 + 17] {
            let d = data(len);
            let shards = encode(code, &d);
            assert_eq!(shards.len(), 5);
            // Every 3-subset of 5.
            for a in 0..5 {
                for b in a + 1..5 {
                    for c in b + 1..5 {
                        let avail = [(a, shards[a].as_slice()), (b, shards[b].as_slice()), (c, shards[c].as_slice())];
                        assert_eq!(decode(code, len, &avail).unwrap(), d, "len {len} shards {a},{b},{c}");
                    }
                }
            }
        }
    }

    #[test]
    fn too_few_shards_fail() {
        let code = Code::new(3, 5).unwrap();
        let d = data(5000);
        let s = encode(code, &d);
        assert!(decode(code, d.len(), &[(0, &s[0]), (4, &s[4])]).is_err());
    }

    #[test]
    fn replication_is_k_equals_one() {
        let code = Code::new(1, 3).unwrap();
        let d = data(4096);
        let s = encode(code, &d);
        assert_eq!(decode(code, d.len(), &[(2, &s[2])]).unwrap(), d);
    }
}
