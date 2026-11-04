//! Graphics ABI v1. Integer-only records; identities and capabilities are claims
//! until checked against kernel-owned registries. No PCI IDs or device addresses.
pub const VERSION: u64 = 1;
/// A new operation in the unchanged UI_SURFACE v8 envelope. Old records retain
/// their meaning. Payload is exactly Negotiation, both input and output.
pub const NEGOTIATE: u32 = 22;
pub const MAX_BUFFER_BYTES: u64 = 256 * 1024 * 1024;
pub const MAX_QUEUED: usize = 8;
pub const MAX_CONTEXT_QUEUED: usize = 2;
pub const MAX_TIMEOUT_US: u64 = 2_000_000;

pub mod capability {
    pub const RENDER: u64 = 1;
    pub const SAMPLE: u64 = 2;
    pub const SHARE: u64 = 4;
    pub const SYNC: u64 = 8;
    pub const PRESENT: u64 = 16;
    pub const UI_RENDERER: u64 = 32;
    pub const UI: u64 = RENDER | SAMPLE | SHARE | SYNC | PRESENT | UI_RENDERER;
}
pub mod access {
    pub const CPU_READ: u32 = 1;
    pub const CPU_WRITE: u32 = 2;
    pub const GPU_READ: u32 = 4;
    pub const GPU_WRITE: u32 = 8;
    pub const SCANOUT: u32 = 16;
    pub const ALL: u32 = 31;
}
pub mod kind {
    pub const BUFFER: u32 = 1;
    pub const CONTEXT: u32 = 2;
    pub const JOB: u32 = 3;
    pub const SYNC: u32 = 4;
}
pub mod policy {
    pub const PREFER_INTEGRATED: u64 = 0;
    pub const EXPLICIT: u64 = 1;
    pub const DIAGNOSTIC_CPU: u64 = 2;
}
pub mod fallback {
    pub const NONE: u64 = 0;
    pub const NO_QUALIFIED_DEVICE: u64 = 1;
    pub const REQUESTED_DEVICE_UNAVAILABLE: u64 = 2;
    pub const CLIENT_RENDERER_UNAVAILABLE: u64 = 3;
    pub const DIAGNOSTIC_CPU: u64 = 4;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Version,
    Shape,
    Bounds,
    Permission,
    Stale,
    Busy,
    Exhausted,
    Order,
    Timeout,
    Cancelled,
    Dependency,
    Quarantined,
}

