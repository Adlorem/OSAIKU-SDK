#![no_std]
#![deny(unsafe_op_in_unsafe_fn)]

//! Initial native runtime boundary, with no OS-source or allocator dependency.
//! Safe native operations exist only on the freestanding x86_64 target. Host
//! builds expose contracts for tooling/tests; they must not invoke raw traps.
pub mod abi;
pub mod agent;
pub mod appearance;
pub mod application;
pub mod assistant;
pub mod compat;
pub mod context;
pub mod files;
pub mod hosted;
pub mod ipc;
pub mod launch;
pub mod lifecycle;
pub mod metrics;
pub mod model_manager;
pub use osaiku_package::compatibility as package_compatibility;
pub mod packaged;
pub mod process;
#[cfg(target_arch = "x86_64")]
pub mod raw;
pub mod report;
pub mod session;
pub mod surface;
pub mod window;

#[cfg(all(target_arch = "x86_64", target_os = "none"))]
mod native;
#[cfg(all(target_arch = "x86_64", target_os = "none"))]
pub use native::*;
