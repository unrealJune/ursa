//! Content-defined chunking. Chunk boundaries follow the content, so inserting bytes in the
//! middle of a file moves only the chunks around the edit (S1: ~2 GB/s per core, never the
//! bottleneck).

use std::io::Read;

/// Sizes from S1's plan: about 1 MiB on average, between 256 KiB and 4 MiB.
pub const MIN_SIZE: u32 = 256 * 1024;
pub const AVG_SIZE: u32 = 1024 * 1024;
pub const MAX_SIZE: u32 = 4 * 1024 * 1024;

/// Splits a stream into chunks.
pub fn chunks<R: Read>(reader: R) -> impl Iterator<Item = std::io::Result<Vec<u8>>> {
    fastcdc::v2020::StreamCDC::new(reader, MIN_SIZE, AVG_SIZE, MAX_SIZE)
        .map(|c| c.map(|d| d.data).map_err(|e| std::io::Error::other(e.to_string())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunks_reassemble_and_respect_bounds() {
        let mut data = vec![0u8; 9 * 1024 * 1024 + 123];
        blake3::Hasher::new().update(b"chunk test").finalize_xof().fill(&mut data);
        let parts: Vec<Vec<u8>> = chunks(&data[..]).collect::<std::io::Result<_>>().unwrap();
        assert_eq!(parts.concat(), data);
        for p in &parts[..parts.len() - 1] {
            assert!(p.len() >= MIN_SIZE as usize && p.len() <= MAX_SIZE as usize);
        }
    }

    #[test]
    fn an_insertion_moves_few_chunks() {
        let mut data = vec![0u8; 16 * 1024 * 1024];
        blake3::Hasher::new().update(b"edit test").finalize_xof().fill(&mut data);
        let before: Vec<Vec<u8>> = chunks(&data[..]).collect::<std::io::Result<_>>().unwrap();
        let mut edited = data.clone();
        edited.splice(8 * 1024 * 1024..8 * 1024 * 1024, b"an edit in the middle".iter().copied());
        let after: Vec<Vec<u8>> = chunks(&edited[..]).collect::<std::io::Result<_>>().unwrap();
        let changed = after.iter().filter(|c| !before.contains(c)).count();
        assert!(changed <= 2, "{changed} chunks changed");
    }
}
