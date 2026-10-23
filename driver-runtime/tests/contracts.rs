use osaiku_driver_runtime::*;
use protocol::*;
use std::cell::Cell;

fn scope() -> Scope {
    Scope::new(1, 2, 3).unwrap()
}
fn handle() -> ResourceHandle {
    ResourceHandle {
        scope: scope(),
        id: ResourceId::new(4).unwrap(),
    }
}
struct Endpoint {
    scope: Scope,
    live: Cell<bool>,
}
impl ClassEndpoint for Endpoint {
    fn scope(&self) -> Scope {
        self.scope
    }
    fn id(&self) -> EndpointId {
        EndpointId::new(7).unwrap()
    }
    fn is_live(&self) -> bool {
        self.live.get()
    }
}
fn endpoint() -> Endpoint {
    Endpoint {
        scope: scope(),
        live: Cell::new(true),
    }
}
fn probing() -> Binding {
    let mut b = Binding::detected(scope());
    b.matched().unwrap();
    b.resources_admitted().unwrap();
    b.probe().unwrap();
    b
}
#[test]
fn code_match_grant_probe_and_publication_are_separate() {
    let e = endpoint();
    let registration = EndpointRegistration::registered(&e).unwrap();
    let mut b = Binding::detected(scope());
    assert_eq!(b.ready(&registration), Err(Error::State));
    b.matched().unwrap();
    assert_eq!(b.endpoint(), None);
    assert_eq!(b.probe(), Err(Error::State));
    b.resources_admitted().unwrap();
    assert_eq!(b.ready(&registration), Err(Error::State));
    b.probe().unwrap();
    b.ready(&registration).unwrap();
    assert_eq!(b.endpoint(), Some(e.id()));
    b.quiesce().unwrap();
    assert_eq!(b.endpoint(), None);
    assert_eq!(b.remove(), Err(Error::State));
    b.platform_stopped().unwrap();
    b.remove().unwrap();
    assert_eq!(b.state(), State::Removed);
    assert_eq!(b.matched(), Err(Error::State));
}
#[test]
fn ready_rechecks_endpoint_liveness_and_scope() {
    let e = endpoint();
    let registration = EndpointRegistration::registered(&e).unwrap();
    e.live.set(false);
    assert_eq!(probing().ready(&registration), Err(Error::NotReady));
    assert!(EndpointRegistration::registered(&e).is_err());
    let e = Endpoint {
        scope: Scope::new(1, 2, 4).unwrap(),
        live: Cell::new(true),
    };
    assert_eq!(
        probing().ready(&EndpointRegistration::registered(&e).unwrap()),
        Err(Error::Stale)
    );
}
#[test]
fn partial_initialization_fault_cannot_remove_resources() {
    let mut b = probing();
    b.fault().unwrap();
    assert_eq!(b.remove(), Err(Error::State));
    b.quiesce().unwrap();
    b.quarantine().unwrap();
    assert_eq!(b.remove(), Err(Error::State));
    b.platform_stopped().unwrap();
    b.remove().unwrap();
}
#[test]
fn dependency_loss_withdraws_endpoint_before_cleanup() {
    let mut b = Binding::detected(scope());
    b.wait_for_dependency().unwrap();
    b.matched().unwrap();
    b.dependency_lost().unwrap();
    assert_eq!(b.state(), State::Waiting);
    let e = endpoint();
    let mut b = probing();
    b.ready(&EndpointRegistration::registered(&e).unwrap())
        .unwrap();
    b.dependency_lost().unwrap();
    assert_eq!(b.state(), State::Quiescing);
    assert_eq!(b.endpoint(), None);
}
#[test]
fn active_dma_never_retires_on_cancel_or_quarantine() {
    let mut r = ResourceLease::admitted(handle(), 4096, DEVICE_READ).unwrap();
    r.activate().unwrap();
    assert_eq!(r.cancel_prepared(), Err(Error::State));
    r.quarantine();
    assert_eq!(
        r.validate(
            handle(),
            DEVICE_READ,
            BufferRange {
                offset: 0,
                bytes: 1
            }
        ),
        Err(Error::State)
    );
    assert_eq!(r.state(), LeaseState::Quarantined);
    r.platform_retired();
    assert_eq!(r.activate(), Err(Error::State));
    r.quarantine();
    assert_eq!(r.state(), LeaseState::Retired);
}
#[test]
fn resource_scope_rights_and_bounds_are_checked() {
    let r = ResourceLease::admitted(handle(), 4096, DEVICE_READ).unwrap();
    let mut h = handle();
    h.scope = Scope::new(1, 3, 3).unwrap();
    assert_eq!(
        r.validate(
            h,
            DEVICE_READ,
            BufferRange {
                offset: 0,
                bytes: 1
            }
        ),
        Err(Error::Stale)
    );
    h = handle();
    h.id = ResourceId::new(5).unwrap();
    assert_eq!(
        r.validate(
            h,
            DEVICE_READ,
            BufferRange {
                offset: 0,
                bytes: 1
            }
        ),
        Err(Error::Stale)
    );
    assert_eq!(
        r.validate(
            handle(),
            DEVICE_WRITE,
            BufferRange {
                offset: 0,
                bytes: 1
            }
        ),
        Err(Error::Permission)
    );
    assert_eq!(
        r.validate(
            handle(),
            DEVICE_READ,
            BufferRange {
                offset: 4095,
                bytes: 2
            }
        ),
        Err(Error::Range)
    );
}
#[test]
fn prepared_resource_can_roll_back_and_never_reactivate() {
    let mut r = ResourceLease::admitted(handle(), 4096, DEVICE_READ).unwrap();
    r.cancel_prepared().unwrap();
    assert_eq!(r.activate(), Err(Error::State));
}
#[test]
fn bounded_queue_refuses_irq_spinlock_and_oversized_work() {
    assert!(Requests::<0>::new(scope()).is_err());
    assert!(Requests::<33>::new(scope()).is_err());
    let mut q = Requests::<1>::new(scope()).unwrap();
    for context in [Context::Irq, Context::SpinLocked] {
        assert_eq!(q.submit(context, 1, 0, 2), Err(Error::Context));
    }
    assert_eq!(
        q.submit(Context::Service, MAX_BUFFER + 1, 0, 2),
        Err(Error::Size)
    );
    assert_eq!(q.submit(Context::Service, 1, 2, 2), Err(Error::Deadline));
    q.submit(Context::Service, 1, 0, 2).unwrap();
    assert_eq!(
        q.submit(Context::Service, 1, 0, 2),
        Err(Error::Backpressure)
    );
}
#[test]
fn cancel_and_deadline_retain_slots_and_late_writes_are_reported() {
    let mut q = Requests::<2>::new(scope()).unwrap();
    let a = q.submit(Context::Service, 8, 0, 10).unwrap();
    let b = q.submit(Context::Service, 8, 0, 20).unwrap();
    q.cancel(scope(), a).unwrap();
    assert_eq!(q.expired(20), 1);
    assert_eq!(q.pending(), 2);
    assert_eq!(q.complete(scope(), a, 8), Ok(true));
    assert_eq!(q.complete(scope(), b, 4), Ok(true));
    assert_eq!(q.pending(), 0);
    assert_eq!(q.complete(scope(), a, 8), Err(Error::Stale));
}
#[test]
fn stale_completion_and_invalid_length_do_not_free_slot() {
    let mut q = Requests::<1>::new(scope()).unwrap();
    let id = q.submit(Context::Service, 8, 0, 10).unwrap();
    assert_eq!(
        q.complete(Scope::new(1, 2, 4).unwrap(), id, 8),
        Err(Error::Stale)
    );
    assert_eq!(q.complete(scope(), id, 9), Err(Error::Size));
    assert_eq!(q.pending(), 1);
    q.quiesce();
    assert_eq!(q.submit(Context::Service, 1, 0, 10), Err(Error::State));
    assert_eq!(q.complete(scope(), id, 8), Ok(false));
    q.platform_stopped();
    assert_eq!(q.submit(Context::Service, 1, 0, 10), Err(Error::State));
}
#[test]
fn reused_queue_slot_never_reuses_request_identity() {
    let mut q = Requests::<1>::new(scope()).unwrap();
    let a = q.submit(Context::Service, 1, 0, 10).unwrap();
    q.complete(scope(), a, 1).unwrap();
    let b = q.submit(Context::Service, 1, 0, 10).unwrap();
    assert_ne!(a, b);
    assert_eq!(q.complete(scope(), a, 1), Err(Error::Stale));
    assert_eq!(q.pending(), 1);
}
