//! CPU asset uploads use the existing bounded shared-region allocator. Large
//! images are tiled by the client; no generic shared-memory quota is enlarged.
use osaiku_driver_protocol::shared::{
    self as wire, op, Handle, Request, MAX_PAGES, PAGE, READ, WRITE,
};
pub const MAX_BYTES: usize = (MAX_PAGES * PAGE) as usize;
/// # Safety
/// write must address only a live writable mapping returned by this same
/// transport. Calls are serialized; SEAL retires every writer before return.
pub unsafe trait Transport {
    fn exchange(&mut self, request: Request) -> Result<Request, isize>;
    fn write(&mut self, address: u64, bytes: &[u8]) -> Result<(), isize>;
}
/// Move-only retained storage. No writable pointer survives successful seal.
pub struct Sealed {
    handle: Handle,
    bytes: u64,
    pages: u64,
}
impl Sealed {
    pub fn id(&self) -> u64 {
        self.handle.id
    }
    pub fn generation(&self) -> u64 {
        self.handle.generation
    }
    pub fn bytes(&self) -> u64 {
        self.bytes
    }
}
pub struct Upload<T: Transport> {
    transport: T,
    failed: bool,
}
impl<T: Transport> Upload<T> {
    pub const fn new(transport: T) -> Self {
        Self {
            transport,
            failed: false,
        }
    }
    pub fn healthy(&self) -> bool {
        !self.failed
    }
    /// No returned object on a lost receipt. Kernel ownership/charges remain
    /// attached to this process and are reclaimed only through proven teardown.
    pub fn seal(&mut self, bytes: &[u8]) -> Result<Sealed, isize> {
        if self.failed {
            return Err(-5);
        }
        if bytes.is_empty() || bytes.len() > MAX_BYTES {
            return Err(-1);
        }
        let result = self.create(bytes);
        if result.is_err() {
            self.failed = true;
        }
        result
    }
    fn create(&mut self, bytes: &[u8]) -> Result<Sealed, isize> {
        let pages = (bytes.len() as u64).div_ceil(PAGE);
        let m = Request {
            pages,
            rights: READ | WRITE,
            ..Request::new(op::CREATE)
        };
        let r = self.transport.exchange(m)?;
        if r.handle.id == 0
            || r.handle.generation == 0
            || r != (Request {
                handle: r.handle,
                ..m
            })
        {
            return Err(-5);
        }
        let handle = r.handle;
        let m = Request {
            handle,
            pages,
            rights: READ | WRITE,
            ..Request::new(op::MAP)
        };
        let r = self.transport.exchange(m)?;
        if r.token == 0
            || r.address < wire::WINDOW_START
            || r.address % PAGE != 0
            || r.address
                .checked_add(pages * PAGE)
                .is_none_or(|n| n > wire::WINDOW_END)
            || r != (Request {
                address: r.address,
                token: r.token,
                ..m
            })
        {
            return Err(-5);
        }
        self.transport.write(r.address, bytes)?;
        let r = self.transport.exchange(Request {
            handle,
            ..Request::new(op::SEAL)
        })?;
        if r != (Request {
            handle,
            pages,
            token: 1,
            live: pages,
            ..Request::new(op::SEAL)
        }) {
            return Err(-5);
        }
        Ok(Sealed {
            handle,
            pages,
            bytes: bytes.len() as u64,
        })
    }
    /// Close new publication; reader retention can delay backing reclamation.
    /// Returns the number of pages actually reclaimed by this call.
    pub fn release(&mut self, sealed: Sealed) -> Result<u64, (isize, Sealed)> {
        if self.failed {
            return Err((-5, sealed));
        }
        let result = self.transport.exchange(Request {
            handle: sealed.handle,
            ..Request::new(op::REVOKE)
        });
        let result = result.and_then(|r| {
            if r != (Request {
                handle: sealed.handle,
                returned: r.returned,
                live: r.live,
                quarantined: r.quarantined,
                ..Request::new(op::REVOKE)
            }) || r
                .returned
                .checked_add(r.live)
                .and_then(|n| n.checked_add(r.quarantined))
                != Some(sealed.pages)
                || r.quarantined != 0
            {
                Err(-5)
            } else {
                Ok(r.returned)
            }
        });
        result.map_err(|e| {
            self.failed = true;
            (e, sealed)
        })
    }
}
