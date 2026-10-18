use osaiku_runtime::context::system::*;
use std::cell::Cell;
const ACTOR: u64 = (2 << 32) | 4;
struct Source {
    rows: Vec<Option<Entity>>,
    reads: Cell<usize>,
    fail_at: Option<usize>,
    now: Result<u64, Reason>,
}
impl Source {
    fn new(count: usize) -> Self {
        Self {
            rows: (0..count)
                .map(|i| {
                    let mut e = Entity::new(PROCESS, i as u64 + 1, 3);
                    e.sampled_ms = 100;
                    e.valid_for_ms = 1000;
                    Some(e)
                })
                .collect(),
            reads: Cell::new(0),
            fail_at: None,
            now: Ok(100),
        }
    }
}
impl Provider for Source {
    fn now_ms(&self) -> Result<u64, Reason> {
        self.now
    }
    fn slots(&self, _: u32) -> Result<usize, Reason> {
        Ok(self.rows.len())
    }
    fn read(&self, _: u32, index: usize) -> Result<Option<Entity>, Reason> {
        self.reads.set(self.reads.get() + 1);
        if self.fail_at == Some(index) {
            Err(Reason::Busy)
        } else {
            Ok(self.rows[index])
        }
    }
}
fn refused(s: &Source, q: &Query, scopes: u32, reason: Reason) {
    let p = query(s, ACTOR, scopes, q);
    p.validate(q, ACTOR).unwrap();
    assert_eq!(p.evidence.decision, 2);
    assert_eq!(p.evidence.reason, reason as u32);
    assert_eq!(p.entities, [Entity::default(); PAGE_SIZE]);
    assert_eq!(p.count, 0);
    assert_eq!(p.next, 0);
}
#[test]
fn paginated_projection_preserves_identity_and_binds_correlation() {
    let s = Source::new(11);
    let mut q = Query::first(PROCESS, 42);
    let mut ids = Vec::new();
    loop {
        let p = query(&s, ACTOR, SYSTEM_READ, &q);
        p.validate(&q, ACTOR).unwrap();
        assert_eq!(p.evidence.decision, 1);
        ids.extend(
            p.entities[..p.count as usize]
                .iter()
                .map(|e| (e.id, e.generation)),
        );
        if p.next == 0 {
            break;
        }
        q.cursor = p.next;
        q.revision = p.evidence.revision;
        q.request_id += 1;
    }
    assert_eq!(ids, (1..=11).map(|id| (id, 3)).collect::<Vec<_>>());
    assert_eq!(s.reads.get(), 33);
}
#[test]
fn sparse_slots_and_small_limit_do_not_repeat_or_skip_entities() {
    let mut s = Source::new(9);
    for i in [0, 2, 3, 5, 8] {
        s.rows[i] = None;
    }
    let mut q = Query::first(PROCESS, 1);
    q.limit = 1;
    let mut ids = Vec::new();
    loop {
        let p = query(&s, ACTOR, SYSTEM_READ, &q);
        p.validate(&q, ACTOR).unwrap();
        ids.push(p.entities[0].id);
        if p.next == 0 {
            break;
        }
        q.cursor = p.next;
        q.revision = p.evidence.revision;
    }
    assert_eq!(ids, [2, 5, 7, 8]);
}
#[test]
fn authority_is_checked_before_any_provider_access() {
    let s = Source::new(5);
    refused(
        &s,
        &Query::first(PROCESS, 1),
        WINDOW_READ,
        Reason::Permission,
    );
    assert_eq!(s.reads.get(), 0);
    let p = query(&s, 0, SYSTEM_READ, &Query::first(PROCESS, 1));
    assert_eq!(p.evidence.reason, Reason::Permission as u32);
}
#[test]
fn instance_replacement_invalidates_continuation_and_discards_partial_results() {
    let mut s = Source::new(9);
    let mut q = Query::first(PROCESS, 1);
    let p = query(&s, ACTOR, SYSTEM_READ, &q);
    q.cursor = p.next;
    q.revision = p.evidence.revision;
    s.rows[8].as_mut().unwrap().generation += 1;
    refused(&s, &q, SYSTEM_READ, Reason::Stale);
}
#[test]
fn provider_failure_does_not_expose_previous_or_partial_data() {
    let mut s = Source::new(9);
    s.fail_at = Some(7);
    refused(&s, &Query::first(PROCESS, 1), SYSTEM_READ, Reason::Busy);
    assert_eq!(s.reads.get(), 8);
}
#[test]
fn budgets_unknown_versions_and_overflow_are_refused_before_scan() {
    let s = Source::new(MAX_SCAN + 1);
    refused(&s, &Query::first(PROCESS, 1), SYSTEM_READ, Reason::Budget);
    for case in 0..7 {
        let mut q = Query::first(PROCESS, 1);
        match case {
            0 => q.version = 999,
            1 => q.limit = 0,
            2 => q.limit = u64::MAX,
            3 => q.cursor = u64::MAX,
            4 => q.cursor = 1,
            5 => q.max_age_ms = u64::MAX,
            _ => q.source = 99,
        }
        refused(&s, &q, SYSTEM_READ, Reason::Invalid);
    }
    assert_eq!(s.reads.get(), 0);
}
#[test]
fn freshness_and_missing_clock_are_explicit() {
    let mut s = Source::new(1);
    let mut q = Query::first(PROCESS, 1);
    q.max_age_ms = 1;
    s.now = Ok(102);
    refused(&s, &q, SYSTEM_READ, Reason::Stale);
    s.now = Ok(99);
    refused(&s, &q, SYSTEM_READ, Reason::Stale);
    s.now = Err(Reason::Unavailable);
    refused(&s, &q, SYSTEM_READ, Reason::Unavailable);
}
#[test]
fn requesting_older_data_cannot_extend_the_sources_lifetime() {
    let mut s = Source::new(1);
    let mut q = Query::first(PROCESS, 1);
    q.max_age_ms = MAX_AGE_MS;
    s.rows[0].as_mut().unwrap().valid_for_ms = 10;
    s.now = Ok(110);
    let page = query(&s, ACTOR, SYSTEM_READ, &q);
    assert_eq!(page.evidence.decision, 1);
    page.validate(&q, ACTOR).unwrap();
    assert_eq!(page.entities[0].valid_for_ms, 10);
    s.now = Ok(111);
    refused(&s, &q, SYSTEM_READ, Reason::Stale);
    s.rows[0].as_mut().unwrap().valid_for_ms = 0;
    s.now = Ok(100);
    assert_eq!(query(&s, ACTOR, SYSTEM_READ, &q).evidence.decision, 1);
    s.now = Ok(101);
    refused(&s, &q, SYSTEM_READ, Reason::Stale);
}

