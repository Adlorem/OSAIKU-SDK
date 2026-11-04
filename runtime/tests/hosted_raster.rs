use osaiku_runtime::{
    abi::*,
    hosted::{HostedRaster, RasterError},
    surface::*,
};
use std::cell::RefCell;

fn offer() -> FrameOffer {
    FrameOffer {
        geometry: GeometryOffer {
            version: VERSION,
            theme: 0,
            target: Target {
                owner: (1 << 32) | 8,
                window: 1,
                incarnation: 1,
                epoch: 1,
            },
            resize_generation: 1,
            window_width: 64,
            window_height: 130,
            content: Rect {
                x: 0,
                y: 1,
                width: 64,
                height: 129,
            },
            frame_layout_hash: 1,
        },
        request: RenderRequest {
            request_id: 1,
            request_hash: 2,
            arm_generation: 3,
            revision: 4,
        },
    }
}
fn wire() -> Vec<u8> {
    let doc = osaiku_ui::compiler::compile(
        "<main id=\"root\"><button id=\"go\" action=\"go\">Zażółć</button></main>",
        "",
    )
    .unwrap();
    let mut bytes = vec![0; osaiku_ui::MAX_WIRE];
    let len = doc.encode(&mut bytes).unwrap();
    bytes.truncate(len);
    bytes
}
fn accept(req: &Request, _: &mut [u8], receipt: &mut Receipt) -> isize {
    receipt.handle = req.handle;
    if req.operation == BEGIN_FRAME {
        receipt.handle.serial = 7;
    }
    receipt.epoch = req.handle.target.epoch;
    SYSCALL_OK
}

#[test]
fn frozen_recipe_batches_uploads_and_guard_refuses_reentry() {
    static RASTER: HostedRaster = HostedRaster::new();
    let bytes = wire();
    let trace = RefCell::new(Vec::new());
    let handle = RASTER
        .publish_with(
            offer(),
            &bytes,
            |req, data, receipt| {
                trace.borrow_mut().push(req.operation);
                if req.operation == UPLOAD {
                    assert!(data.len() <= MAX_TRANSFER);
                }
                if req.operation == SEAL {
                    let metadata = unsafe { &*data.as_ptr().cast::<Publication>() };
                    assert!(metadata.validate().is_ok());
                    assert!(metadata.element_count > 1);
                    assert_eq!(metadata.handle.revision, 4);
                }
                accept(req, data, receipt)
            },
            || {
                trace.borrow_mut().push(99);
                assert_eq!(
                    RASTER.publish_with(
                        offer(),
                        &bytes,
                        |_, _, _| panic!("reentry"),
                        || panic!("reentry")
                    ),
                    Err(RasterError::Publish(PublishError::Transport(
                        SYSCALL_ERR_BUSY
                    )))
                );
                Ok(())
            },
        )
        .unwrap();
    assert_eq!(handle.serial, 7);
    // Streaming dirty detection scans at most 32 rows per band; 129 rows
    // require five uploads even when the byte transfer cap would allow one.
    assert_eq!(
        *trace.borrow(),
        [
            BEGIN_FRAME,
            99,
            UPLOAD,
            UPLOAD,
            UPLOAD,
            UPLOAD,
            UPLOAD,
            SEAL
        ]
    );
}

#[test]
fn busy_budget_is_shared_across_steps_and_cancels_exact_candidate() {
    static RASTER: HostedRaster = HostedRaster::new();
    let mut calls = 0;
    let mut cancelled = false;
    let mut checkpoints = 0;
    let result = RASTER.publish_with(
        offer(),
        &wire(),
        |req, data, receipt| {
            calls += 1;
            if req.operation == CANCEL {
                assert_eq!(req.handle.serial, 7);
                cancelled = true;
            } else if calls <= 4 || req.operation == UPLOAD {
                return SYSCALL_ERR_BUSY;
            }
            accept(req, data, receipt)
        },
        || {
            checkpoints += 1;
            Ok(())
        },
    );
    assert_eq!(
        result,
        Err(RasterError::Publish(PublishError::Transport(
            SYSCALL_ERR_BUSY
        )))
    );
    assert_eq!((calls, checkpoints, cancelled), (11, 9, true));
}

