#![no_std]
#![forbid(unsafe_code)]

//! Owner-side contracts, not a syscall transport or hardware authority.
//! A trusted host retains these ledgers; peer messages cannot instantiate or
//! replace its entries. No Drop operation claims that DMA has stopped.
pub use osaiku_driver_protocol as protocol;
use protocol::{BufferRange, EndpointId, Error, RequestId, ResourceHandle, Scope, Sequence};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LeaseState {
    Prepared,
    Active,
    Quarantined,
    Retired,
}
pub struct ResourceLease {
    handle: ResourceHandle,
    bytes: u64,
    rights: u8,
    state: LeaseState,
}
impl ResourceLease {
    /// Records a resource already admitted by the trusted platform. This does
    /// not allocate/map hardware and must never be called solely from a manifest.
    pub fn admitted(handle: ResourceHandle, bytes: u64, rights: u8) -> Result<Self, Error> {
        if bytes == 0
            || rights == 0
            || rights & !(protocol::DEVICE_READ | protocol::DEVICE_WRITE) != 0
        {
            return Err(Error::Range);
        }
        Ok(Self {
            handle,
            bytes,
            rights,
            state: LeaseState::Prepared,
        })
    }
    pub fn handle(&self) -> ResourceHandle {
        self.handle
    }
    pub fn state(&self) -> LeaseState {
        self.state
    }
    pub fn validate(
        &self,
        handle: ResourceHandle,
        rights: u8,
        range: BufferRange,
    ) -> Result<(), Error> {
        if handle != self.handle {
            return Err(Error::Stale);
        }
        if matches!(self.state, LeaseState::Quarantined | LeaseState::Retired) {
            return Err(Error::State);
        }
        if rights == 0 || rights & !self.rights != 0 {
            return Err(Error::Permission);
        }
        range.validate(self.bytes)
    }
    pub fn activate(&mut self) -> Result<(), Error> {
        if self.state != LeaseState::Prepared {
            return Err(Error::State);
        }
        self.state = LeaseState::Active;
        Ok(())
    }
    pub fn quarantine(&mut self) {
        if self.state != LeaseState::Retired {
            self.state = LeaseState::Quarantined;
        }
    }
    /// The platform calls this only after its device-specific stop proof.
    /// An IPC "Stopped" response or deadline expiration is not that proof.
    pub fn platform_retired(&mut self) {
        self.state = LeaseState::Retired;
    }
    pub fn cancel_prepared(&mut self) -> Result<(), Error> {
        if self.state != LeaseState::Prepared {
            return Err(Error::State);
        }
        self.state = LeaseState::Retired;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum State {
    Detected,
    Waiting,
    Matched,
    Allocated,
    Probing,
    Ready,
    Quiescing,
    Stopped,
    Faulted,
    Quarantined,
    Removed,
}
/// Evidence created by the owner of a real, registered class endpoint.
/// Keeping this record alive is the host's responsibility; provider loss must
/// call dependency_lost before that endpoint can disappear or be reassigned.
pub trait ClassEndpoint {
    fn scope(&self) -> Scope;
    fn id(&self) -> EndpointId;
    fn is_live(&self) -> bool;
}
pub struct EndpointRegistration<'a> {
    endpoint: &'a dyn ClassEndpoint,
}
impl<'a> EndpointRegistration<'a> {
    pub fn registered(endpoint: &'a dyn ClassEndpoint) -> Result<Self, Error> {
        if !endpoint.is_live() {
            return Err(Error::NotReady);
        }
        Ok(Self { endpoint })
    }
}
pub struct Binding {
    scope: Scope,
    state: State,
    endpoint: Option<EndpointId>,
}
impl Binding {
    pub const fn detected(scope: Scope) -> Self {
        Self {
            scope,
            state: State::Detected,
            endpoint: None,
        }
    }
    pub fn state(&self) -> State {
        self.state
    }
    pub fn endpoint(&self) -> Option<EndpointId> {
        self.endpoint
    }
    fn change(&mut self, from: State, to: State) -> Result<(), Error> {
        if self.state != from {
            return Err(Error::State);
        }
        self.state = to;
        Ok(())
    }
    pub fn wait_for_dependency(&mut self) -> Result<(), Error> {
        self.change(State::Detected, State::Waiting)
    }
    pub fn matched(&mut self) -> Result<(), Error> {
        if !matches!(self.state, State::Detected | State::Waiting) {
            return Err(Error::State);
        }
        self.state = State::Matched;
        Ok(())
    }
    pub fn resources_admitted(&mut self) -> Result<(), Error> {
        self.change(State::Matched, State::Allocated)
    }
    pub fn probe(&mut self) -> Result<(), Error> {
        self.change(State::Allocated, State::Probing)
    }
    pub fn ready(&mut self, endpoint: &EndpointRegistration<'_>) -> Result<(), Error> {
        if endpoint.endpoint.scope() != self.scope {
            return Err(Error::Stale);
        }
        if !endpoint.endpoint.is_live() {
            return Err(Error::NotReady);
        }
        self.change(State::Probing, State::Ready)?;
        self.endpoint = Some(endpoint.endpoint.id());
        Ok(())
    }
    pub fn quiesce(&mut self) -> Result<(), Error> {
        if !matches!(
            self.state,
            State::Allocated | State::Probing | State::Ready | State::Faulted
        ) {
            return Err(Error::State);
        }
        self.endpoint = None;
        self.state = State::Quiescing;
        Ok(())
    }
    pub fn dependency_lost(&mut self) -> Result<(), Error> {
        if matches!(
            self.state,
            State::Detected | State::Matched | State::Waiting
        ) {
            self.state = State::Waiting;
            Ok(())
        } else {
            self.quiesce()
        }
    }
    pub fn fault(&mut self) -> Result<(), Error> {
        if matches!(
            self.state,
            State::Stopped | State::Removed | State::Quarantined
        ) {
            return Err(Error::State);
        }
        self.endpoint = None;
        self.state = State::Faulted;
        Ok(())
    }
    pub fn quarantine(&mut self) -> Result<(), Error> {
        if !matches!(self.state, State::Faulted | State::Quiescing) {
            return Err(Error::State);
        }
        self.endpoint = None;
        self.state = State::Quarantined;
        Ok(())
    }
    /// Trusted platform has synchronized IRQ/work and proved DMA idle, and
    /// the request owner has drained/retired its outstanding queue.
    pub fn platform_stopped(&mut self) -> Result<(), Error> {
        if !matches!(self.state, State::Quiescing | State::Quarantined) {
            return Err(Error::State);
        }
        self.state = State::Stopped;
        Ok(())
    }
    pub fn remove(&mut self) -> Result<(), Error> {
        if !matches!(
            self.state,
            State::Detected | State::Waiting | State::Matched | State::Stopped
        ) {
            return Err(Error::State);
        }
        self.state = State::Removed;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Context {
    Service,
    Irq,
    SpinLocked,
}
#[derive(Clone, Copy)]
struct Pending {
    id: RequestId,
    bytes: u64,
    deadline: u64,
    cancelled: bool,
}
/// Bounded metadata only. Buffers stay owned/pinned until completion or a
/// platform stop proof; cancel/deadline merely request cancellation.
pub struct Requests<const N: usize> {
    scope: Scope,
    sequence: Sequence,
    slots: [Option<Pending>; N],
    accepting: bool,
}
impl<const N: usize> Requests<N> {
    pub fn new(scope: Scope) -> Result<Self, Error> {
        if N == 0 || N > protocol::MAX_IN_FLIGHT {
            return Err(Error::Size);
        }
        Ok(Self {
            scope,
            sequence: Sequence::after(0),
            slots: [None; N],
            accepting: true,
        })
    }
    pub fn submit(
        &mut self,
        context: Context,
        bytes: u64,
        now: u64,
        deadline: u64,
    ) -> Result<RequestId, Error> {
        if context != Context::Service {
            return Err(Error::Context);
        }
        if !self.accepting {
            return Err(Error::State);
        }
        if bytes == 0 || bytes > protocol::MAX_BUFFER {
            return Err(Error::Size);
        }
        if deadline <= now {
            return Err(Error::Deadline);
        }
        let slot = self
            .slots
            .iter_mut()
            .find(|slot| slot.is_none())
            .ok_or(Error::Backpressure)?;
        let id = RequestId::new(self.sequence.issue()?).ok_or(Error::Exhausted)?;
        *slot = Some(Pending {
            id,
            bytes,
            deadline,
            cancelled: false,
        });
        Ok(id)
    }
    pub fn cancel(&mut self, scope: Scope, id: RequestId) -> Result<(), Error> {
        if scope != self.scope {
            return Err(Error::Stale);
        }
        let slot = self
            .slots
            .iter_mut()
            .flatten()
            .find(|slot| slot.id == id)
            .ok_or(Error::Stale)?;
        slot.cancelled = true;
        Ok(())
    }
    pub fn expired(&mut self, now: u64) -> usize {
        let mut n = 0;
        for slot in self.slots.iter_mut().flatten() {
            if slot.deadline <= now && !slot.cancelled {
                slot.cancelled = true;
                n += 1;
            }
        }
        n
    }
    /// Call only after the transport has validated actual completion/length.
    /// Returns true when cancellation/deadline was requested. A successful
    /// late write still happened; never replay it automatically.
    pub fn complete(&mut self, scope: Scope, id: RequestId, actual: u64) -> Result<bool, Error> {
        if scope != self.scope {
            return Err(Error::Stale);
        }
        let slot = self
            .slots
            .iter_mut()
            .find(|slot| slot.is_some_and(|p| p.id == id))
            .ok_or(Error::Stale)?;
        let pending = slot.ok_or(Error::Stale)?;
        if actual > pending.bytes {
            return Err(Error::Size);
        }
        *slot = None;
        Ok(pending.cancelled)
    }
    pub fn quiesce(&mut self) {
        self.accepting = false;
    }
    pub fn pending(&self) -> usize {
        self.slots.iter().flatten().count()
    }
    /// Called by the owner after the platform stop proof, never on timeout.
    pub fn platform_stopped(&mut self) {
        self.accepting = false;
        self.slots.fill(None);
    }
}