#[test]
fn corrupted_or_misbound_transport_is_scrubbed() {
    let s = Source::new(2);
    let q = Query::first(PROCESS, 55);
    for case in 0..6 {
        let mut out = Page::default();
        let r = read_with(&q, ACTOR, &mut out, |q, p| {
            *p = query(&s, ACTOR, SYSTEM_READ, q);
            match case {
                0 => p.entities[0].values[7] = 0xdeadbeef,
                1 => p.count = u32::MAX,
                2 => p.evidence.actor += 1,
                3 => p.evidence.request_id += 1,
                4 => p.next = u64::MAX,
                _ => return -14,
            }
            0
        });
        assert!(r.is_err());
        assert_eq!(out, Page::default());
    }
}
#[test]
fn source_shape_cannot_smuggle_unused_fields_or_invalid_relations() {
    let mut s = Source::new(1);
    s.rows[0].as_mut().unwrap().values[7] = 1;
    refused(&s, &Query::first(PROCESS, 1), SYSTEM_READ, Reason::Invalid);
    s.rows[0].as_mut().unwrap().values[7] = 0;
    s.rows[0].as_mut().unwrap().related_id = 3;
    refused(&s, &Query::first(PROCESS, 1), SYSTEM_READ, Reason::Invalid);
}
#[test]
fn catalog_is_static_typed_bounded_and_never_an_execution_grant() {
    assert_eq!(CATALOG.len(), 6);
    for (i, op) in CATALOG.iter().enumerate() {
        assert_eq!(op.id as usize, i + 1);
        assert_eq!(op.version, 1);
        assert_ne!(op.required_scope, 0);
        assert!(op.max_bytes <= 65536);
        assert!(op.parameters.iter().all(|p| matches!(
            p.value_type(),
            ValueType::SourceEnum
                | ValueType::Unsigned64
                | ValueType::WindowTarget
                | ValueType::Rectangle
                | ValueType::ProcessEndpoint
                | ValueType::DisclosureEnum
        )));
    }
    assert_eq!(CATALOG[0].shape, Shape::EntityPage);
    assert!(!CATALOG[0].mutates_capture);
}
