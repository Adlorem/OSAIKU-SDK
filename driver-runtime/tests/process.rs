use osaiku_driver_runtime::{
    process::*,
    protocol::{Error, Scope},
};
fn scope() -> Scope {
    Scope::new(7, 8, 9).unwrap()
}
fn ready(role: Role) -> Instance {
    let mut i = Instance::admitted(role, scope(), 1, 1, 10);
    i.authorize(role, &Message::new(op::HELLO, scope()), 11)
        .unwrap();
    assert_eq!(i.phase, Phase::Initializing);
    i.authorize(role, &Message::new(op::READY, scope()), 12)
        .unwrap();
    i
}
#[test]
fn wire_rejects_versions_reserved_and_overflow() {
    let m = Message::new(op::HELLO, scope());
    assert_eq!(Message::decode(&m.encode()), Ok(m));
    for at in 0..8 {
        let mut b = m.encode();
        b[at] ^= 1;
        assert_eq!(Message::decode(&b), Err(Error::Version));
    }
    for at in 88..DATA {
        let mut b = m.encode();
        b[at] = 1;
        assert_eq!(Message::decode(&b), Err(Error::Reserved));
    }
    let mut m = m;
    m.offset = u64::MAX;
    m.length = 2;
    assert_eq!(Message::decode(&m.encode()), Err(Error::Range));
}
#[test]
fn no_early_ready_or_endpoint_publication() {
    let mut i = Instance::admitted(Role::Driver, scope(), 1, 1, 0);
    for op in [op::READY, op::SUBMIT, op::TAKE] {
        assert!(i
            .authorize(Role::Driver, &Message::new(op, scope()), 1)
            .is_err());
    }
    assert_eq!(i.phase, Phase::Starting);
}
#[test]
fn role_assignment_release_and_generation_are_independent() {
    for field in 0..5 {
        let mut i = ready(Role::Driver);
        let mut m = Message::new(op::SUBMIT, scope());
        match field {
            0 => m.scope = Scope::new(8, 8, 9).unwrap(),
            1 => m.scope = Scope::new(7, 9, 9).unwrap(),
            2 => m.scope = Scope::new(7, 8, 10).unwrap(),
            3 => m.release = 2,
            _ => m.endpoint = 2,
        };
        assert_eq!(i.authorize(Role::Driver, &m, 20), Err(Error::Permission));
    }
    let mut i = ready(Role::Driver);
    assert_eq!(
        i.authorize(Role::Manager, &Message::new(op::SUBMIT, scope()), 20),
        Err(Error::Permission)
    );
    assert!(i
        .authorize(Role::Driver, &Message::new(op::RECONCILE, scope()), 20)
        .is_err());
}
#[test]
fn copied_payload_cannot_change_after_admission() {
    let mut q = Copies::new();
    let mut m = Message::new(op::SUBMIT, scope());
    m.length = PAYLOAD as u64;
    m.payload.fill(42);
    let id = q.submit(m).unwrap();
    m.payload.fill(99);
    let got = q.take(id).unwrap();
    assert_eq!(got.payload, [42; PAYLOAD]);
    assert!(q.take(id).is_err());
    assert!(q.take(id + 1).is_err());
}
#[test]
fn bounded_queue_flood_preserves_other_devices() {
    let mut a = Copies::new();
    let mut b = Copies::new();
    let m = Message::new(op::SUBMIT, scope());
    let ids: Vec<_> = (0..SLOTS).map(|_| a.submit(m).unwrap()).collect();
    assert_eq!(a.submit(m), Err(Error::Backpressure));
    assert!(b.submit(m).is_ok());
    a.take(ids[0]).unwrap();
    let newer = a.submit(m).unwrap();
    assert!(newer > *ids.last().unwrap());
    a.close();
    assert!(a.take(ids[1]).is_err());
    assert!(a.submit(m).is_err());
}
#[test]
fn death_in_each_phase_revokes_admission_but_requires_platform_stop() {
    for stage in 0..3 {
        let mut i = Instance::admitted(Role::Driver, scope(), 1, 1, 0);
        if stage >= 1 {
            i.authorize(Role::Driver, &Message::new(op::HELLO, scope()), 1)
                .unwrap();
        }
        if stage >= 2 {
            i.authorize(Role::Driver, &Message::new(op::READY, scope()), 2)
                .unwrap();
        }
        assert!(i.poll(3, false));
        assert_eq!(i.phase, Phase::Quarantined);
        assert_eq!(i.failure, Failure::ProcessLost);
        assert!(i
            .authorize(Role::Driver, &Message::new(op::HEARTBEAT, scope()), 4)
            .is_err());
        i.platform_stopped();
        assert_eq!(i.phase, Phase::Stopped);
        assert_eq!(i.failure, Failure::ProcessLost);
    }
}
#[test]
fn never_yielding_and_never_initializing_peers_expire() {
    let mut i = Instance::admitted(Role::Driver, scope(), 1, 1, 0);
    assert!(i.poll(START_TIMEOUT, true));
    assert_eq!(i.failure, Failure::StartTimeout);
    let mut i = ready(Role::Driver);
    assert!(!i.poll(12 + WATCHDOG - 1, true));
    assert!(i.poll(12 + WATCHDOG, true));
    assert_eq!(i.failure, Failure::Watchdog);
}
#[test]
fn heartbeat_does_not_extend_start_and_flood_is_terminal() {
    let mut i = Instance::admitted(Role::Driver, scope(), 1, 1, 0);
    assert!(i
        .authorize(
            Role::Driver,
            &Message::new(op::HEARTBEAT, scope()),
            START_TIMEOUT - 1
        )
        .is_err());
    assert!(i.poll(START_TIMEOUT, true));
    let mut i = ready(Role::Driver);
    for _ in 2..MAX_CALLS {
        i.authorize(Role::Driver, &Message::new(op::HEARTBEAT, scope()), 20)
            .unwrap();
    }
    assert_eq!(
        i.authorize(Role::Driver, &Message::new(op::HEARTBEAT, scope()), 20),
        Err(Error::Backpressure)
    );
    assert_eq!(i.failure, Failure::Flood);
}
#[test]
fn restart_has_no_replayed_work_or_inherited_ready() {
    let mut old = ready(Role::Driver);
    let id = old
        .copies
        .submit(Message::new(op::SUBMIT, scope()))
        .unwrap();
    old.fail(Failure::ProcessLost);
    let mut new = Instance::admitted(Role::Driver, Scope::new(7, 8, 10).unwrap(), 1, 1, 30);
    assert_eq!(new.phase, Phase::Starting);
    assert!(new.copies.take(id).is_err());
    assert!(new
        .authorize(Role::Driver, &Message::new(op::HELLO, scope()), 31)
        .is_err());
}

