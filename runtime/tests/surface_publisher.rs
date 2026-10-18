use osaiku_runtime::{abi::*, surface::*};
use osaiku_ui::{compiler::compile, layout::Layout, paint::Scene};

fn target() -> Target {
    Target {
        owner: (1 << 32) | 8,
        window: 1,
        incarnation: 1,
        epoch: 1,
    }
}

#[test]
fn publisher_state_stays_small_and_borrows_bulk_storage() {
    // Streaming adds a base handle and a prepared-band fingerprint. Keep the
    // continuation within 1/8 of a 4 KiB stack page; pixels, metadata and the
    // dirty tracker must remain borrowed, never embedded in this state.
    assert!(core::mem::size_of::<SurfacePublisher<'_, '_, Layout>>() <= 512);
}

fn offer() -> GeometryOffer {
    GeometryOffer {
        version: VERSION,
        theme: 0,
        target: target(),
        resize_generation: 3,
        window_width: 2,
        window_height: 3,
        content: Rect {
            x: 0,
            y: 1,
            width: 2,
            height: 2,
        },
        frame_layout_hash: 1,
    }
}

#[test]
fn armed_frame_uses_kernel_revision_and_checks_request_echoes() {
    let frame = FrameOffer {
        geometry: offer(),
        request: RenderRequest {
            request_id: 41,
            request_hash: 42,
            arm_generation: 43,
            revision: 9,
        },
    };
    for corrupt in [false, true] {
        let result = frame_offer_with(target(), 41, 42, |req, bytes, receipt| {
            assert_eq!(req.operation, FRAME_OFFER);
            assert_eq!((req.handle.revision, req.offset), (41, 42));
            *receipt = Receipt {
                handle: req.handle,
                epoch: target().epoch,
                charged_bytes: 0,
            };
            let mut returned = frame;
            if corrupt {
                returned.request.request_hash += 1;
            }
            unsafe {
                core::ptr::write_unaligned(bytes.as_mut_ptr().cast::<FrameOffer>(), returned);
            }
            SYSCALL_OK
        });
        assert_eq!(
            result,
            if corrupt {
                Err(PublishError::Receipt)
            } else {
                Ok(frame)
            }
        );
    }
    let doc = compile("<main id=\"r\"></main>", "").unwrap();
    let mut layout = Layout::EMPTY;
    let scene = Scene::prepare(&doc, 2, 2, &mut layout).unwrap();
    let mut metadata = Publication::EMPTY;
    let mut band = [0; 8];
    let mut producer =
        SurfacePublisher::from_frame(&scene, frame, &[], &mut metadata, &mut band).unwrap();
    producer
        .step_with(|req, bytes, receipt| {
            assert_eq!(req.operation, BEGIN_FRAME);
            assert_eq!(req.handle.revision, 9);
            assert_eq!(
                unsafe { core::ptr::read_unaligned(bytes.as_ptr().cast::<FrameOffer>()) },
                frame
            );
            accept(req, bytes, receipt)
        })
        .unwrap();
    producer.cancel_with(accept).unwrap();
    let handle = producer.handle().unwrap();
    for corrupt in [false, true] {
        let observed = frame_request_with(handle, |req, bytes, receipt| {
            assert_eq!(req.operation, FRAME_REQUEST);
            accept(req, bytes, receipt);
            let mut binding = frame.request;
            if corrupt {
                binding.revision += 1;
            }
            unsafe {
                core::ptr::write_unaligned(bytes.as_mut_ptr().cast::<RenderRequest>(), binding);
            }
            SYSCALL_OK
        });
        assert_eq!(
            observed,
            if corrupt {
                Err(PublishError::Receipt)
            } else {
                Ok(frame.request)
            }
        );
    }
}