#[test]
fn failed_checkpoint_cancels_and_releases_scratch_guard() {
    static RASTER: HostedRaster = HostedRaster::new();
    let mut operations = Vec::new();
    assert_eq!(
        RASTER.publish_with(
            offer(),
            &wire(),
            |req, data, receipt| {
                operations.push(req.operation);
                accept(req, data, receipt)
            },
            || Err(-123)
        ),
        Err(RasterError::Checkpoint(-123))
    );
    assert_eq!(operations, [BEGIN_FRAME, CANCEL]);
    assert!(RASTER
        .publish_with(offer(), &wire(), accept, || Ok(()))
        .is_ok());
}

#[test]
fn malformed_begin_is_not_retried_and_failed_cleanup_retains_handle() {
    static RASTER: HostedRaster = HostedRaster::new();
    let mut calls = 0;
    assert_eq!(
        RASTER.publish_with(
            offer(),
            &wire(),
            |_, _, _| {
                calls += 1;
                SYSCALL_OK
            },
            || panic!("corrupt begin")
        ),
        Err(RasterError::Publish(PublishError::Receipt))
    );
    assert_eq!(calls, 1);
    let error = RASTER
        .publish_with(
            offer(),
            &wire(),
            |req, data, receipt| {
                if req.operation == CANCEL {
                    return SYSCALL_ERR_BUSY;
                }
                if req.operation == UPLOAD {
                    return SYSCALL_ERR_STALE_ENDPOINT;
                }
                accept(req, data, receipt)
            },
            || Ok(()),
        )
        .unwrap_err();
    assert!(matches!(
        error,
        RasterError::Publish(PublishError::Cleanup {
            handle: Handle { serial: 7, .. },
            code: SYSCALL_ERR_BUSY
        })
    ));
}

#[test]
fn invalid_recipe_or_offer_never_allocates_and_valid_recipe_can_follow() {
    static RASTER: HostedRaster = HostedRaster::new();
    assert!(matches!(
        RASTER.publish_with(
            offer(),
            &[0],
            |_, _, _| panic!("invalid recipe"),
            || panic!("invalid recipe")
        ),
        Err(RasterError::View(_))
    ));
    let mut invalid = offer();
    invalid.request.revision = 0;
    assert_eq!(
        RASTER.publish_with(
            invalid,
            &wire(),
            |_, _, _| panic!("invalid offer"),
            || panic!("invalid offer")
        ),
        Err(RasterError::Publish(PublishError::Content(
            Refusal::Identity
        )))
    );
    assert!(RASTER
        .publish_with(offer(), &wire(), accept, || Ok(()))
        .is_ok());
}

#[test]
fn full_hd_upload_has_bounded_batch_yields_and_row_aligned_transfers() {
    static RASTER: HostedRaster = HostedRaster::new();
    let mut frame = offer();
    frame.geometry.window_width = 1920;
    frame.geometry.window_height = 1081;
    frame.geometry.content.width = 1920;
    frame.geometry.content.height = 1080;
    let mut uploads = 0;
    let mut uploaded = 0;
    let mut yields = 0;
    RASTER
        .publish_with(
            frame,
            &wire(),
            |req, bytes, receipt| {
                if req.operation == UPLOAD {
                    assert_eq!(req.offset, uploaded);
                    assert_eq!(bytes.len() % (1920 * 4), 0);
                    assert!(bytes.len() <= MAX_TRANSFER);
                    uploads += 1;
                    uploaded += bytes.len() as u64;
                }
                accept(req, bytes, receipt)
            },
            || {
                yields += 1;
                Ok(())
            },
        )
        .unwrap();
    assert_eq!(uploaded, 1920 * 1080 * 4);
    assert_eq!(uploads, 135);
    assert_eq!(yields, 28);
}

