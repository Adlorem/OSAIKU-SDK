//! Synchronous compatibility endpoint. Implementations must finish using every
//! borrowed slice before returning; asynchronous DMA uses owned queue tokens.
use crate::block::Error;
pub const MAX_READ_BYTES: usize = 64 * 1024;

pub trait Media {
    fn blocks(&self) -> u64;
    /// Monotonic attachment/write revision for retained continuations. A caller
    /// without a revision must retain exclusive ownership between their steps.
    fn revision(&self) -> u64 {
        0
    }
    fn read(&mut self, lba: u64, data: &mut [u8; 512]) -> Result<(), Error>;
    fn write(&mut self, lba: u64, data: &[u8; 512]) -> Result<(), Error>;
    fn write_blocks(&mut self, lba: u64, data: &[u8]) -> Result<(), Error> {
        if data.is_empty() || data.len() > MAX_READ_BYTES || data.len() % 512 != 0 {
            return Err(Error::Length);
        }
        lba.checked_add((data.len() / 512) as u64)
            .filter(|end| *end <= self.blocks())
            .ok_or(Error::InvalidRange)?;
        for (i, part) in data.chunks_exact(512).enumerate() {
            self.write(lba + i as u64, part.try_into().map_err(|_| Error::Length)?)?;
        }
        Ok(())
    }
    fn read_blocks(&mut self, lba: u64, out: &mut [u8]) -> Result<(), Error> {
        if out.is_empty() || out.len() > MAX_READ_BYTES || out.len() % 512 != 0 {
            return Err(Error::Length);
        }
        lba.checked_add((out.len() / 512) as u64)
            .filter(|end| *end <= self.blocks())
            .ok_or(Error::InvalidRange)?;
        for (i, part) in out.chunks_exact_mut(512).enumerate() {
            let mut b = [0; 512];
            self.read(lba + i as u64, &mut b)?;
            part.copy_from_slice(&b);
        }
        Ok(())
    }
    fn flush(&mut self) -> Result<(), Error>;
}