#[test]
fn geometry_query_checks_shape_identity_and_receipt() {
    for fault in 0..5 {
        let result = geometry_with(target(), |req, bytes, receipt| {
            assert_eq!(req.operation, GEOMETRY);
            assert_eq!(req.handle.serial, 0);
            assert_eq!(req.handle.revision, 0);
            let mut value = offer();
            *receipt = Receipt {
                handle: req.handle,
                epoch: target().epoch,
                charged_bytes: 0,
            };
            match fault {
                1 => value.target.window += 1,
                2 => value.theme = 2,
                3 => receipt.epoch += 1,
                4 => value.resize_generation = 0,
                _ => {}
            }
            unsafe {
                core::ptr::write_unaligned(bytes.as_mut_ptr().cast::<GeometryOffer>(), value);
            }
            SYSCALL_OK
        });
        match fault {
            0 => assert_eq!(result, Ok(offer())),
            1 | 3 => assert_eq!(result, Err(PublishError::Receipt)),
            _ => assert_eq!(result, Err(PublishError::Content(Refusal::Identity))),
        }
    }
}

#[test]
fn offered_publisher_returns_exact_offer_and_refuses_oversized_layout() {
    let doc = compile("<main id=\"r\"></main>", "").unwrap();
    let mut layout = Layout::EMPTY;
    let scene = Scene::prepare(&doc, 2, 2, &mut layout).unwrap();
    let mut metadata = Publication::EMPTY;
    let mut band = [0; 8];
    assert!(matches!(
        SurfacePublisher::from_offer(
            &scene,
            GeometryOffer {
                window_width: 1,
                ..offer()
            },
            1,
            &[],
            &mut metadata,
            &mut band
        ),
        Err(PublishError::Content(Refusal::Bounds))
    ));
    let mut producer =
        SurfacePublisher::from_offer(&scene, offer(), 1, &[], &mut metadata, &mut band).unwrap();
    for status in [SYSCALL_ERR_BUSY, SYSCALL_OK] {
        let result = producer.step_with(|req, bytes, receipt| {
            assert_eq!(req.operation, BEGIN_OFFER);
            assert_eq!(req.handle.target, offer().target);
            assert_eq!(bytes.len(), 80);
            assert_eq!(
                unsafe { core::ptr::read_unaligned(bytes.as_ptr().cast::<GeometryOffer>()) },
                offer()
            );
            accept(req, bytes, receipt);
            status
        });
        if status == SYSCALL_OK {
            assert!(matches!(result, Ok(PublishStep::Begun { .. })));
        } else {
            assert_eq!(result, Err(PublishError::Transport(status)));
        }
    }
    producer.cancel_with(accept).unwrap();
}

#[test]
fn content_offer_requires_exact_size_and_keeps_local_pixel_origin() {
    let offer = offer();
    assert_eq!(
        offer.content_extent().unwrap(),
        Extent {
            width: 2,
            height: 2,
            stride: 8,
            format: BGRA8_PREMULTIPLIED
        }
    );
    for extent in [
        Extent {
            width: 1,
            height: 2,
            stride: 4,
            format: BGRA8_PREMULTIPLIED,
        },
        Extent {
            width: 2,
            height: 3,
            stride: 8,
            format: BGRA8_PREMULTIPLIED,
        },
    ] {
        assert_eq!(offer.admits(extent), Err(Refusal::Bounds));
    }
    let outside = GeometryOffer {
        content: Rect {
            y: u32::MAX,
            ..offer.content
        },
        ..offer
    };
    assert_eq!(outside.validate(), Err(Refusal::Bounds));
    let doc = compile(
        "<main id=\"r\"><button id=\"b\" action=\"go\">OK</button></main>",
        "",
    )
    .unwrap();
    let mut layout = Layout::EMPTY;
    let scene = Scene::prepare(&doc, 2, 2, &mut layout).unwrap();
    let mut metadata = Publication::EMPTY;
    let mut band = [0; 8];
    let mut producer =
        SurfacePublisher::from_offer(&scene, offer, 1, &[], &mut metadata, &mut band).unwrap();
    while !matches!(
        producer.step_with(accept).unwrap(),
        PublishStep::Sealed { .. }
    ) {}
    drop(producer);
    assert_eq!(metadata.extent, offer.content_extent().unwrap());
    assert_eq!(
        metadata.damage[0],
        Rect {
            x: 0,
            y: 0,
            width: 2,
            height: 2
        }
    );
    assert!(metadata.elements[..metadata.element_count as usize]
        .iter()
        .all(|e| e.bounds.within(metadata.extent)));
}
fn accept(req: &Request, _: &mut [u8], receipt: &mut Receipt) -> isize {
    *receipt = Receipt {
        handle: Handle {
            serial: 7,
            ..req.handle
        },
        epoch: req.handle.target.epoch,
        charged_bytes: 0,
    };
    SYSCALL_OK
}