#[test]
fn host_palette_changes_frozen_recipe_pixels_but_keeps_semantic_source() {
    static RASTER: HostedRaster = HostedRaster::new();
    let bytes = wire();
    let render = |theme| {
        let mut frame = offer();
        frame.geometry.theme = theme;
        let mut pixels = Vec::new();
        let mut source_hash = 0;
        RASTER
            .publish_with(
                frame,
                &bytes,
                |req, data, receipt| {
                    if req.operation == UPLOAD {
                        pixels.extend_from_slice(data);
                    }
                    if req.operation == SEAL {
                        let metadata = unsafe { &*data.as_ptr().cast::<Publication>() };
                        source_hash = metadata.source_hash;
                    }
                    accept(req, data, receipt)
                },
                || Ok(()),
            )
            .unwrap();
        (pixels, source_hash)
    };
    let (dark, source) = render(0);
    let (contrast, other_source) = render(1);
    assert_ne!(dark, contrast);
    let (light, light_source) = render(2);
    assert_ne!(dark, light);
    assert_eq!(source, light_source);
    assert_eq!(source, osaiku_ui::wire_hash(&bytes));
    assert_eq!(source, other_source);
    assert_eq!(dark, render(0).0);
    let mut bad = offer();
    bad.geometry.theme = 3;
    assert!(RASTER
        .publish_with(
            bad,
            &bytes,
            |_, _, _| panic!("invalid palette reached transport"),
            || Ok(())
        )
        .is_err());
}

#[derive(Default)]
struct PixelTransport {
    published: Vec<u8>,
    candidate: Vec<u8>,
    ops: Vec<u32>,
    uploaded: usize,
    copied: usize,
    refuse_base: bool,
    fail_copy: bool,
}
impl PixelTransport {
    fn exchange(&mut self, req: &Request, data: &mut [u8], receipt: &mut Receipt) -> isize {
        self.ops.push(req.operation);
        if matches!(req.operation, BEGIN_DELTA | BEGIN_STREAM) && self.refuse_base {
            self.refuse_base = false;
            return SYSCALL_ERR_STALE_ENDPOINT;
        }
        match req.operation {
            BEGIN_FRAME | BEGIN_DELTA | BEGIN_STREAM => {
                self.candidate = vec![0; req.extent.bytes().unwrap() as usize]
            }
            COPY_BASE => {
                if self.fail_copy {
                    self.fail_copy = false;
                    return SYSCALL_ERR_STALE_ENDPOINT;
                }
                assert!(data.is_empty());
                let range = req.offset as usize..(req.offset + req.server_endpoint) as usize;
                self.candidate[range.clone()].copy_from_slice(&self.published[range]);
                self.copied += req.server_endpoint as usize;
            }
            UPLOAD => {
                self.candidate[req.offset as usize..req.offset as usize + data.len()]
                    .copy_from_slice(data);
                self.uploaded += data.len();
            }
            SEAL => {
                let metadata = unsafe { &*data.as_ptr().cast::<Publication>() };
                metadata.validate().unwrap();
                self.published.clone_from(&self.candidate);
            }
            CANCEL => self.candidate.clear(),
            _ => panic!("unexpected operation"),
        }
        receipt.handle = req.handle;
        if matches!(req.operation, BEGIN_FRAME | BEGIN_DELTA | BEGIN_STREAM) {
            receipt.handle.serial = req.handle.revision + 10;
        }
        receipt.epoch = req.handle.target.epoch;
        SYSCALL_OK
    }
}
fn band_wire(label: &str) -> Vec<u8> {
    let doc = osaiku_ui::compiler::compile(
        &format!("<main id=\"r\"><span id=\"s\">{label}</span></main>"),
        "",
    )
    .unwrap();
    let mut bytes = vec![0; osaiku_ui::MAX_WIRE];
    let len = doc.encode(&mut bytes).unwrap();
    bytes.truncate(len);
    bytes
}
fn band_offer(revision: u64) -> FrameOffer {
    let mut f = offer();
    f.geometry.window_width = 128;
    f.geometry.window_height = 97;
    f.geometry.content.width = 128;
    f.geometry.content.height = 96;
    f.request.revision = revision;
    f
}