/// B,G,R,A bytes, premultiplied alpha, UNORM; the X variant is opaque B,G,R,X.
/// These are DRM fourcc values, not a promise of any backend's scanout support.
pub const ARGB8888: u32 = u32::from_le_bytes(*b"AR24");
pub const XRGB8888: u32 = u32::from_le_bytes(*b"XR24");
pub const LINEAR: u64 = 0;
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BufferDesc {
    pub width: u32,
    pub height: u32,
    pub format: u32,
    pub access: u32,
    pub stride: u64,
    pub bytes: u64,
    pub modifier: u64,
}
impl BufferDesc {
    pub fn validate(self) -> Result<(), Error> {
        if !matches!(self.format, ARGB8888 | XRGB8888)
            || self.modifier != LINEAR
            || self.access == 0
            || self.access & !access::ALL != 0
        {
            return Err(Error::Shape);
        }
        if self.width == 0
            || self.height == 0
            || self.width > 16384
            || self.height > 16384
            || self.stride % 4 != 0
            || self.stride < u64::from(self.width) * 4
            || self.stride.checked_mul(self.height.into()) != Some(self.bytes)
            || self.bytes > MAX_BUFFER_BYTES
        {
            return Err(Error::Bounds);
        }
        Ok(())
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Handle {
    /// Encoded process endpoint, including its nonzero generation.
    pub owner: u64,
    pub device: u64,
    pub generation: u64,
    /// Never reused, including when a slot is recycled.
    pub serial: u64,
    pub kind: u32,
    pub reserved: u32,
}

pub mod stage {
    pub const ACCEPTED: u64 = 1;
    pub const RENDER_DONE: u64 = 2;
    pub const PRESENTED: u64 = 3;
    pub const RELEASED: u64 = 4;
    pub const CANCELLED: u64 = 5;
    pub const TIMED_OUT: u64 = 6;
}
/// Completion events are distinct from submission success and from each other.
/// The receiver checks exact job identity and increasing per-job sequence.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Completion {
    pub version: u64,
    pub job: Handle,
    pub sequence: u64,
    pub stage: u64,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Submission {
    pub version: u64,
    pub context: Handle,
    pub buffer: Handle,
    /// All-zero means no dependency; otherwise an existing binary sync handle.
    pub wait: Handle,
    pub timeout_us: u64,
}
/// A read-only grant bound to the compositor's process generation and one
/// job completion. Possession alone is insufficient: the registry must match it.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Share {
    pub buffer: Handle,
    pub completion: Handle,
    pub recipient: u64,
    pub serial: u64,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct UiFrame {
    pub image: Handle,
    pub completion: Handle,
    pub offer: crate::surface::FrameOffer,
    pub metadata_serial: u64,
    pub source_hash: u64,
}

/// Supplied by the trusted device broker after qualifying the complete path.
/// v1 supports same-device composition/presentation only. No inferred cross-GPU
/// copies; no selection based on vendor, PCI enumeration order or GOP ownership.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Device {
    pub id: u64,
    pub generation: u64,
    pub integrated: bool,
    pub capabilities: u64,
    pub presentation_device: u64,
}
impl Device {
    fn supports(self, required: u64) -> bool {
        self.id != 0
            && self.generation != 0
            && self.presentation_device == self.id
            && self.capabilities & required == required
            && self.capabilities & !capability::UI == 0
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Negotiation {
    pub min_version: u64,
    pub max_version: u64,
    pub required: u64,
    pub client_caps: u64,
    pub policy: u64,
    pub requested_device: u64,
    pub selected_version: u64,
    pub device: u64,
    pub generation: u64,
    pub capabilities: u64,
    /// 0 = explicit CPU fallback, 1 = native GPU UI path.
    pub renderer: u64,
    pub fallback: u64,
}
impl Negotiation {
    pub const fn gpu_default(client_caps: u64) -> Self {
        Self {
            min_version: VERSION,
            max_version: VERSION,
            required: capability::UI,
            client_caps,
            policy: policy::PREFER_INTEGRATED,
            requested_device: 0,
            selected_version: 0,
            device: 0,
            generation: 0,
            capabilities: 0,
            renderer: 0,
            fallback: 0,
        }
    }
    pub fn resolve(mut self, devices: &[Device]) -> Result<Self, Error> {
        if self.min_version == 0 || self.min_version > VERSION || self.max_version < VERSION {
            return Err(Error::Version);
        }
        if self.required != capability::UI
            || self.client_caps & !capability::UI != 0
            || self.policy > policy::DIAGNOSTIC_CPU
            || (self.policy == policy::EXPLICIT) != (self.requested_device != 0)
            || [
                self.selected_version,
                self.device,
                self.generation,
                self.capabilities,
                self.renderer,
                self.fallback,
            ]
            .iter()
            .any(|v| *v != 0)
        {
            return Err(Error::Shape);
        }
        self.selected_version = VERSION;
        if self.policy == policy::DIAGNOSTIC_CPU {
            self.fallback = fallback::DIAGNOSTIC_CPU;
            return Ok(self);
        }
        // Duplicate identities cannot be interpreted safely as one device.
        if devices
            .iter()
            .enumerate()
            .any(|(i, d)| devices[..i].iter().any(|p| p.id == d.id))
        {
            return Err(Error::Shape);
        }
        let selected = devices
            .iter()
            .filter(|d| {
                d.supports(self.required)
                    && (self.policy != policy::EXPLICIT || d.id == self.requested_device)
            })
            .min_by_key(|d| (!d.integrated, d.id));
        let Some(d) = selected else {
            self.fallback = if self.policy == policy::EXPLICIT {
                fallback::REQUESTED_DEVICE_UNAVAILABLE
            } else {
                fallback::NO_QUALIFIED_DEVICE
            };
            return Ok(self);
        };
        if self.client_caps & capability::UI_RENDERER == 0 {
            self.fallback = fallback::CLIENT_RENDERER_UNAVAILABLE;
            return Ok(self);
        }
        self.device = d.id;
        self.generation = d.generation;
        self.capabilities = d.capabilities;
        self.renderer = 1;
        Ok(self)
    }
    pub fn validate_response(self, request: Self) -> Result<(), Error> {
        let mut echoed = self;
        echoed.selected_version = 0;
        echoed.device = 0;
        echoed.generation = 0;
        echoed.capabilities = 0;
        echoed.renderer = 0;
        echoed.fallback = 0;
        if echoed != request || self.selected_version != VERSION {
            return Err(Error::Version);
        }
        let devices = [Device {
            id: self.device,
            generation: self.generation,
            integrated: true,
            capabilities: self.capabilities,
            presentation_device: self.device,
        }];
        if self.renderer == 1 {
            if request.resolve(&devices)? != self {
                return Err(Error::Shape);
            }
        } else if self.renderer != 0
            || self.device != 0
            || self.generation != 0
            || self.capabilities != 0
            || !matches!(self.fallback, 1..=4)
            || (self.fallback == fallback::DIAGNOSTIC_CPU)
                != (self.policy == policy::DIAGNOSTIC_CPU)
            || (self.fallback == fallback::REQUESTED_DEVICE_UNAVAILABLE
                && self.policy != policy::EXPLICIT)
            || (self.fallback == fallback::CLIENT_RENDERER_UNAVAILABLE
                && self.client_caps & capability::UI_RENDERER != 0)
        {
            return Err(Error::Shape);
        }
        Ok(())
    }
}

/// No hidden retry or cached generation: negotiate before each new publication.
pub fn negotiate_with(
    request: Negotiation,
    mut exchange: impl FnMut(&crate::surface::Request, &mut [u8], &mut crate::surface::Receipt) -> isize,
) -> Result<Negotiation, crate::surface::PublishError> {
    use crate::surface::{PublishError, Receipt, Request, REQUEST_VERSION};
    let mut response = request;
    let bytes = unsafe {
        core::slice::from_raw_parts_mut(
            (&mut response as *mut Negotiation).cast::<u8>(),
            core::mem::size_of::<Negotiation>(),
        )
    };
    let mut receipt = Receipt::default();
    let code = exchange(
        &Request {
            version: REQUEST_VERSION,
            operation: NEGOTIATE,
            ..Request::default()
        },
        bytes,
        &mut receipt,
    );
    if code != crate::abi::SYSCALL_OK {
        return Err(PublishError::Transport(code));
    }
    if receipt != Receipt::default() || response.validate_response(request).is_err() {
        return Err(PublishError::Receipt);
    }
    Ok(response)
}
const _: () = assert!(core::mem::size_of::<Handle>() == 40);
const _: () = assert!(core::mem::size_of::<BufferDesc>() == 40);
const _: () = assert!(core::mem::size_of::<Negotiation>() == 96);
const _: () = assert!(core::mem::size_of::<Completion>() == 64);
const _: () = assert!(core::mem::size_of::<Submission>() == 136);
const _: () = assert!(core::mem::size_of::<Share>() == 96);
const _: () = assert!(core::mem::size_of::<UiFrame>() == 208);
