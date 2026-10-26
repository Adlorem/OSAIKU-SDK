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
#[cfg(all(target_arch = "x86_64", target_os = "none"))]
pub mod desktop;
pub mod directory_summary;
pub mod file_broker;
pub mod file_browser;
pub mod file_export;
pub mod file_manager;
pub mod files;
pub mod hosted;
pub mod ipc;
pub mod launch;
pub mod lifecycle;
pub mod metrics;
pub mod model_manager;
pub mod packages;
pub mod threads;
pub use osaiku_package::compatibility as package_compatibility;
pub mod packaged;
pub mod private_data;
pub mod process;
#[cfg(target_arch = "x86_64")]
pub mod raw;
pub mod report;
pub mod session;
#[cfg(all(target_arch = "x86_64", target_os = "none"))]
pub mod status_client;
pub mod status_snapshot;
pub mod surface;
pub mod window;
pub mod window_status;

#[cfg(all(target_arch = "x86_64", target_os = "none"))]
mod native;
#[cfg(all(target_arch = "x86_64", target_os = "none"))]
pub use native::*;

pub mod devices;
pub mod package_service;

pub mod picker;