#[test]
fn automatic_dirty_bands_match_full_pixels_and_semantic_only_revision_uploads_nothing() {
    let raster = Box::new(HostedRaster::new());
    let mut transport = PixelTransport::default();
    raster
        .publish_with(
            band_offer(1),
            &band_wire("a"),
            |r, b, o| transport.exchange(r, b, o),
            || Ok(()),
        )
        .unwrap();
    transport.uploaded = 0;
    raster
        .publish_with(
            band_offer(2),
            &band_wire("b"),
            |r, b, o| transport.exchange(r, b, o),
            || Ok(()),
        )
        .unwrap();
    assert_eq!(transport.uploaded, 128 * 4 * 32);
    assert_eq!(transport.copied, 128 * 4 * 64);
    let fresh = Box::new(HostedRaster::new());
    let mut reference = PixelTransport::default();
    fresh
        .publish_with(
            band_offer(2),
            &band_wire("b"),
            |r, b, o| reference.exchange(r, b, o),
            || Ok(()),
        )
        .unwrap();
    assert_eq!(transport.published, reference.published);
    transport.uploaded = 0;
    transport.copied = 0;
    raster
        .publish_with(
            band_offer(3),
            &band_wire("b"),
            |r, b, o| transport.exchange(r, b, o),
            || Ok(()),
        )
        .unwrap();
    assert_eq!((transport.uploaded, transport.copied), (0, 128 * 4 * 96));
}

#[test]
fn stale_base_falls_back_once_and_failed_delta_discards_fingerprints() {
    let raster = Box::new(HostedRaster::new());
    let mut transport = PixelTransport::default();
    let wire = band_wire("a");
    raster
        .publish_with(
            band_offer(1),
            &wire,
            |r, b, o| transport.exchange(r, b, o),
            || Ok(()),
        )
        .unwrap();
    transport.refuse_base = true;
    transport.ops.clear();
    raster
        .publish_with(
            band_offer(2),
            &wire,
            |r, b, o| transport.exchange(r, b, o),
            || Ok(()),
        )
        .unwrap();
    assert_eq!(&transport.ops[..2], &[BEGIN_STREAM, BEGIN_FRAME]);
    transport.fail_copy = true;
    let old = transport.published.clone();
    assert!(raster
        .publish_with(
            band_offer(3),
            &wire,
            |r, b, o| transport.exchange(r, b, o),
            || Ok(())
        )
        .is_err());
    assert_eq!(transport.published, old);
    assert_eq!(transport.ops.last(), Some(&CANCEL));
    transport.ops.clear();
    raster
        .publish_with(
            band_offer(3),
            &wire,
            |r, b, o| transport.exchange(r, b, o),
            || Ok(()),
        )
        .unwrap();
    assert_eq!(transport.ops[0], BEGIN_FRAME);
    for change in 0..3 {
        let mut frame = band_offer(4 + change);
        match change {
            0 => {
                frame.geometry.resize_generation += 1;
            }
            1 => {
                frame.geometry.target.epoch += 1;
            }
            _ => {
                frame.geometry.theme = 1;
            }
        }
        transport.ops.clear();
        raster
            .publish_with(
                frame,
                &wire,
                |r, b, o| transport.exchange(r, b, o),
                || Ok(()),
            )
            .unwrap();
        assert_eq!(transport.ops[0], BEGIN_FRAME);
    }
}
