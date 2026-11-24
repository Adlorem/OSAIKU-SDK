use super::*;

#[test]
fn rx_snapshots_keep_exact_lengths_and_private_tails() {
    #[repr(align(4096))]
    struct Data([u8; 4097]);
    let mut source = Data(core::array::from_fn(|i| i.wrapping_mul(31) as u8));
    let original = source.0;
    let geometry = Geometry {
        generation: 1,
        capacity: 64,
        slots: 1,
        slot_bytes: 4096,
        mtu: 1500,
    };
    for skip in [0, 1] {
        for length in (14..=24).chain(1510..=1518) {
            source.0 = original;
            let entry = Entry {
                length,
                generation: 1,
                ..Entry::default()
            };
            let mut copy = [0xcc; 1520];
            let n =
                unsafe { copy_rx(source.0.as_ptr().add(skip), geometry, entry, &mut copy[1..]) }
                    .unwrap();
            source.0.fill(0);
            assert_eq!(n, length as usize);
            assert_eq!(&copy[1..1 + n], &original[skip..skip + n]);
            assert_eq!(copy[0], 0xcc);
            assert!(copy[1 + n..].iter().all(|b| *b == 0xcc));
        }
    }
}

#[test]
fn publish_only_commits_and_wakes_for_the_valid_prefix_that_fits() {
    #[repr(align(4096))]
    struct Page([AtomicU32; 1024]);
    let entries = Page([const { AtomicU32::new(0) }; 1024]);
    let indices = Page([const { AtomicU32::new(0) }; 1024]);
    let line = |n: usize| indices.0.as_ptr().wrapping_add(16 * n).cast::<u32>();
    let mut ring = unsafe {
        Ring::mapped(
            entries.0.as_ptr().cast_mut().cast(),
            line(0).cast_mut(),
            line(1),
            line(2).cast_mut(),
            line(3),
            64,
            true,
        )
        .unwrap()
    };
    let geometry = Geometry {
        generation: 1,
        capacity: 64,
        slots: 64,
        slot_bytes: 4096,
        mtu: 1500,
    };
    let mut owner = Ownership::<64>::new(geometry, true).unwrap();
    let mut pending = Returns::<64>::new(64).unwrap();
    for _ in 0..62 {
        ring.push(Entry::default()).unwrap();
    }
    for slot in 0..4 {
        let e = Entry {
            slot,
            generation: 1,
            ..Entry::default()
        };
        if slot != 3 {
            owner.queue_return(e).unwrap();
        }
        pending.push(e, 1, 20).unwrap();
    }
    indices.0[48].store(1, Ordering::Relaxed);
    let mut wake = false;
    ring.publish_returns(&mut pending, &mut owner, &mut wake)
        .unwrap();
    assert!(wake);
    assert_eq!(indices.0[0].load(Ordering::Acquire), 64);
    assert_eq!(pending.front().unwrap().slot, 2);
    wake = false;
    ring.publish_returns(&mut pending, &mut owner, &mut wake)
        .unwrap();
    assert!(!wake);
    indices.0[16].store(2, Ordering::Release);
    assert_eq!(
        ring.publish_returns(&mut pending, &mut owner, &mut wake),
        Err(Error::State)
    );
    assert!(wake);
    assert_eq!(indices.0[0].load(Ordering::Acquire), 65);
    assert_eq!(entries.0[0].load(Ordering::Relaxed), 2);
    assert_eq!(pending.front().unwrap().slot, 3);
    assert!(pending.expired(21));
}