#[test]
fn each_step_is_bounded_and_retains_cursor_on_refusal() {
    let doc = compile(
        "<main id=\"r\"><span id=\"label\">Zażółć</span></main>",
        "main { background-color: #123456; }",
    )
    .unwrap();
    let mut layout = Layout::EMPTY;
    let scene = Scene::prepare(&doc, 1025, 20, &mut layout).unwrap();
    let mut metadata = Publication::EMPTY;
    let mut band = [0xa5; MAX_TRANSFER];
    let mut producer =
        SurfacePublisher::new(&scene, target(), 1, &[], &mut metadata, &mut band).unwrap();
    let mut calls = 0;
    assert_eq!(
        producer.step_with(|_, _, _| {
            calls += 1;
            SYSCALL_ERR_BUSY
        }),
        Err(PublishError::Transport(SYSCALL_ERR_BUSY))
    );
    assert_eq!(calls, 1);
    assert_eq!(producer.handle(), None);
    let begun = producer.step_with(accept).unwrap();
    assert!(matches!(begun, PublishStep::Begun { .. }));
    let mut pixels = Vec::new();
    for (offset, length) in [(0, 61500), (61500, 20500)] {
        let mut attempts = 0;
        assert_eq!(
            producer.step_with(|req, bytes, _| {
                attempts += 1;
                assert_eq!(req.operation, UPLOAD);
                assert_eq!(req.offset, offset);
                assert_eq!(bytes.len(), length);
                SYSCALL_ERR_BUSY
            }),
            Err(PublishError::Transport(SYSCALL_ERR_BUSY))
        );
        assert_eq!(attempts, 1);
        assert_eq!(
            producer.step_with(|req, bytes, receipt| {
                attempts += 1;
                assert_eq!(req.offset, offset);
                pixels.extend_from_slice(bytes);
                accept(req, bytes, receipt)
            }),
            Ok(PublishStep::Uploaded { offset, length })
        );
        assert_eq!(attempts, 2);
        // A caller may run unrelated application/event work here.
    }
    assert_eq!(
        producer.step_with(|req, _, _| {
            assert_eq!(req.operation, SEAL);
            SYSCALL_ERR_BUSY
        }),
        Err(PublishError::Transport(SYSCALL_ERR_BUSY))
    );
    assert!(matches!(
        producer.step_with(accept),
        Ok(PublishStep::Sealed { .. })
    ));
    assert!(matches!(
        producer.step_with(|_, _, _| panic!("duplicate seal")),
        Ok(PublishStep::Sealed { .. })
    ));
    assert_eq!(
        producer.cancel_with(|_, _, _| panic!("cancelled published pixels")),
        Err(PublishError::Transport(SYSCALL_ERR_INVALID_STATE))
    );
    drop(producer);
    assert!(band.iter().all(|v| *v == 0));
    let mut expected = vec![0; pixels.len()];
    for (row, bytes) in expected.chunks_mut(4100).enumerate() {
        osaiku_ui::raster::rows(&scene, row as u32, bytes).unwrap();
    }
    assert_eq!(pixels, expected);
    metadata.validate().unwrap();
}

