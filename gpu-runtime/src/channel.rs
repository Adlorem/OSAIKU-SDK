//! Typed copied-mailbox client. Acknowledging a batch means its CPU inputs are
//! no longer read; it never manufactures a GPU completion/presentation fence.
use crate::protocol::{
    buffer::{kind, Handle},
    channel::{self as wire, op, status, Message},
    render::{self, Scope},
    scene::Batch,
};
pub trait Transport {
    fn exchange(&mut self, message: Message, payload: &mut [u8]) -> Result<Message, isize>;
}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Offer{pub device:u64,pub connection:u64,pub width:u32,pub height:u32}
pub fn offer(transport:&mut impl Transport)->Result<Option<Offer>,isize>{
    let r=transport.exchange(Message::new(op::OFFER),&mut[])?;
    if r.operation!=op::OFFER||r.session!=0||r.generation!=0||r.sequence!=0||r.handle!=Handle::default()||r.length!=0||r.args[4..]!=[0;12]{return Err(-5);}
    if r.status==0&&r.args==[0;16]{return Ok(None);}
    if r.status!=1||r.args[0]==0||r.args[1]==0||!(1..=4096).contains(&r.args[2])||!(1..=4096).contains(&r.args[3]){return Err(-5);}
    Ok(Some(Offer{device:r.args[0],connection:r.args[1],width:r.args[2]as u32,height:r.args[3]as u32}))
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Identity {
    pub client: u64,
    pub renderer: u64,
    pub session: u64,
    pub generation: u64,
    pub device: u64,
    pub connection: u64,
    pub width: u32,
    pub height: u32,
}
impl Identity {
    fn parse(m: Message) -> Result<Self, isize> {
        let a = m.args;
        if m.operation != op::IDENTITY
            || m.session == 0
            || m.generation == 0
            || m.sequence != 0
            || m.handle != Handle::default()
            || m.length != 0
            || a[6..] != [0; 10]
            || a[0] as u32 == 0
            || a[0] >> 32 == 0
            || a[2] == 0
            || a[3] == 0
            || !(1..=16384).contains(&a[4])
            || !(1..=16384).contains(&a[5])
            || !matches!(m.status, status::IDLE | status::PENDING)
            || (m.status == status::PENDING && a[1] != 0)
            || (m.status == status::IDLE && (a[1] as u32 == 0 || a[1] >> 32 == 0 || a[1] == a[0]))
        {
            return Err(-5);
        }
        Ok(Self {
            client: a[0],
            renderer: a[1],
            session: m.session,
            generation: m.generation,
            device: a[2],
            connection: a[3],
            width: a[4] as u32,
            height: a[5] as u32,
        })
    }
    pub fn scope(self, sequence: u64) -> Scope {
        Scope {
            session: self.session,
            generation: self.generation,
            sequence,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AssetMap {
    address: u64,
    bytes: u64,
    handle: Handle,
    session: u64,
}
impl AssetMap {
    pub fn address(self) -> u64 {
        self.address
    }
    pub fn bytes(self) -> u64 {
        self.bytes
    }
    pub fn handle(self) -> Handle {
        self.handle
    }
    /// # Safety
    /// Transport must be the native admitted channel. Only use this borrow
    /// while the corresponding renderer batch is RUNNING, before COMPLETE.
    pub unsafe fn read(&self) -> &[u8] {
        unsafe { core::slice::from_raw_parts(self.address as *const u8, self.bytes as usize) }
    }
}
pub struct Channel<T> {
    transport: T,
    identity: Identity,
    failed: bool,
    next: u64,
    pending: Option<u64>,
}
impl<T: Transport> Channel<T> {
    pub fn open(mut transport: T, width: u32, height: u32) -> Result<Self, isize> {
        let mut m = Message::new(op::OPEN);
        m.args[0] = width.into();
        m.args[1] = height.into();
        let identity = Identity::parse(transport.exchange(m, &mut [])?)?;
        if identity.width != width || identity.height != height {
            return Err(-5);
        }
        Ok(Self {
            transport,
            identity,
            failed: false,
            next: 1,
            pending: None,
        })
    }
    pub fn connect(mut transport: T) -> Result<Self, isize> {
        let identity = Identity::parse(transport.exchange(Message::new(op::IDENTITY), &mut [])?)?;
        Ok(Self {
            transport,
            identity,
            failed: false,
            next: 1,
            pending: None,
        })
    }
    pub fn identity(&self) -> Identity {
        self.identity
    }
    pub fn healthy(&self) -> bool {
        !self.failed
    }
    fn bad<R>(&mut self) -> Result<R, isize> {
        self.failed = true;
        Err(-5)
    }
    fn request(&self, operation: u32) -> Message {
        Message {
            session: self.identity.session,
            generation: self.identity.generation,
            ..Message::new(operation)
        }
    }
    fn call(&mut self, m: Message, bytes: &mut [u8]) -> Result<Message, isize> {
        if self.failed {
            return Err(-5);
        }
        let reply = match self.transport.exchange(m, bytes) {
            Ok(r) => r,
            Err(e) => {
                if e != wire::BUSY {
                    self.failed = true;
                }
                return Err(e);
            }
        };
        if reply.operation != m.operation
            || reply.session != m.session
            || reply.generation != m.generation
            || (m.operation != op::TAKE && reply.sequence != m.sequence)
            || (m.operation != op::TAKE && reply.length != 0)
            || (m.operation != op::REGISTER && reply.handle != m.handle)
            || (!matches!(m.operation, op::MAP | op::POLL) && reply.args != m.args)
            || (m.operation == op::POLL && reply.args[4..] != [0; 12])
        {
            return self.bad();
        }
        Ok(reply)
    }
    pub fn refresh(&mut self) -> Result<bool, isize> {
        if self.failed {
            return Err(-5);
        }
        let i = match self
            .transport
            .exchange(Message::new(op::IDENTITY), &mut [])
            .and_then(Identity::parse)
        {
            Ok(i) => i,
            Err(e) => {
                if e != wire::BUSY {
                    self.failed = true;
                }
                return Err(e);
            }
        };
        if (Identity {
            renderer: self.identity.renderer,
            width: self.identity.width,
            height: self.identity.height,
            ..i
        }) != self.identity
            || (self.identity.renderer != 0 && i.renderer != self.identity.renderer)
        {
            return self.bad();
        }
        self.identity = i;
        Ok(i.renderer != 0)
    }
    pub fn register(
        &mut self,
        shared_id: u64,
        shared_generation: u64,
        bytes: u64,
    ) -> Result<Handle, isize> {
        let mut m = self.request(op::REGISTER);
        m.args[..3].copy_from_slice(&[shared_id, shared_generation, bytes]);
        let r = self.call(m, &mut [])?;
        let h = r.handle;
        if r.status != 0
            || h.owner != self.identity.client
            || h.device != self.identity.device
            || h.generation != self.identity.connection
            || h.serial == 0
            || h.kind != kind::BUFFER
            || h.reserved != 0
        {
            return self.bad();
        }
        Ok(h)
    }
    pub fn release(&mut self, handle: Handle) -> Result<(), isize> {
        let mut m = self.request(op::RELEASE);
        m.handle = handle;
        if self.call(m, &mut [])?.status != 0 {
            return self.bad();
        }
        Ok(())
    }
    /// No sequence is consumed on backpressure. Retain all registered handles
    /// until poll reports DONE/FAILED; neither is a presentation receipt.
    pub fn submit(&mut self, bytes: &mut [u8]) -> Result<u64, isize> {
        if self.pending.is_some() {
            return Err(wire::BUSY);
        }
        let b = Batch::decode(bytes, self.identity.scope(self.next)).map_err(|_| -1isize)?;
        if b.viewport() != (self.identity.width, self.identity.height) {
            return Err(-1);
        }
        let next = self.next.checked_add(1).ok_or(-5isize)?;
        let mut m = self.request(op::SUBMIT);
        m.sequence = self.next;
        m.length = bytes.len() as u64;
        if self.call(m, bytes)?.status != status::QUEUED {
            return self.bad();
        }
        self.pending = Some(self.next);
        self.next = next;
        Ok(m.sequence)
    }
    pub fn next_scope(&self) -> Scope {
        self.identity.scope(self.next)
    }
    pub fn resize(&mut self, width: u32, height: u32) -> Result<(), isize> {
        if self.pending.is_some() { return Err(wire::BUSY); }
        if width == 0 || height == 0 || width > 4096 || height > 4096 { return Err(-1); }
        let mut m = self.request(op::RESIZE); m.args[..2].copy_from_slice(&[u64::from(width), u64::from(height)]);
        if self.call(m, &mut [])?.status != 0 { return self.bad(); }
        self.identity.width = width; self.identity.height = height; Ok(())
    }
    pub fn poll(&mut self) -> Result<bool, isize> {
        self.poll_result().map(|r| r.is_some())
    }
    /// CPU command result only. Returned opaque surface references must still
    /// be checked by the platform's publication boundary before display use.
    pub fn poll_result(&mut self) -> Result<Option<[u64; 4]>, isize> {
        let mut m = self.request(op::POLL);
        m.sequence = self.pending.ok_or(-5isize)?;
        let reply = self.call(m, &mut [])?;
        match reply.status {
            status::QUEUED | status::RUNNING if reply.args[..4] == [0; 4] => Ok(None),
            status::DONE => {
                self.pending = None;
                Ok(Some(reply.args[..4].try_into().unwrap()))
            }
            status::FAILED => {
                self.pending = None;
                Err(-5)
            }
            _ => self.bad(),
        }
    }
    pub fn take(&mut self, out: &mut [u8]) -> Result<(Scope, usize), isize> {
        if self.pending.is_some() {
            return Err(wire::BUSY);
        }
        if out.len() < render::MAX_BYTES {
            return Err(-1);
        }
        self.refresh()?;
        let r = self.call(self.request(op::TAKE), out)?;
        let n = usize::try_from(r.length).map_err(|_| -5isize)?;
        if r.status != status::RUNNING || r.sequence != self.next || n > out.len() {
            return self.bad();
        }
        let scope = self.identity.scope(r.sequence);
        let valid = Batch::decode(&out[..n], scope)
            .is_ok_and(|b| b.viewport() == (self.identity.width, self.identity.height));
        if !valid {
            return self.bad();
        }
        self.pending = Some(r.sequence);
        Ok((scope, n))
    }
    pub fn map(&mut self, handle: Handle) -> Result<AssetMap, isize> {
        if self.pending.is_none() {
            return Err(-5);
        }
        let mut m = self.request(op::MAP);
        m.handle = handle;
        let r = self.call(m, &mut [])?;
        let persistent=self.identity.client as u32==8 && r.args[0]>=0xb000_0000;
        let (start,end,limit)=if persistent {(0xb000_0000,0xc000_0000,64*1024*1024)}else{(wire::ASSET_START,wire::ASSET_END,render::MAX_ASSET_BYTES)};
        if r.status != 0
            || r.args[3..] != [0; 13]
            || r.args[0] < start
            || r.args[0] & 4095 != 0
            || r.args[1] == 0
            || r.args[2] == 0
            || r.args[2] > limit
            || r.args[0]
                .checked_add(r.args[2])
                .is_none_or(|at| at > end)
        {
            return self.bad();
        }
        Ok(AssetMap {
            address: r.args[0],
            bytes: r.args[2],
            handle,
            session: self.identity.session,
        })
    }
    pub fn complete(&mut self, succeeded: bool) -> Result<(), isize> {
        self.complete_result(succeeded, [0; 4])
    }
    pub fn complete_result(&mut self, succeeded: bool, result: [u64; 4]) -> Result<(), isize> {
        let mut m = self.request(op::COMPLETE);
        m.sequence = self.pending.ok_or(-5isize)?;
        let next = m.sequence.checked_add(1).ok_or(-5isize)?;
        m.args[0] = u64::from(!succeeded);
        m.args[1..5].copy_from_slice(&result);
        if self.call(m, &mut [])?.status
            != if succeeded {
                status::DONE
            } else {
                status::FAILED
            }
        {
            return self.bad();
        }
        self.pending = None;
        self.next = next;
        Ok(())
    }
    pub fn close(&mut self) -> Result<(), isize> {
        let r = self.call(self.request(op::CLOSE), &mut []);
        self.failed = true;
        r.and_then(|r| {
            if r.status == status::CLOSED {
                Ok(())
            } else {
                Err(-5)
            }
        })
    }
}
