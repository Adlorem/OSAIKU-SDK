use osaiku_runtime::{abi::*, surface::*};

fn handle() -> Handle {
    Handle {
        target: Target {
            owner: (1 << 32) | 8,
            window: 1,
            incarnation: 2,
            epoch: 3,
        },
        serial: 4,
        revision: 5,
    }
}
fn binding() -> RenderRequest {
    RenderRequest {
        request_id: 6,
        request_hash: 7,
        arm_generation: 8,
        revision: 5,
    }
}
fn snapshot() -> FrameSnapshot {
    FrameSnapshot {
        handle: handle(),
        frame: FrameOffer {
            request: binding(),
            geometry: GeometryOffer {
                version: VERSION,
                theme: 0,
                target: handle().target,
                resize_generation: 1,
                frame_layout_hash: 2,
                window_width: 2,
                window_height: 3,
                content: Rect {
                    x: 0,
                    y: 1,
                    width: 2,
                    height: 2,
                },
            },
        },
    }
}

#[test]
fn receiver_exposes_only_complete_copy_and_retries_without_recopying() {
    assert!(core::mem::size_of::<SurfaceReceiver<'_>>() < 512);
    let mut descriptor = snapshot();
    descriptor.frame.geometry.content.width = 8;
    descriptor.frame.geometry.window_width = 8;
    descriptor.frame.geometry.content.height = 3000;
    descriptor.frame.geometry.window_height = 3001;
    let mut output = metadata();
    let mut pixels = vec![0xaa; 96000];
    let mut receiver = SurfaceReceiver::new(descriptor, &mut output, &mut pixels).unwrap();
    assert!(receiver.received().is_none());
    let mut calls = Vec::new();
    assert_eq!(
        receiver
            .step_with(|req, bytes, receipt| {
                calls.push(req.operation);
                respond(req, bytes, receipt);
                if req.operation == METADATA {
                    let mut record = metadata();
                    record.extent.width = 8;
                    record.extent.stride = 32;
                    record.damage[0].width = 8;
                    record.extent.height = 3000;
                    record.damage[0].height = 3000;
                    unsafe {
                        core::ptr::write_unaligned(
                            bytes.as_mut_ptr().cast::<Publication>(),
                            record,
                        );
                    }
                }
                SYSCALL_OK
            })
            .unwrap(),
        ReceiveStep::Prepared
    );
    assert_eq!(calls, [ACQUIRE, FRAME_REQUEST, METADATA]);
    assert!(receiver.received().is_none());
    assert_eq!(
        receiver.step_with(|req, bytes, _| {
            assert_eq!(
                (req.operation, req.offset, bytes.len()),
                (READ, 0, MAX_TRANSFER)
            );
            bytes.fill(0xee);
            SYSCALL_ERR_BUSY
        }),
        Err(ReadError::Transport(SYSCALL_ERR_BUSY))
    );
    for (offset, length, value) in [
        (0, MAX_TRANSFER, 17),
        (MAX_TRANSFER, 96000 - MAX_TRANSFER, 29),
    ] {
        let mut count = 0;
        assert_eq!(
            receiver
                .step_with(|req, bytes, receipt| {
                    count += 1;
                    assert_eq!(
                        (req.operation, req.offset, bytes.len()),
                        (READ, offset as u64, length)
                    );
                    respond(req, bytes, receipt);
                    bytes.fill(value);
                    SYSCALL_OK
                })
                .unwrap(),
            ReceiveStep::Copied(ReadChunk {
                offset: offset as u64,
                length
            })
        );
        assert_eq!(count, 1);
        assert!(receiver.received().is_none());
    }
    assert_eq!(
        receiver.step_with(|req, _, _| {
            assert_eq!(req.operation, RELEASE);
            SYSCALL_ERR_BUSY
        }),
        Err(ReadError::Transport(SYSCALL_ERR_BUSY))
    );
    assert!(receiver.received().is_none());
    assert_eq!(
        receiver
            .step_with(|req, bytes, receipt| {
                assert_eq!(req.operation, RELEASE);
                respond(req, bytes, receipt)
            })
            .unwrap(),
        ReceiveStep::Complete
    );
    let frame = receiver.received().unwrap();
    assert_eq!(frame.snapshot, descriptor);
    assert_eq!(frame.metadata.handle, descriptor.handle);
    assert_eq!(frame.metadata.extent.height, 3000);
    assert!(frame.pixels[..MAX_TRANSFER].iter().all(|b| *b == 17));
    assert!(frame.pixels[MAX_TRANSFER..].iter().all(|b| *b == 29));
    assert_eq!(
        receiver
            .step_with(|_, _, _| panic!("complete receiver repeated work"))
            .unwrap(),
        ReceiveStep::Complete
    );
    receiver
        .cancel_with(|_, _, _| panic!("lease already released"))
        .unwrap();
    assert!(receiver.received().is_none());
}

#[test]
fn receiver_requires_exact_storage_and_preserves_cancellation_after_failure() {
    let mut output = metadata();
    for length in [0, 12, 20] {
        assert!(matches!(
            SurfaceReceiver::new(snapshot(), &mut output, &mut vec![0; length]),
            Err(ReadError::Content(Refusal::Bounds))
        ));
    }
    let mut pixels = [0; 16];
    let mut receiver = SurfaceReceiver::new(snapshot(), &mut output, &mut pixels).unwrap();
    assert_eq!(
        receiver.step_with(|req, bytes, receipt| {
            if req.operation == METADATA {
                return SYSCALL_ERR_BUSY;
            }
            respond(req, bytes, receipt)
        }),
        Err(ReadError::Transport(SYSCALL_ERR_BUSY))
    );
    assert!(receiver.received().is_none());
    assert_eq!(
        receiver.cancel_with(|req, _, _| {
            assert_eq!(req.operation, RELEASE);
            SYSCALL_ERR_BUSY
        }),
        Err(ReadError::Transport(SYSCALL_ERR_BUSY))
    );
    receiver
        .cancel_with(|req, bytes, receipt| {
            assert_eq!(req.operation, RELEASE);
            respond(req, bytes, receipt)
        })
        .unwrap();
    assert!(receiver.received().is_none());
    assert_eq!(
        receiver.step_with(|_, _, _| panic!("cancelled receiver issued work")),
        Err(ReadError::Transport(SYSCALL_ERR_INVALID_STATE))
    );
    receiver
        .cancel_with(|_, _, _| panic!("cancelled receiver repeated cleanup"))
        .unwrap();
}

#[test]
fn snapshot_discovery_checks_descriptor_and_receipt_as_one_reply() {
    for corrupt in 0..7 {
        let result = frame_snapshot_with(handle().target, |req, bytes, receipt| {
            assert_eq!(req.operation, FRAME_SNAPSHOT);
            assert_eq!((req.handle.serial, req.handle.revision), (0, 0));
            let mut answer = snapshot();
            receipt.handle = answer.handle;
            receipt.epoch = answer.handle.target.epoch;
            match corrupt {
                1 => answer.handle.target.window += 1,
                2 => answer.frame.request.revision += 1,
                3 => answer.frame.geometry.content.width += 1,
                4 => receipt.handle.serial += 1,
                5 => receipt.epoch += 1,
                6 => {
                    answer.handle.target.window += 1;
                    answer.frame.geometry.target = answer.handle.target;
                    receipt.handle = answer.handle;
                }
                _ => {}
            }
            unsafe {
                core::ptr::write_unaligned(bytes.as_mut_ptr().cast::<FrameSnapshot>(), answer);
            }
            SYSCALL_OK
        });
        if corrupt == 0 {
            assert_eq!(result.unwrap(), snapshot());
        } else {
            assert!(result.is_err());
        }
    }
}

#[test]
fn snapshot_reader_refuses_supersession_and_mismatched_extent() {
    let mut reader = SurfaceReader::from_snapshot(snapshot()).unwrap();
    let mut observed = metadata();
    assert_eq!(
        reader.start_with(&mut observed, |req, _, _| {
            assert_eq!(req.operation, ACQUIRE);
            SYSCALL_ERR_STALE_ENDPOINT
        }),
        Err(ReadError::Transport(SYSCALL_ERR_STALE_ENDPOINT))
    );
    reader
        .cancel_with(|_, _, _| panic!("no lease acquired"))
        .unwrap();
    let mut changed = snapshot();
    changed.frame.geometry.window_width = 3;
    changed.frame.geometry.content.width = 3;
    let mut reader = SurfaceReader::from_snapshot(changed).unwrap();
    assert_eq!(
        reader.start_with(&mut observed, respond),
        Err(ReadError::Content(Refusal::Bounds))
    );
    assert_eq!(observed.version, 0);
    reader.cancel_with(respond).unwrap();
    let mut reader = SurfaceReader::from_snapshot(snapshot()).unwrap();
    reader.start_with(&mut observed, respond).unwrap();
    assert_eq!(observed.handle, snapshot().handle);
    reader.cancel_with(respond).unwrap();
}
fn metadata() -> Publication {
    let mut record = Publication::EMPTY;
    record.version = VERSION;
    record.handle = handle();
    record.extent = Extent {
        width: 2,
        height: 2,
        stride: 8,
        format: BGRA8_PREMULTIPLIED,
    };
    record.damage_count = 1;
    record.damage[0] = Rect {
        x: 0,
        y: 0,
        width: 2,
        height: 2,
    };
    record
}
fn respond(req: &Request, bytes: &mut [u8], receipt: &mut Receipt) -> isize {
    receipt.handle = req.handle;
    receipt.epoch = req.handle.target.epoch;
    match req.operation {
        FRAME_REQUEST => unsafe {
            core::ptr::write_unaligned(bytes.as_mut_ptr().cast::<RenderRequest>(), binding());
        },
        METADATA => unsafe {
            core::ptr::write_unaligned(bytes.as_mut_ptr().cast::<Publication>(), metadata());
        },
        READ => bytes.fill(255),
        ACQUIRE | RELEASE => assert!(bytes.is_empty()),
        _ => panic!("unexpected operation"),
    }
    SYSCALL_OK
}

#[test]
fn bound_reader_checks_every_request_field_before_exposing_metadata_or_pixels() {
    assert!(core::mem::size_of::<SurfaceReader>() < 256);
    for field in 0..4 {
        let mut reader = SurfaceReader::for_frame(handle(), binding()).unwrap();
        let mut observed = metadata();
        let mut calls = Vec::new();
        let result = reader.start_with(&mut observed, |req, bytes, receipt| {
            calls.push(req.operation);
            let result = respond(req, bytes, receipt);
            if req.operation == FRAME_REQUEST {
                let mut wrong = binding();
                match field {
                    0 => wrong.request_id += 1,
                    1 => wrong.request_hash += 1,
                    2 => wrong.arm_generation += 1,
                    _ => wrong.revision += 1,
                }
                unsafe {
                    core::ptr::write_unaligned(bytes.as_mut_ptr().cast::<RenderRequest>(), wrong);
                }
            }
            result
        });
        assert_eq!(result, Err(ReadError::Content(Refusal::Stale)));
        assert_eq!(calls, [ACQUIRE, FRAME_REQUEST]);
        assert!(
            observed
                == Publication {
                    version: 0,
                    ..Publication::EMPTY
                }
        );
        assert_eq!(
            reader.next_with(&mut [0; 8], |_, _, _| panic!("read before validation")),
            Err(ReadError::Transport(SYSCALL_ERR_INVALID_STATE))
        );
        reader
            .cancel_with(|req, bytes, receipt| {
                assert_eq!(req.operation, RELEASE);
                respond(req, bytes, receipt)
            })
            .unwrap();
    }
    let mut bad = binding();
    bad.revision += 1;
    assert!(matches!(
        SurfaceReader::for_frame(handle(), bad),
        Err(ReadError::Content(Refusal::Identity))
    ));
    bad = binding();
    bad.arm_generation = 0;
    assert!(SurfaceReader::for_frame(handle(), bad).is_err());
}

#[test]
fn bound_reader_retries_under_one_lease_and_clears_refused_metadata() {
    for failing_op in [ACQUIRE, FRAME_REQUEST, METADATA] {
        let mut reader = SurfaceReader::for_frame(handle(), binding()).unwrap();
        let mut observed = metadata();
        let mut acquired = 0;
        assert_eq!(
            reader.start_with(&mut observed, |req, bytes, receipt| {
                if req.operation == failing_op {
                    bytes.fill(0xaa); // Even a partially written failed response is inert.
                    return SYSCALL_ERR_BUSY;
                }
                acquired += usize::from(req.operation == ACQUIRE);
                respond(req, bytes, receipt)
            }),
            Err(ReadError::Transport(SYSCALL_ERR_BUSY))
        );
        assert!(
            observed
                == Publication {
                    version: 0,
                    ..Publication::EMPTY
                }
        );
        reader
            .start_with(&mut observed, |req, bytes, receipt| {
                acquired += usize::from(req.operation == ACQUIRE);
                respond(req, bytes, receipt)
            })
            .unwrap();
        assert_eq!(acquired, 1);
        assert_eq!(observed, metadata());
        let mut scratch = [0; 8];
        for offset in [0, 8] {
            assert_eq!(
                reader.next_with(&mut scratch, respond).unwrap(),
                Some(ReadChunk { offset, length: 8 })
            );
            assert_eq!(scratch, [255; 8]);
        }
        reader.finish_with(respond).unwrap();
    }
}

#[test]
fn bound_reader_refuses_corrupt_receipts_and_metadata_without_losing_cleanup_handle() {
    for failing_op in [ACQUIRE, FRAME_REQUEST, METADATA] {
        for bad_payload in [false, true] {
            let mut reader = SurfaceReader::for_frame(handle(), binding()).unwrap();
            let mut observed = metadata();
            let result = reader.start_with(&mut observed, |req, bytes, receipt| {
                respond(req, bytes, receipt);
                if req.operation == failing_op {
                    if bad_payload && !bytes.is_empty() {
                        bytes.fill(0);
                    } else {
                        receipt.epoch += 1;
                    }
                }
                SYSCALL_OK
            });
            assert!(result.is_err());
            assert!(
                observed
                    == Publication {
                        version: 0,
                        ..Publication::EMPTY
                    }
            );
            assert_eq!(reader.handle(), handle());
            reader
                .cancel_with(|req, bytes, receipt| {
                    assert_eq!(req.operation, RELEASE);
                    assert_eq!(req.handle, handle());
                    respond(req, bytes, receipt)
                })
                .unwrap();
        }
    }
}
