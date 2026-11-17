//! Package-private RPC over copied, kernel-authenticated envelopes. Callers
//! supply their accounted scratch buffer; no 64 KiB temporary lives on a stack.
use crate::protocol::{
    backend::{self as wire, op, state, Message, Peer},
    channel::BUSY,
};
pub trait Transport {
    fn exchange(&mut self, packet: &mut [u8]) -> Result<(), isize>;
}
fn put(packet: &mut [u8], m: Message) -> Result<(), isize> {
    packet
        .get_mut(..wire::HEADER)
        .ok_or(-1isize)?
        .copy_from_slice(&m.encode());
    Ok(())
}
fn get(packet: &[u8]) -> Result<Message, isize> {
    Message::decode(packet.get(..wire::HEADER).ok_or(-5isize)?).map_err(|_| -5)
}
fn exchange(
    t: &mut impl Transport,
    failed: &mut bool,
    packet: &mut [u8],
) -> Result<Message, isize> {
    if *failed {
        return Err(-5);
    }
    let result = t.exchange(packet).and_then(|_| get(packet));
    if result.as_ref().is_err_and(|e| *e != BUSY) {
        *failed = true;
    }
    result
}
pub struct Client<T> {
    transport: T,
    peer: Peer,
    pending: Option<u64>,
    failed: bool,
}
impl<T: Transport> Client<T> {
    pub fn new(transport: T, identity: crate::channel::Identity) -> Result<Self, isize> {
        let peer = Peer {
            client: identity.client,
            renderer: identity.renderer,
            session: identity.session,
            generation: identity.generation,
            device: identity.device,
            connection: identity.connection,
        };
        if !peer.valid() {
            return Err(-8);
        }
        Ok(Self {
            transport,
            peer,
            pending: None,
            failed: false,
        })
    }
    pub fn healthy(&self) -> bool {
        !self.failed
    }
    /// One kernel call, completed only by the matching backend reply. BUSY
    /// means no admission; an admitted request is never replayed by this API.
    pub fn call(&mut self, packet: &mut [u8], bytes: usize) -> Result<usize, isize> {
        if self.pending.is_some() { return Err(BUSY); }
        if bytes == 0 || bytes > wire::MAX_PAYLOAD || packet.len() > wire::MAX_BYTES
            || bytes + wire::HEADER > packet.len() { return Err(-1); }
        put(packet, Message { peer:self.peer, length:bytes as u64, ..Message::new(op::CALL) })?;
        let r=exchange(&mut self.transport,&mut self.failed,packet)?;
        if r.operation!=op::CALL || r.peer!=self.peer || r.serial==0 || r.state!=state::DONE
            || r.length==0 || r.length as usize>packet.len()-wire::HEADER {
            self.failed=true;return Err(-5);
        }
        Ok(r.length as usize)
    }
    pub fn send(&mut self, packet: &mut [u8], bytes: usize) -> Result<(), isize> {
        if self.pending.is_some() {
            return Err(BUSY);
        }
        if bytes == 0 || bytes > wire::MAX_PAYLOAD || bytes + wire::HEADER > packet.len() {
            return Err(-1);
        }
        let m = Message {
            peer: self.peer,
            length: bytes as u64,
            ..Message::new(op::SEND)
        };
        put(packet, m)?;
        let r = exchange(
            &mut self.transport,
            &mut self.failed,
            &mut packet[..wire::HEADER + bytes],
        )?;
        if r.operation != op::SEND
            || r.peer != self.peer
            || r.serial == 0
            || r.state != state::QUEUED
            || r.length != 0
        {
            self.failed = true;
            return Err(-5);
        }
        self.pending = Some(r.serial);
        Ok(())
    }
    pub fn poll(&mut self, packet: &mut [u8]) -> Result<usize, isize> {
        if packet.len() > wire::MAX_BYTES {
            return Err(-1);
        }
        let serial = self.pending.ok_or(-5isize)?;
        let m = Message {
            peer: self.peer,
            serial,
            ..Message::new(op::POLL)
        };
        put(packet, m)?;
        let r = exchange(&mut self.transport, &mut self.failed, packet)?;
        if r.operation != op::POLL
            || r.peer != self.peer
            || r.serial != serial
            || r.state != state::DONE
            || r.length == 0
            || r.length as usize > packet.len() - wire::HEADER
        {
            self.failed = true;
            return Err(-5);
        }
        self.pending = None;
        Ok(r.length as usize)
    }
}
/// Move-only response authority. The server cannot acknowledge another peer
/// accidentally by reusing a mutable global request header.
pub struct Ticket(Option<Message>);
impl Ticket {
    pub fn peer(&self) -> Option<Peer> {
        self.0.map(|m| m.peer)
    }
    pub fn serial(&self) -> Option<u64> {
        self.0.map(|m| m.serial)
    }
}
pub struct Server<T> {
    transport: T,
    failed: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PeerAccess {
    pub display: bool,
}
impl<T: Transport> Server<T> {
    pub fn new(transport: T) -> Self {
        Self {
            transport,
            failed: false,
        }
    }
    pub fn healthy(&self) -> bool {
        !self.failed
    }
    pub fn peer_alive(&mut self, peer: Peer) -> Result<bool, isize> {
        self.peer_access(peer).map(|a| a.is_some())
    }
    pub fn peer_access(&mut self, peer: Peer) -> Result<Option<PeerAccess>, isize> {
        let m = Message {
            peer,
            ..Message::new(op::PROBE)
        };
        m.request().map_err(|_| -8isize)?;
        let mut packet = m.encode();
        let r = exchange(&mut self.transport, &mut self.failed, &mut packet)?;
        let live = r.state == state::DONE;
        if !matches!(r.state, state::DONE | state::CLOSED)
            || r.serial > 1
            || (!live && r.serial != 0)
            || r != (Message {
                state: r.state,
                serial: r.serial,
                ..m
            })
        {
            self.failed = true;
            return Err(-5);
        }
        Ok(live.then_some(PeerAccess {
            display: r.serial == 1,
        }))
    }
    pub fn receive(&mut self, packet: &mut [u8]) -> Result<(Ticket, usize), isize> {
        self.receive_op(packet, op::RECEIVE)
    }
    /// Atomic queue check and bounded sleep in the kernel. The timeout keeps
    /// the owner heartbeat/retirement loop live without a check/sleep race.
    pub fn receive_wait(&mut self, packet: &mut [u8]) -> Result<(Ticket, usize), isize> {
        self.receive_op(packet, op::RECEIVE_WAIT)
    }
    pub fn receive_wait_for(&mut self,packet:&mut [u8],millis:u64)->Result<(Ticket,usize),isize>{
        if !(1..=10).contains(&millis){return Err(-1);}
        self.receive_timed(packet,op::RECEIVE_WAIT,millis)
    }
    fn receive_op(&mut self, packet: &mut [u8], operation:u64) -> Result<(Ticket, usize), isize> {
        self.receive_timed(packet,operation,0)
    }
    fn receive_timed(&mut self, packet: &mut [u8], operation:u64,millis:u64) -> Result<(Ticket, usize), isize> {
        if packet.len() > wire::MAX_BYTES {
            return Err(-1);
        }
        put(packet, Message{serial:millis,..Message::new(operation)})?;
        let r = exchange(&mut self.transport, &mut self.failed, packet)?;
        if r.operation != op::RECEIVE
            || !r.peer.valid()
            || r.serial == 0
            || r.state != state::RUNNING
            || r.length == 0
            || r.length as usize > packet.len() - wire::HEADER
        {
            self.failed = true;
            return Err(-5);
        }
        Ok((Ticket(Some(r)), r.length as usize))
    }
    pub fn reply(
        &mut self,
        ticket: &mut Ticket,
        packet: &mut [u8],
        bytes: usize,
    ) -> Result<(), isize> {
        let old = ticket.0.ok_or(-5isize)?;
        if bytes == 0 || bytes > wire::MAX_PAYLOAD || bytes + wire::HEADER > packet.len() {
            return Err(-1);
        }
        let m = Message {
            operation: op::REPLY,
            serial: old.serial,
            peer: old.peer,
            state: 0,
            length: bytes as u64,
        };
        put(packet, m)?;
        let r = exchange(
            &mut self.transport,
            &mut self.failed,
            &mut packet[..wire::HEADER + bytes],
        )?;
        if r != (Message {
            state: state::DONE,
            length: 0,
            ..m
        }) {
            self.failed = true;
            return Err(-5);
        }
        ticket.0 = None;
        Ok(())
    }
}