#[test]
fn solicited_block_steps_do_not_exhaust_idle_flood_allowance() {
    let mut i = ready(Role::Driver);
    i.admit_block();
    for _ in 0..10_000 {
        for operation in [
            op::HEARTBEAT,
            op::BLOCK_FETCH,
            op::BLOCK_STEP,
            op::HEARTBEAT,
            op::BLOCK_FETCH,
        ] {
            i.authorize(Role::Driver, &Message::new(operation, scope()), 20)
                .unwrap();
        }
        // Only a trusted owner holding the matching one-use receipt does this.
        i.block_progress();
    }
    assert_eq!(i.phase, Phase::Ready);
    let mut rejected = false;
    for _ in 0..8193 {
        if i.authorize(Role::Driver, &Message::new(op::BLOCK_FETCH, scope()), 20)
            .is_err()
        {
            rejected = true;
            break;
        }
    }
    assert!(rejected);
    assert_eq!(i.failure, Failure::Flood);
    i.block_progress();
    assert_eq!(i.phase, Phase::Quarantined);
}

#[test]
fn block_and_input_admissions_are_separate() {
    let mut input = ready(Role::Driver);
    input.admit_hid();
    assert_eq!(
        input.authorize(Role::Driver, &Message::new(op::BLOCK_FETCH, scope()), 20),
        Err(Error::Permission)
    );
    let mut block = ready(Role::Driver);
    block.admit_block();
    assert_eq!(
        block.authorize(Role::Driver, &Message::new(op::HID_READ, scope()), 20),
        Err(Error::Permission)
    );
    block
        .authorize(Role::Driver, &Message::new(op::BLOCK_FETCH, scope()), 20)
        .unwrap();
}
