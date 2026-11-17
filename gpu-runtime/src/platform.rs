//! Typed platform client. Allocation handles and IOVAs are distinct from CPU
//! pointers and physical RAM. Uncertain replies poison the connection.
use crate::protocol::{
    control,
    platform::{self as wire, op, Message, Scope},
};
pub trait Transport {
    fn exchange(&mut self, request: Message) -> Result<Message, isize>;
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Info {
    pub pci_identity: u32,
    pub revision_class: u32,
    pub registers: u64,
    pub aperture: u64,
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub framebuffer_offset: u64,
    pub framebuffer_bytes: u64,
    pub clock_hz: u64,
    pub dma_bits: u8,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Publication { pub data: u64, pub fence: u64 }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PublishedSource {
    pub opaque: u64, pub bytes: u64, pub fence: u64, pub kind: u64,
    pub peer: crate::protocol::backend::Peer,
}
#[derive(Debug)]
pub struct Allocation {
    scope: Scope,
    handle: u64,
    cpu: u64,
    iova: u64,
    bytes: u64,
}
impl Allocation {
    pub fn cpu(&self) -> u64 {
        self.cpu
    }
    pub fn iova(&self) -> u64 {
        self.iova
    }
    pub fn bytes(&self) -> u64 {
        self.bytes
    }
    /// A page capability for this established mapping. Neither CPU virtual nor
    /// physical addresses participate in its construction. The caller retains
    /// this allocation until all GPU references are retired.
    pub fn page(&self, index: u64) -> Result<crate::memory::DmaPage, crate::memory::Error> {
        use crate::memory::{DmaPage, Error};
        if index >= self.bytes / wire::PAGE {
            return Err(Error::Invalid);
        }
        let serial = self
            .handle
            .checked_mul(wire::MAX_PAGES)
            .and_then(|n| n.checked_add(index))
            .ok_or(Error::Exhausted)?;
        unsafe {
            DmaPage::established(
                self.iova + index * wire::PAGE,
                self.scope.device,
                self.scope.connection,
                serial,
            )
        }
    }
}
pub struct Platform<T> {
    transport: T,
    scope: Scope,
    info: Info,
    failed: bool,
    ready: bool,
}
impl<T: Transport> Platform<T> {
    pub fn connect(mut transport: T, identity: control::Message) -> Result<Self, isize> {
        let scope = Scope::from_control(identity).map_err(|_| -8isize)?;
        let request = Message::new(op::INFO, scope);
        let reply = transport.exchange(request)?;
        if reply.operation != op::INFO
            || reply.scope != scope
            || reply.args[11..].iter().any(|v| *v != 0)
        {
            return Err(-5);
        }
        let a = reply.args;
        if a[0] > u32::MAX as u64
            || a[1] > u32::MAX as u64
            || a[2] == 0
            || a[3] == 0
            || a[2] > wire::MMIO_BYTES
            || a[3] > wire::APERTURE_BYTES
            || !(1..=16384).contains(&a[4])
            || !(1..=16384).contains(&a[5])
            || a[6] < a[4]
            || a[6] > 32768
            || a[8] < a[6] * a[5] * 4
            || a[7].checked_add(a[8]).is_none_or(|n| n > a[3])
            || a[9] == 0
            || !(32..=52).contains(&a[10])
        {
            return Err(-5);
        }
        Ok(Self {
            transport,
            scope,
            failed: false,
            ready: false,
            info: Info {
                pci_identity: a[0] as u32,
                revision_class: a[1] as u32,
                registers: a[2],
                aperture: a[3],
                width: a[4] as u32,
                height: a[5] as u32,
                stride: a[6] as u32,
                framebuffer_offset: a[7],
                framebuffer_bytes: a[8],
                clock_hz: a[9],
                dma_bits: a[10] as u8,
            },
        })
    }
    pub fn info(&self) -> Info {
        self.info
    }
    pub fn healthy(&self) -> bool {
        !self.failed
    }
    pub fn scope(&self) -> wire::Scope {
        self.scope
    }
    pub fn owns(&self, page: &crate::memory::DmaPage) -> bool {
        page.matches(self.scope.device, self.scope.connection)
    }
    pub fn config(&mut self, offset: u64) -> Result<u32, isize> {
        let value = self.exchange(op::CONFIG, &[offset], 1)?.args[0];
        if value >= u32::MAX as u64 {
            self.failed = true;
            return Err(-5);
        }
        Ok(value as u32)
    }
    /// Preserved device addresses, never a CPU mapping or an allocation token.
    pub fn reservation(&mut self) -> Result<(u64, u64), isize> {
        let a = self.exchange(op::RESERVATION, &[], 2)?.args;
        if a[0] == 0
            || (a[0] | a[1]) & (wire::PAGE - 1) != 0
            || a[1] <= a[0]
            || a[1] > 1u64 << self.info.dma_bits
        {
            self.failed = true;
            return Err(-5);
        }
        Ok((a[0], a[1]))
    }
    fn exchange(&mut self, operation: u64, args: &[u64], outputs: usize) -> Result<Message, isize> {
        if self.failed {
            return Err(-5);
        }
        let mut request = Message::new(operation, self.scope);
        request.args[..args.len()].copy_from_slice(args);
        request.request().map_err(|_| -8isize)?;
        let result = self.transport.exchange(request);
        match result {
            Ok(r)
                if r.operation == operation
                    && r.scope == self.scope
                    && r.args[outputs..].iter().all(|v| *v == 0) =>
            {
                Ok(r)
            }
            Ok(_) => {
                self.failed = true;
                Err(-5)
            }
            Err(e) => {
                self.failed = true;
                Err(e)
            }
        }
    }
    pub fn map_bar(
        &mut self,
        index: u64,
        offset: u64,
        pages: u64,
        cache: u64,
    ) -> Result<u64, isize> {
        if self.ready {
            return Err(-8);
        }
        let limit = match index {
            0 => self.info.registers,
            2 => self.info.aperture,
            _ => return Err(-8),
        };
        let bytes = pages.checked_mul(wire::PAGE).ok_or(-8isize)?;
        if offset.checked_add(bytes).is_none_or(|n| n > limit) {
            return Err(-8);
        }
        let r = self.exchange(op::MAP_BAR, &[index, offset, pages, cache], 2)?;
        let expected = (if index == 0 {
            wire::MMIO_START
        } else {
            wire::APERTURE_START
        }) + offset;
        if r.args[0] != expected || r.args[1] != bytes {
            self.failed = true;
            return Err(-5);
        }
        Ok(expected)
    }
    /// # Safety
    /// The hardware owner has completed firmware GT retirement and configured
    /// the advertised memory-coherence contract while preserving GOP scanout.
    pub unsafe fn handoff(&mut self) -> Result<(), isize> {
        if self.ready {
            return Err(-8);
        }
        self.exchange(
            op::HANDOFF,
            &[wire::HANDOFF_VERSION, wire::HANDOFF_RETIRED_COHERENT],
            0,
        )?;
        self.ready = true;
        Ok(())
    }
    pub fn allocate(&mut self, pages: u64, writable: bool) -> Result<Allocation, isize> {
        if !self.ready {
            return Err(-8);
        }
        let r = self.exchange(op::ALLOCATE, &[pages, u64::from(writable)], 4)?;
        let a = r.args;
        let bytes = pages * wire::PAGE;
        if a[0] == 0
            || a[3] != bytes
            || a[1] < wire::DMA_START
            || a[1]
                .checked_add(bytes)
                .is_none_or(|n| n > wire::DMA_START + wire::DMA_BYTES)
            || (a[1] | a[2]) & 4095 != 0
            || a[2] == 0
            || a[2]
                .checked_add(bytes)
                .is_none_or(|n| n > 1 << self.info.dma_bits)
        {
            self.failed = true;
            return Err(-5);
        }
        Ok(Allocation {
            scope: self.scope,
            handle: a[0],
            cpu: a[1],
            iova: a[2],
            bytes,
        })
    }
    pub fn sync(&mut self, a: &Allocation) -> Result<(), isize> {
        if a.scope != self.scope {
            return Err(-8);
        }
        self.exchange(op::SYNC, &[a.handle], 0).map(|_| ())
    }
    pub fn revoke_grant(&mut self, a: &Allocation) -> Result<(), isize> {
        if a.scope != self.scope {
            return Err(-8);
        }
        self.exchange(op::REVOKE_GRANT, &[a.handle], 0).map(|_| ())
    }
    pub fn grant(
        &mut self,
        a: &Allocation,
        peer: crate::protocol::backend::Peer,
        writable: bool,
    ) -> Result<Grant, isize> {
        if a.scope != self.scope
            || peer.device != self.scope.device
            || peer.connection != self.scope.connection
        {
            return Err(-8);
        }
        let args = [
            a.handle,
            peer.client,
            peer.renderer,
            peer.session,
            peer.generation,
            u64::from(writable),
        ];
        self.exchange(op::GRANT, &args, 0)?;
        Ok(Grant {
            scope: self.scope,
            handle: a.handle,
            peer,
            bytes: a.bytes,
            writable,
        })
    }
    /// On failure return the token so the owner cannot mistake an uncertain
    /// release for free memory. Allocation has no implicit release in Drop.
    pub fn release(&mut self, a: Allocation) -> Result<(), (isize, Allocation)> {
        if a.scope != self.scope {
            return Err((-8, a));
        }
        self.exchange(op::RELEASE, &[a.handle], 0)
            .map(|_| ())
            .map_err(|e| (e, a))
    }
    pub fn irq(&mut self, enable: bool) -> Result<u64, isize> {
        self.exchange(op::IRQ, &[u64::from(enable)], 1)
            .map(|r| r.args[0])
    }
    pub fn display_snapshot(&mut self, peer:crate::protocol::backend::Peer, tokens:[u64;3])->Result<[u64;6],isize>{
        if !self.ready||!peer.valid()||peer.device!=self.scope.device||peer.connection!=self.scope.connection
            ||tokens.contains(&0)||tokens[0]==tokens[1]||tokens[0]==tokens[2]||tokens[1]==tokens[2]{return Err(-8);}
        let r=self.exchange(op::DISPLAY_CONTROL,&[wire::display::SNAPSHOT,peer.client,peer.renderer,peer.session,peer.generation,tokens[0],tokens[1],tokens[2]],6)?;
        if r.args[..3].iter().any(|s|!(1..=4).contains(s)){self.failed=true;return Err(-5);}
        Ok(r.args[..6].try_into().unwrap())
    }
    pub fn cursor_backing(&mut self,a:&Allocation)->Result<bool,isize>{
        if !self.ready||a.scope!=self.scope||a.bytes()!=8*4096{return Err(-8);}
        let r=self.exchange(op::DISPLAY_CONTROL,&[wire::display::CURSOR_BACKING,a.handle],1)?;
        if r.args[0]>1{self.failed=true;return Err(-5);}Ok(r.args[0]==1)
    }
    /// # Safety
    /// All CPU writers and writable GPU aliases of this opaque object have
    /// retired. Backing stays pinned until publication_alive returns false,
    /// and imported read bindings retain their own separate hardware pins.
    pub unsafe fn publish(&mut self, peer: crate::protocol::backend::Peer, opaque: u64, bytes: u64, fence: u64) -> Result<Publication, isize> {
        if !self.ready || !peer.valid() || peer.device != self.scope.device || peer.connection != self.scope.connection { return Err(-8); }
        let r = self.exchange(op::PUBLISH, &[opaque,peer.client,peer.renderer,peer.session,peer.generation,bytes,fence], 2)?;
        if r.args[0] >> 63 != 1 || r.args[1] >> 63 != 1 || r.args[0] == r.args[1] { self.failed = true; return Err(-5); }
        Ok(Publication { data:r.args[0], fence:r.args[1] })
    }
    pub fn publication_source(&mut self, peer: crate::protocol::backend::Peer, token:u64) -> Result<PublishedSource,isize> {
        if !self.ready || !peer.valid() || token >> 63 != 1 { return Err(-8); }
        let r = self.exchange(op::PUBLICATION_LOOKUP, &[token,peer.client,peer.renderer,peer.session,peer.generation], 8)?;
        let a=r.args;
        let origin=crate::protocol::backend::Peer{client:a[4],renderer:a[5],session:a[6],generation:a[7],device:self.scope.device,connection:self.scope.connection};
        if a[..3].contains(&0) || a[1]%4096!=0 || !matches!(a[3],1|2) || !origin.valid(){self.failed=true;return Err(-5);}
        Ok(PublishedSource{opaque:a[0],bytes:a[1],fence:a[2],kind:a[3],peer:origin})
    }
    pub fn drop_publication(&mut self, peer:crate::protocol::backend::Peer, token:u64)->Result<(),isize>{
        if !self.ready || !peer.valid() || token>>63!=1{return Err(-8);}
        self.exchange(op::PUBLICATION_DROP,&[token,peer.client,peer.renderer,peer.session,peer.generation],0).map(|_|())
    }
    pub fn publication_alive(&mut self,opaque:u64)->Result<bool,isize>{
        if !self.ready || opaque==0{return Err(-8);}
        let r=self.exchange(op::PUBLICATION_ALIVE,&[opaque],1)?;
        if r.args[0]>1{self.failed=true;return Err(-5);}Ok(r.args[0]!=0)
    }
    /// Backend display transaction: 1 register, 2 exclude GOP writers before
    /// flip, 3 observed flip/vblank/fence, 4 observed GOP restore, 5 release.
    /// # Safety
    /// Phases 3/4 require actual hardware retirement, never a queued command.
    pub unsafe fn display(&mut self,phase:u64,peer:crate::protocol::backend::Peer,buffer:u64,serial:u64,fence:u64)->Result<(),isize>{
        if !self.ready||!peer.valid()||peer.device!=self.scope.device||peer.connection!=self.scope.connection{return Err(-8);}
        self.exchange(op::DISPLAY,&[phase,peer.client,peer.renderer,peer.session,peer.generation,buffer,serial,fence],0).map(|_|())
    }
    /// Versioned kernel display owner. No physical address or MMIO receipt is
    /// accepted from the backend; page lists refer to broker allocation tokens.
    pub fn display_control(&mut self, action:u64, peer:Option<crate::protocol::backend::Peer>, buffer:u64, value:u64)->Result<[u64;16],isize>{
        use wire::display;
        let mut args=[0;8]; args[0]=action;
        if action>display::INITIALIZE {
            let p=peer.filter(|p|p.valid()&&p.device==self.scope.device&&p.connection==self.scope.connection).ok_or(-8isize)?;
            args[1..6].copy_from_slice(&[p.client,p.renderer,p.session,p.generation,buffer]);
            args[6]=value;
        }
        self.exchange(op::DISPLAY_CONTROL,&args,16).map(|m|m.args)
    }
    pub fn display_pages(&mut self, peer:crate::protocol::backend::Peer, buffer:u64, offset:u64, allocation:&Allocation)->Result<(),isize>{
        if !self.ready || allocation.scope!=self.scope || !peer.valid() || peer.device!=self.scope.device || peer.connection!=self.scope.connection{return Err(-8);}
        self.exchange(op::DISPLAY_CONTROL,&[wire::display::PAGES,peer.client,peer.renderer,peer.session,peer.generation,buffer,offset,allocation.handle],0).map(|_|())
    }
    pub fn revoke_step(&mut self) -> Result<bool, isize> {
        let r = self.exchange(op::REVOKE, &[], 1)?;
        if r.args[0] > 1 {
            self.failed = true;
            return Err(-5);
        }
        if r.args[0] == 1 {
            self.failed = true;
            self.ready = false;
        }
        Ok(r.args[0] == 1)
    }
}

/// An opaque CPU grant carried in the family RPC reply. It contains no CPU,
/// physical or device address; the broker validates the full session again.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Grant {
    pub scope: Scope,
    pub handle: u64,
    pub peer: crate::protocol::backend::Peer,
    pub bytes: u64,
    pub writable: bool,
}
impl Grant {
    fn request(self, operation: u64) -> Message {
        let mut m = Message::new(operation, self.scope);
        m.args[..5].copy_from_slice(&[
            self.handle,
            self.peer.client,
            self.peer.renderer,
            self.peer.session,
            self.peer.generation,
        ]);
        m
    }
}
/// Renderer-side CPU mapping. No allocation, BAR, IRQ or IOMMU methods exist.
/// A failed receipt stops this channel and preserves every outstanding token.
pub struct PeerMemory<T> {
    transport: T,
    peer: crate::protocol::backend::Peer,
    failed: bool,
}
#[derive(Debug)]
pub struct Mapping {
    grant: Grant,
    address: u64,
}
impl Mapping {
    pub fn address(&self) -> u64 {
        self.address
    }
    pub fn bytes(&self) -> u64 {
        self.grant.bytes
    }
    pub fn writable(&self) -> bool {
        self.grant.writable
    }
}
impl<T: Transport> PeerMemory<T> {
    pub fn new(transport: T, peer: crate::protocol::backend::Peer) -> Self {
        Self {
            transport,
            peer,
            failed: false,
        }
    }
    fn exchange(&mut self, grant: Grant, operation: u64, address: u64) -> Result<Message, isize> {
        if self.failed {
            return Err(-5);
        }
        if grant.peer != self.peer
            || grant.scope.device != self.peer.device
            || grant.scope.connection != self.peer.connection
            || grant.bytes == 0
            || grant.bytes % wire::PAGE != 0
            || grant.bytes > wire::MAX_PAGES * wire::PAGE
        {
            return Err(-8);
        }
        let mut request = grant.request(operation);
        request.args[5] = address;
        request.request().map_err(|_| -8isize)?;
        let result = self.transport.exchange(request).and_then(|r| {
            if r.operation != operation || r.scope != grant.scope {
                return Err(-5);
            }
            let used = if operation == op::MAP_BUFFER { 3 } else { 0 };
            if r.args[used..].iter().any(|v| *v != 0) {
                return Err(-5);
            }
            if operation == op::MAP_BUFFER
                && (r.args[0] < wire::DMA_START
                    || (address != 0 && r.args[0] != address)
                    || r.args[0] % wire::PAGE != 0
                    || r.args[1] != grant.bytes
                    || r.args[2] != u64::from(grant.writable)
                    || r.args[0]
                        .checked_add(grant.bytes)
                        .is_none_or(|n| n > wire::DMA_START + wire::DMA_BYTES))
            {
                return Err(-5);
            }
            Ok(r)
        });
        if result.is_err() {
            self.failed = true;
        }
        result
    }
    pub fn map(&mut self, grant: Grant) -> Result<Mapping, isize> {
        self.map_at(grant, 0)
    }
    /// Bind chunks of a BO into a contiguous renderer VA reservation. The
    /// kernel checks every overlap against all live aliases in this root.
    pub fn map_at(&mut self, grant: Grant, address: u64) -> Result<Mapping, isize> {
        self.exchange(grant, op::MAP_BUFFER, address)
            .map(|r| Mapping {
                grant,
                address: r.args[0],
            })
    }
    pub fn unmap(&mut self, mapping: Mapping) -> Result<(), (isize, Mapping)> {
        self.exchange(mapping.grant, op::UNMAP_BUFFER, 0)
            .map(|_| ())
            .map_err(|e| (e, mapping))
    }
}
