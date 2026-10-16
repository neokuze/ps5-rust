//! Hardware RNG. Feature `random`.

use crate::{check, Result};

/// Fills `buf` with random bytes, 64 at a time (the per-call limit is
/// undocumented; 64 is a conservative chunk size).
pub fn fill(buf: &mut [u8]) -> Result<()> {
    for chunk in buf.chunks_mut(64) {
        check(unsafe { ps5_sys::sceRandomGetRandomNumber(chunk.as_mut_ptr(), chunk.len()) })?;
    }
    Ok(())
}

pub fn u64() -> Result<u64> {
    let mut b = [0u8; 8];
    fill(&mut b)?;
    Ok(u64::from_ne_bytes(b))
}
