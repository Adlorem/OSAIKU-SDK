//! Bound client for the kernel-owned lifecycle endpoint. Identity is obtained
//! from that endpoint; constructor arguments cannot select another GPU/role.
use crate::protocol::control::{op, Message};

pub trait Transport {
    fn exchange(&mut self, request: Message) -> Result<Message, isize>;
}

pub struct Process<T> {
    transport: T,
    identity: Message,
    failed: bool,
}
impl<T: Transport> Process<T> {
    pub fn connect(mut transport: T) -> Result<Self, isize> {
        let identity = transport.exchange(Message::new(op::IDENTITY))?;
        if identity.operation != op::IDENTITY
            || !matches!(identity.role, 4 | 5)
            || identity.attempt == 0
            || identity.selection == 0
            || identity.device == 0
            || identity.connection == 0
            || identity.sequence != 0
            || identity.value != 0
            || identity.digest == [0; 32]
        {
            return Err(-5);
        }
        Ok(Self {
            transport,
            identity,
            failed: false,
        })
    }
    pub fn identity(&self) -> Message {
        self.identity
    }
    fn send(&mut self, operation: u64, sequence: u64, value: u64) -> Result<(), isize> {
        if self.failed {
            return Err(-5);
        }
        let request = Message {
            operation,
            sequence,
            value,
            ..self.identity
        };
        match self.transport.exchange(request) {
            Ok(reply) if reply == request => Ok(()),
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
    pub fn heartbeat(&mut self) -> Result<(), isize> {
        self.send(op::HEARTBEAT, 0, 0)
    }
    pub fn draining(&mut self) -> Result<bool, isize> {
        if self.failed {
            return Err(-5);
        }
        let request = Message {
            operation: op::STATUS,
            ..self.identity
        };
        match self.transport.exchange(request) {
            Ok(reply) if reply.value <= 1 && Message { value: 0, ..reply } == request => {
                Ok(reply.value == 1)
            }
            _ => {
                self.failed = true;
                Err(-5)
            }
        }
    }
    /// After actual engine stop, firmware restore and platform revocation.
    pub fn stopped(&mut self) -> Result<(), isize> {
        if self.identity.role != 4 {
            return Err(-8);
        }
        self.send(op::STOPPED, 0, 0)
    }
    /// # Safety
    /// Sole backend has stopped firmware GT/uC, verified unchanged display,
    /// PAT, reservation and GGTT, masked IRQ, released wake and revoked every
    /// CPU mapping. No HANDOFF was attempted and no DMA was allocated. This
    /// is terminal; report FAILURE afterwards, never resume GPU access.
    pub unsafe fn firmware_aborted(&mut self) -> Result<(), isize> {
        if self.identity.role != 4 {
            return Err(-8);
        }
        self.send(op::FIRMWARE_ABORTED, 0, 0)
    }
    /// Only the backend may report a fence actually observed in its hardware.
    /// The kernel independently requires the live platform hardware grant.
    pub fn fence(&mut self, sequence: u64) -> Result<(), isize> {
        if self.identity.role != 4 || sequence == 0 {
            return Err(-8);
        }
        self.send(op::FENCE, sequence, 0)
    }
    pub fn presented(&mut self, sequence: u64, fence: u64) -> Result<(), isize> {
        if self.identity.role != 4 || sequence == 0 || fence == 0 {
            return Err(-8);
        }
        self.send(op::PRESENT, sequence, fence)
    }
    pub fn fail(&mut self) -> Result<(), isize> {
        let result = self.send(op::FAILURE, 0, 0);
        self.failed = true;
        result
    }
}
