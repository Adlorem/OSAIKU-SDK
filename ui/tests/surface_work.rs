use osaiku_ui::surface::*;

#[test]
fn full_frame_cost_counts_row_packing_and_all_pixel_copies() {
    let extent = Extent {
        width: 1024,
        height: 768,
        stride: 4096,
        format: BGRA8_PREMULTIPLIED,
    };
    assert_eq!(
        FullFrameWork::calculate(extent).unwrap(),
        FullFrameWork {
            pixel_bytes: 3 * 1024 * 1024,
            upload_calls: 48,
            read_calls: 48,
            pixel_copy_bytes: 12 * 1024 * 1024
        }
    );
    let odd = FullFrameWork::calculate(Extent {
        width: 1025,
        stride: 4100,
        ..extent
    })
    .unwrap();
    assert_eq!(odd.upload_calls, 52);
    assert_eq!(odd.read_calls, 49);
    assert!(FullFrameWork::calculate(Extent {
        stride: 1,
        ..extent
    })
    .is_err());
}

#[test]
fn saturated_measurements_are_explicit_instead_of_wrapping() {
    let mut counter = WorkCounters {
        dispatched_calls: u64::MAX,
        ..WorkCounters::EMPTY
    };
    counter.add(WorkCounters {
        dispatched_calls: 1,
        payload_copy_bytes: 42,
        ..WorkCounters::EMPTY
    });
    assert_eq!(counter.dispatched_calls, u64::MAX);
    assert_eq!(counter.payload_copy_bytes, 42);
    assert_eq!(counter.saturated, 1);
}
