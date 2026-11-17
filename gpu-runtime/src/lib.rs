#![no_std]
//! GPU ownership interfaces. Native allocation, IOMMU and IRQ enforcement stay
//! with the platform; constructing a local value cannot grant device access.
#[cfg(feature = "cache")]
pub mod cache;
#[cfg(feature = "cache")]
pub mod blob;
pub use osaiku_gpu_protocol as protocol;
pub mod memory;
pub mod upload;
pub mod channel;
pub mod channel_wire;
pub mod backend;
pub mod process;
pub mod platform;
pub mod render;
#[cfg(all(target_arch = "x86_64", target_os = "none"))]
pub mod native;

pub mod measurement;
