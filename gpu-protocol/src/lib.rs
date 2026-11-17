#![no_std]
//! Public GPU package descriptors. Decoding never grants device access.
//! Wire records use explicit little-endian fields; no Rust pointers cross IPC.
pub mod resources;
pub mod control;
pub mod channel;
pub mod backend;
pub mod render;
pub mod compositor;
pub mod scene;
pub mod platform;
pub mod buffer;

pub const PACKAGE_BYTES: usize = 192;
pub const BACKEND_COMPONENT: u16 = 4;
pub const RENDERER_COMPONENT: u16 = 5;
pub const MAX_BACKEND_ELF: u64 = 16 * 1024 * 1024;
pub const MAX_RENDERER_ELF: u64 = 128 * 1024 * 1024;
pub const MAX_PROCESS_BYTES: u64 = 1024 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Length,
    Reserved,
    Identity,
    Version,
    Budget,
    AddressSpace,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Interface {
    pub major: u16,
    pub minor: u16,
    pub features: u64,
}
impl Interface {
    fn decode(b: &[u8]) -> Result<Self, Error> {
        zero(&b[4..8])?;
        let result = Self {
            major: u16_at(b, 0),
            minor: u16_at(b, 2),
            features: u64_at(b, 8),
        };
        if result.major == 0 {
            return Err(Error::Version);
        }
        Ok(result)
    }
}

/// Memory is per process; DMA is separately charged to the hardware backend.
/// The platform must also account image, heap, stacks, TLS and page tables.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Budget {
    pub memory: u64,
    pub dma: u64,
    pub stack: u32,
    pub tls: u32,
    pub threads: u16,
    pub instances: u16,
}
impl Budget {
    /// Existing workers include exited, unjoined slots. The control thread is
    /// always charged as well. This check cannot increase the process image or
    /// stack/TLS storage admitted by the loader.
    pub fn admits_worker(self, occupied_workers: usize, stack_bytes: u64, tls_bytes: usize) -> bool {
        occupied_workers.checked_add(1).is_some_and(|n| n < usize::from(self.threads))
            && stack_bytes != 0 && stack_bytes <= u64::from(self.stack)
            && tls_bytes != 0 && tls_bytes as u64 <= u64::from(self.tls)
    }
    fn decode(b: &[u8], backend: bool) -> Result<Self, Error> {
        zero(&b[28..32])?;
        let result = Self {
            memory: u64_at(b, 0),
            dma: u64_at(b, 8),
            stack: u32_at(b, 16),
            tls: u32_at(b, 20),
            threads: u16_at(b, 24),
            instances: u16_at(b, 26),
        };
        if !(4096..=MAX_PROCESS_BYTES).contains(&result.memory)
            || result.memory % 4096 != 0
            || result.dma > 4 * MAX_PROCESS_BYTES
            || result.dma % 4096 != 0
            || !(4096..=1024 * 1024).contains(&result.stack)
            || result.stack % 4096 != 0
            || result.tls > 64 * 1024
            || result.tls % 4096 != 0
            || !(1..=64).contains(&result.threads)
            || !(1..=64).contains(&result.instances)
            || (backend && result.instances != 1)
            || (!backend && result.dma != 0)
            || u64::from(result.threads) * (u64::from(result.stack) + u64::from(result.tls))
                >= result.memory
        {
            return Err(Error::Budget);
        }
        Ok(result)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemoryModel {
    Unified,
    DeviceLocal,
}

/// Three independently negotiated boundaries, with no kernel build hash.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Package<'a> {
    pub family: &'a str,
    pub platform: Interface,
    pub renderer: Interface,
    pub client: Interface,
    pub backend_budget: Budget,
    pub renderer_budget: Budget,
    pub dma_address_bits: u8,
    pub gpu_address_bits: u8,
    pub memory: MemoryModel,
}
impl<'a> Package<'a> {
    pub fn decode(b: &'a [u8]) -> Result<Self, Error> {
        if b.len() != PACKAGE_BYTES {
            return Err(Error::Length);
        }
        let end = b[..64].iter().position(|v| *v == 0).unwrap_or(64);
        zero(&b[end..64])?;
        if end == 0
            || !b[..end]
                .iter()
                .all(|v| v.is_ascii_lowercase() || v.is_ascii_digit() || b".-".contains(v))
        {
            return Err(Error::Identity);
        }
        let family = core::str::from_utf8(&b[..end]).map_err(|_| Error::Identity)?;
        let memory = match b[178] {
            1 => MemoryModel::Unified,
            2 => MemoryModel::DeviceLocal,
            _ => return Err(Error::AddressSpace),
        };
        if !(32..=64).contains(&b[176]) || !(32..=64).contains(&b[177]) {
            return Err(Error::AddressSpace);
        }
        zero(&b[179..])?;
        Ok(Self {
            family,
            platform: Interface::decode(&b[64..80])?,
            renderer: Interface::decode(&b[80..96])?,
            client: Interface::decode(&b[96..112])?,
            backend_budget: Budget::decode(&b[112..144], true)?,
            renderer_budget: Budget::decode(&b[144..176], false)?,
            dma_address_bits: b[176],
            gpu_address_bits: b[177],
            memory,
        })
    }
}

fn zero(b: &[u8]) -> Result<(), Error> {
    if b.iter().any(|v| *v != 0) {
        Err(Error::Reserved)
    } else {
        Ok(())
    }
}
fn u16_at(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes(b[at..at + 2].try_into().unwrap())
}
fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}
fn u64_at(b: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(b[at..at + 8].try_into().unwrap())
}

pub mod measurement;