#[test]
fn cancellation_retries_keep_exact_handle_and_stale_epoch_cleans_up() {
    let doc = osaiku_ui::compiler::compile_unstyled("<main id=\"r\"></main>", "").unwrap();
    let mut layout = Layout::EMPTY;
    let scene = Scene::prepare(&doc, 2, 2, &mut layout).unwrap();
    let mut metadata = Publication::EMPTY;
    let mut band = [0; 8];
    let mut producer =
        SurfacePublisher::new(&scene, target(), 1, &[], &mut metadata, &mut band).unwrap();
    producer.step_with(accept).unwrap();
    producer.step_with(accept).unwrap();
    let handle = producer.handle().unwrap();
    assert_eq!(
        producer.cancel_with(|req, _, _| {
            assert_eq!(req.operation, CANCEL);
            assert_eq!(req.handle, handle);
            SYSCALL_ERR_BUSY
        }),
        Err(PublishError::Cleanup {
            handle,
            code: SYSCALL_ERR_BUSY
        })
    );
    assert_eq!(producer.handle(), Some(handle));
    producer
        .cancel_with(|_, _, _| SYSCALL_ERR_STALE_ENDPOINT)
        .unwrap();
    producer
        .cancel_with(|_, _, _| panic!("duplicate cancel"))
        .unwrap();
    assert_eq!(
        producer.step_with(|_, _, _| panic!("cancelled producer progressed")),
        Err(PublishError::Transport(SYSCALL_ERR_INVALID_STATE))
    );
    drop(producer);
    assert_eq!(band, [0; 8]);
}

#[test]
fn corrupt_receipts_never_trigger_duplicate_upload_or_cancel_foreign_handle() {
    let doc = compile("<main id=\"r\"></main>", "").unwrap();
    let mut layout = Layout::EMPTY;
    let scene = Scene::prepare(&doc, 2, 2, &mut layout).unwrap();
    for bad_begin in [true, false] {
        let mut metadata = Publication::EMPTY;
        let mut band = [0; 8];
        let mut producer =
            SurfacePublisher::new(&scene, target(), 1, &[], &mut metadata, &mut band).unwrap();
        if !bad_begin {
            producer.step_with(accept).unwrap();
        }
        assert_eq!(
            producer.step_with(|req, bytes, receipt| {
                accept(req, bytes, receipt);
                receipt.handle.target.owner ^= 1;
                SYSCALL_OK
            }),
            Err(PublishError::Receipt)
        );
        assert_eq!(
            producer.step_with(|_, _, _| panic!("uncertain step retried")),
            Err(PublishError::Receipt)
        );
        if bad_begin {
            assert_eq!(
                producer.cancel_with(|_, _, _| panic!("foreign handle cancelled")),
                Err(PublishError::Receipt)
            );
        } else {
            producer
                .cancel_with(|req, bytes, receipt| {
                    assert_eq!(req.handle.target, target());
                    assert_eq!(req.handle.serial, 7);
                    accept(req, bytes, receipt)
                })
                .unwrap();
        }
        drop(producer);
        assert_eq!(band, [0; 8]);
    }
}

#[test]
fn cancel_before_begin_is_local_and_oversized_or_short_scratch_is_refused() {
    let doc = compile("<main id=\"r\"></main>", "").unwrap();
    let mut layout = Layout::EMPTY;
    let scene = Scene::prepare(&doc, 2, 2, &mut layout).unwrap();
    let mut metadata = Publication::EMPTY;
    for length in [0, 7, MAX_TRANSFER + 1] {
        let mut band = vec![0; length];
        assert!(matches!(
            SurfacePublisher::new(&scene, target(), 1, &[], &mut metadata, &mut band),
            Err(PublishError::Content(Refusal::Bounds))
        ));
    }
    let mut band = [0xff; 8];
    let mut producer =
        SurfacePublisher::new(&scene, target(), 1, &[], &mut metadata, &mut band).unwrap();
    producer
        .cancel_with(|_, _, _| panic!("prepared publisher called transport"))
        .unwrap();
    assert_eq!(producer.handle(), None);
    drop(producer);
    assert_eq!(band, [0; 8]);
}
