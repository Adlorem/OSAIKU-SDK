use osaiku_runtime::context::{self, leases::Pool, *};
use osaiku_ui::{
    compiler::compile,
    paint::Scene,
    surface::{Extent, Handle, Publication, Rect, Refusal, Target, BGRA8_PREMULTIPLIED},
    symbol,
};
const OWNER: u64 = (1 << 32) | 8;
const AGENT: u64 = (1 << 32) | 9;
fn info() -> Info {
    Info {
        version: VERSION,
        disclosure: CONTENT,
        token: 1,
        recipient: AGENT,
        handle: Handle {
            target: Target {
                owner: OWNER,
                window: 2,
                incarnation: 3,
                epoch: 4,
            },
            serial: 5,
            revision: 6,
        },
        frame_generation: 7,
        source_hash: 8,
        app_id: 9,
        crop: Rect {
            x: 0,
            y: 0,
            width: 160,
            height: 100,
        },
        extent: Extent {
            width: 160,
            height: 100,
            stride: 640,
            format: BGRA8_PREMULTIPLIED,
        },
        flags: CLIENT_CONTENT | HOST_OVERLAYS_EXCLUDED | OWNER_AUTHORIZED,
        reserved: 0,
    }
}
fn source() -> (osaiku_ui::Document, Publication) {
    let view=compile(r#"<main id="root"><p id="a">Visible data</p><p id="b">Other data</p><p id="hidden" hidden="true">Secret</p></main>"#,
        "main { padding:0px; gap:0px; } p { height:24px; }").unwrap();
    let scene = Scene::new(&view, 160, 100, &[0; osaiku_ui::MAX_NODES]).unwrap();
    let mut p = Publication::EMPTY;
    p.prepare(&scene, info().handle, &[]).unwrap();
    p.source_hash = 8;
    (view, p)
}
fn snapshot() -> Snapshot {
    let (v, p) = source();
    let mut s = Snapshot::default();
    s.project(info(), &p, &v).unwrap();
    s
}
fn label<'a>(s: &'a Snapshot, id: &str) -> Option<&'a str> {
    let e = s.elements[..s.count as usize]
        .iter()
        .find(|e| e.id == symbol(id))?;
    Some(
        std::str::from_utf8(
            &s.labels[e.label_offset as usize..(e.label_offset + e.label_len) as usize],
        )
        .unwrap(),
    )
}
#[test]
fn one_publication_provides_image_identity_semantics_and_provenance() {
    let s = snapshot();
    s.validate().unwrap();
    assert_eq!(s.info, info());
    assert_eq!(label(&s, "a"), Some("Visible data"));
    assert_eq!(label(&s, "hidden"), None);
    assert!(!s.labels.windows(6).any(|b| b == b"Secret"));
    assert!(s.elements[..s.count as usize]
        .iter()
        .filter(|e| e.parent != 0)
        .all(|e| e.parent == symbol("root")));
}
#[test]
fn clipped_text_is_redacted_and_bounds_are_relative_to_crop() {
    let (v, p) = source();
    let mut i = info();
    i.crop = Rect {
        x: 30,
        y: 0,
        width: 80,
        height: 40,
    };
    i.extent = Extent {
        width: 80,
        height: 40,
        stride: 320,
        ..i.extent
    };
    let mut s = Snapshot::default();
    s.project(i, &p, &v).unwrap();
    assert_eq!(label(&s, "a"), Some(""));
    assert!(s.elements[..s.count as usize]
        .iter()
        .all(|e| e.bounds.within(i.extent)));
    assert!(s.elements[..s.count as usize]
        .iter()
        .all(|e| e.flags & REDACTED != 0));
}
#[test]
fn structure_only_does_not_disclose_text_values_or_pixels() {
    let (v, p) = source();
    let mut i = info();
    i.disclosure = STRUCTURE;
    let mut s = Snapshot::default();
    s.project(i, &p, &v).unwrap();
    assert_eq!((s.label_len, s.point_len), (0, 0));
    assert!(s.elements[..s.count as usize]
        .iter()
        .all(|e| e.value == 10001 && e.flags & REDACTED != 0));
    let mut out = vec![0xaa; 640];
    context::pixels(&i, p.extent, &vec![99; 64000], 0, &mut out).unwrap();
    assert!(out.chunks_exact(4).all(|p| p == [0, 0, 0, 255]));
}
#[test]
fn later_overlay_hides_underlying_semantic_content() {
    let (v, mut p) = source();
    let a = p.elements.iter().position(|e| e.id == symbol("a")).unwrap();
    let b = p.elements.iter().position(|e| e.id == symbol("b")).unwrap();
    p.elements[b].bounds = p.elements[a].bounds;
    p.placements[b] = p.placements[a];
    let mut s = Snapshot::default();
    s.project(info(), &p, &v).unwrap();
    assert_eq!(label(&s, "a"), Some(""));
    assert_eq!(label(&s, "b"), Some("Other data"));
}
#[test]
fn mixed_revision_epoch_source_or_geometry_is_refused() {
    let (v, p) = source();
    for case in 0..5 {
        let mut i = info();
        match case {
            0 => i.handle.revision += 1,
            1 => i.handle.target.epoch += 1,
            2 => i.source_hash += 1,
            3 => i.crop.x = 1000,
            _ => i.handle.serial += 1,
        };
        assert!(Snapshot::default().project(i, &p, &v).is_err());
    }
}
#[test]
fn crop_copy_handles_row_boundaries_and_refuses_overflow() {
    let mut i = info();
    i.crop = Rect {
        x: 1,
        y: 1,
        width: 2,
        height: 2,
    };
    i.extent = Extent {
        width: 2,
        height: 2,
        stride: 8,
        format: 1,
    };
    let extent = Extent {
        width: 4,
        height: 4,
        stride: 16,
        format: 1,
    };
    let src: Vec<u8> = (0..64).collect();
    let mut out = [0; 16];
    context::pixels(&i, extent, &src, 0, &mut out).unwrap();
    assert_eq!(&out[..8], &src[20..28]);
    assert_eq!(&out[8..], &src[36..44]);
    assert_eq!(
        context::pixels(&i, extent, &src, u64::MAX - 3, &mut out),
        Err(Refusal::Bounds)
    );
    assert_eq!(
        context::pixels(&i, extent, &src, 1, &mut out),
        Err(Refusal::Bounds)
    );
}
#[test]
fn owner_grants_only_exact_recipient_and_tokens_never_replay() {
    let mut p = Pool::new();
    assert!(matches!(
        p.insert(AGENT, info(), 64000, ()),
        Err((Refusal::Permission, _))
    ));
    let first = p.insert(OWNER, info(), 64000, ()).unwrap();
    assert!(p.get(OWNER, first.token).is_ok());
    assert!(p.get(AGENT, first.token).is_ok());
    assert!(matches!(
        p.get(AGENT + (1 << 32), first.token),
        Err(Refusal::Permission)
    ));
    assert!(matches!(
        p.take(AGENT + 1, first.token),
        Err(Refusal::Permission)
    ));
    p.take(AGENT, first.token).unwrap();
    let second = p.insert(OWNER, info(), 64000, ()).unwrap();
    assert_ne!(first.token, second.token);
    assert!(matches!(p.get(AGENT, first.token), Err(Refusal::Stale)));
}
#[test]
fn bounded_pin_quota_and_revocation_return_owned_backing() {
    let mut p = Pool::new();
    let a = p.insert(OWNER, info(), 64000, 1).unwrap();
    p.insert(OWNER, info(), 64000, 2).unwrap();
    assert_eq!(p.insert(OWNER, info(), 64000, 3), Err((Refusal::Quota, 3)));
    let retired = p.revoke_one(|i| i.token == a.token).unwrap();
    assert_eq!(retired.backing, 1);
    assert!(p.get(AGENT, a.token).is_err());
    assert!(p.insert(OWNER, info(), 64000, 3).is_ok());
}
#[test]
fn global_retained_bytes_are_charged_not_just_crop_size() {
    let mut p = Pool::new();
    p.insert(OWNER, info(), 64 * 1024 * 1024, ()).unwrap();
    let mut second = info();
    second.handle.target.owner += 1;
    second.recipient += 1;
    assert_eq!(
        p.insert(second.handle.target.owner, second, 4, ()),
        Err((Refusal::Quota, ()))
    );
}
#[test]
fn replacement_does_not_mix_retained_source_and_retirement_drops_pins() {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    #[derive(Debug)]
    struct Pin(Arc<AtomicUsize>);
    impl Drop for Pin {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    let drops = Arc::new(AtomicUsize::new(0));
    let mut p = Pool::new();
    let first = p.insert(OWNER, info(), 64000, Pin(drops.clone())).unwrap();
    let mut next = info();
    next.handle.revision += 1;
    p.insert(OWNER, next, 64000, Pin(drops.clone())).unwrap();
    assert_eq!(p.get(AGENT, first.token).unwrap().info.handle.revision, 6);
    while let Some(pin) = p.revoke_one(|i| i.handle.target.epoch == 4) {
        drop(pin);
    }
    assert_eq!(drops.load(Ordering::SeqCst), 2);
    assert!(p.get(AGENT, first.token).is_err());
}
#[test]
fn receiver_exposes_only_complete_copy_and_cancellation_stops_reads() {
    let mut s = snapshot();
    s.info.extent.height = 200;
    s.info.crop.height = 200;
    let mut bytes = vec![0xaa; 128000];
    let mut r = Receiver::new(&s, &mut bytes).unwrap();
    assert!(matches!(r.finish(), Err(Error::Incomplete)));
    assert!(!r
        .next_with(|req, out| {
            assert_eq!(req.offset, 0);
            out.fill(1);
            0
        })
        .unwrap());
    assert!(matches!(r.finish(), Err(Error::Incomplete)));
    assert!(r
        .next_with(|req, out| {
            assert_eq!(req.offset, 65536);
            out.fill(2);
            0
        })
        .unwrap());
    let result = r.finish().unwrap();
    assert_eq!((result.pixels[0], result.pixels[127999]), (1, 2));
    assert_eq!(r.cancel_with(|_, _| -13), Err(Error::Transport(-13)));
    assert!(r
        .next_with(|_, _| panic!("cancellation pending must not restart reading"))
        .is_err());
    r.cancel_with(|req, out| {
        assert_eq!(req.operation, RELEASE);
        assert!(out.is_empty());
        0
    })
    .unwrap();
    assert!(r.next_with(|_, _| panic!("canceled read")).is_err());
    drop(r);
    assert!(bytes.iter().all(|b| *b == 0));
}
#[test]
fn failed_read_scrubs_partial_image_and_can_retry_from_start() {
    let s = snapshot();
    let mut bytes = vec![1; 64000];
    let mut r = Receiver::new(&s, &mut bytes).unwrap();
    assert_eq!(
        r.next_with(|_, out| {
            out.fill(99);
            -12
        }),
        Err(Error::Transport(-12))
    );
    assert!(r.finish().is_err());
    drop(r);
    assert!(bytes.iter().all(|b| *b == 0));
}
#[test]
fn wire_reply_validation_rejects_token_substitution_and_scrubs_semantics() {
    let mut output = snapshot();
    let old = output.clone();
    assert!(semantics_with(2, &mut output, |_, out| {
        unsafe {
            std::ptr::write_unaligned(out.as_mut_ptr().cast::<Snapshot>(), old.clone());
        }
        0
    })
    .is_err());
    assert_eq!(output.count, 0);
    assert!(output.labels.iter().all(|b| *b == 0));
    assert!(output.validate().is_err());
    let i = info();
    let req = Request::open(
        i.handle.target,
        i.handle.revision,
        i.recipient,
        i.crop,
        CONTENT,
    );
    assert!(open_with(&req, |_, out| {
        let mut wrong = i;
        wrong.recipient += 1;
        unsafe {
            std::ptr::write_unaligned(out.as_mut_ptr().cast::<Info>(), wrong);
        }
        0
    })
    .is_err());
}
#[test]
fn malformed_or_smuggled_records_are_rejected() {
    for case in 0..5 {
        let mut s = snapshot();
        match case {
            0 => s.labels[8191] = 1,
            1 => s.elements[127].id = 8,
            2 => s.elements[1].label_len = u32::MAX,
            3 => s.elements[1].flags |= REDACTED,
            _ => s.info.disclosure = STRUCTURE,
        };
        assert!(s.validate().is_err());
    }
    let mut r = Request::token(PIXELS, 1, 0);
    r.target = info().handle.target;
    assert!(r.validate().is_err());
    let mut r = Request::token(RELEASE, 1, 0);
    r.offset = 4;
    assert!(r.validate().is_err());
}

#[test]
fn shared_chart_samples_are_bounded_and_hidden_gaps_are_zero() {
    let mut view=compile(r#"<main id="root"><ui-chart id="one"></ui-chart><ui-chart id="two"></ui-chart><ui-chart id="three"></ui-chart></main>"#,
        "main { padding:0px; gap:0px; } ui-chart { height:30px; }").unwrap();
    view.point_len = 120;
    for j in 0..120 {
        view.points[j] = osaiku_ui::Point {
            time_ms: j as u64,
            value: 1234,
        };
    }
    for node in view.nodes[1..4].iter_mut() {
        node.series_start = 60;
        node.series_len = 60;
    }
    view.validate().unwrap();
    let scene = Scene::new(&view, 160, 100, &[0; osaiku_ui::MAX_NODES]).unwrap();
    let mut p = Publication::EMPTY;
    p.prepare(&scene, info().handle, &[]).unwrap();
    p.source_hash = 8;
    let mut s = Snapshot::default();
    s.project(info(), &p, &view).unwrap();
    assert_eq!(s.point_len, 120);
    assert!(s.points[..60].iter().all(|p| *p == Point::default()));
    assert!(s.elements[1..4]
        .iter()
        .all(|e| e.point_offset == 60 && e.point_len == 60));
    s.points[0].value = 99;
    assert!(s.validate().is_err());
}
#[test]
fn unused_labels_cannot_smuggle_data_in_structure_only_context() {
    let mut s = snapshot();
    s.info.disclosure = STRUCTURE;
    for e in &mut s.elements[..s.count as usize] {
        e.flags |= REDACTED;
        e.label_offset = 0;
        e.label_len = 0;
        e.value = 10001;
    }
    assert!(s.validate().is_err());
}
#[test]
fn caller_can_select_current_presented_revision_without_accepting_a_mixed_reply() {
    let i = info();
    let request = Request::open(i.handle.target, 0, i.recipient, i.crop, CONTENT);
    let result = open_with(&request, |_, out| {
        unsafe {
            std::ptr::write_unaligned(out.as_mut_ptr().cast::<Info>(), i);
        }
        0
    })
    .unwrap();
    assert_eq!(result.handle.revision, 6);
}

fn semantic_reply(s: &Snapshot, r: &Request, out: &mut [u8]) -> isize {
    assert_eq!(
        r.operation, SEMANTICS,
        "text consumers must never request pixels"
    );
    assert_eq!(r.token, s.info.token);
    unsafe {
        std::ptr::write_unaligned(out.as_mut_ptr().cast::<Snapshot>(), s.clone());
    }
    0
}
#[test]
fn text_consumer_reads_exact_revision_and_scrubs_staging_buffer() {
    let s = snapshot();
    let mut out = Snapshot::default();
    let labels = text::consume_with(
        &info(),
        AGENT,
        99,
        &mut out,
        |r, out| semantic_reply(&s, r, out),
        |text| {
            assert_eq!(text.info(), info());
            assert_eq!(text.evidence().actor, AGENT);
            assert_eq!(text.evidence().object, info().handle.target.window);
            assert_eq!(text.evidence().revision, info().handle.revision);
            text.nodes().map(|n| n.text.to_string()).collect::<Vec<_>>()
        },
    )
    .unwrap();
    assert!(labels.iter().any(|s| s == "Visible data"));
    assert!(!labels.iter().any(|s| s.contains("Secret")));
    assert_eq!(out, Snapshot::default());
}
#[test]
fn text_revocation_close_and_revision_substitution_never_call_consumer() {
    let s = snapshot();
    for case in 0..4 {
        let mut out = s.clone();
        let result = text::consume_with(
            &info(),
            AGENT,
            1,
            &mut out,
            |r, out| {
                if case == 0 {
                    return -12;
                } // closed window
                if case == 1 {
                    return -8;
                } // revoked authority
                let mut wrong = s.clone();
                if case == 2 {
                    wrong.info.handle.revision += 1;
                } else {
                    wrong.info.handle.target.incarnation += 1;
                }
                semantic_reply(&wrong, r, out)
            },
            |_| panic!("stale context must not reach a model"),
        );
        assert!(result.is_err());
        assert_eq!(out, Snapshot::default());
    }
}
#[test]
fn structure_text_has_no_labels_values_or_offered_actions() {
    let (v, p) = source();
    let mut i = info();
    i.disclosure = STRUCTURE;
    let mut s = Snapshot::default();
    s.project(i, &p, &v).unwrap();
    let mut out = Snapshot::default();
    text::consume_with(
        &i,
        AGENT,
        1,
        &mut out,
        |r, out| semantic_reply(&s, r, out),
        |t| {
            assert!(t
                .nodes()
                .all(|n| n.text.is_empty() && n.redacted && n.offered_action.is_none()));
        },
    )
    .unwrap();
}
#[test]
fn image_handoff_revalidates_authority_after_last_chunk() {
    let s = snapshot();
    let mut scratch = Snapshot::default();
    let mut pixels = vec![0; s.info.extent.bytes().unwrap() as usize];
    let mut receiver = Receiver::new(&s, &mut pixels).unwrap();
    while !receiver
        .next_with(|r, out| {
            assert_eq!(r.operation, PIXELS);
            out.fill(0x44);
            0
        })
        .unwrap()
    {}
    assert!(receiver.finish_with(&mut scratch, |_, _| -12).is_err());
    assert!(receiver.finish().is_err());
    drop(receiver);
    assert!(pixels.iter().all(|v| *v == 0));
    assert_eq!(scratch, Snapshot::default());
}

#[test]
fn initial_fixed_service_recipient_does_not_transfer_authority_to_a_restart() {
    let mut i = info();
    i.recipient = 4; // assistantd's exact initial endpoint
    i.validate().unwrap();
    Request::open(
        i.handle.target,
        i.handle.revision,
        i.recipient,
        i.crop,
        CONTENT,
    )
    .validate()
    .unwrap();
    let mut pool = Pool::new();
    let granted = pool.insert(OWNER, i, 64000, ()).unwrap();
    assert!(pool.get(4, granted.token).is_ok());
    assert_eq!(
        pool.get((1 << 32) | 4, granted.token).err(),
        Some(Refusal::Permission)
    );
    assert!(pool.revoke_one(|i| i.recipient == 4).is_some());
    assert_eq!(pool.get(4, granted.token).err(), Some(Refusal::Stale));
}

#[test]
fn refused_text_read_correlates_actor_operation_object_without_stale_evidence() {
    let mut out = snapshot();
    let failure =
        text::consume_recorded_with(&info(), AGENT, 77, &mut out, |_, _| -14, |_| ()).unwrap_err();
    assert_eq!(failure.reason, system::Reason::Timeout);
    assert_eq!(failure.evidence.request_id, 77);
    assert_eq!(failure.evidence.actor, AGENT);
    assert_eq!(failure.evidence.operation, 2);
    assert_eq!(failure.evidence.object, info().handle.target.window);
    assert_eq!(failure.evidence.decision, 2);
    assert_eq!(failure.evidence.digest, 0);
    assert_eq!(failure.evidence.revision, 0);
    assert_eq!(out, Snapshot::default());
}
