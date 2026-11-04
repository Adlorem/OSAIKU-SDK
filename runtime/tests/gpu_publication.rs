use osaiku_runtime::{
    abi::*,
    gpu_painter::{self, Command},
    graphics::*,
    hosted::HostedRaster,
    surface::*,
};
use std::sync::atomic::{AtomicU32, Ordering::*};
static FAIL: AtomicU32 = AtomicU32::new(0);
static RESETS: AtomicU32 = AtomicU32::new(0);
unsafe extern "C" fn dispatch(c: *mut Command) -> i32 {
    let c = unsafe { &mut *c };
    if c.op == 8 {
        RESETS.fetch_add(1, Relaxed);
        return 0;
    }
    if c.op == 1 && FAIL.load(Relaxed) > 0 {
        FAIL.fetch_sub(1, Relaxed);
        return 5;
    }
    if c.op == 2 {
        c.result = [11, 12, 256, ARGB8888 as u64];
    }
    0
}
fn offer() -> FrameOffer {
    FrameOffer {
        geometry: GeometryOffer {
            version: osaiku_runtime::surface::VERSION,
            theme: 0,
            target: Target {
                owner: (1 << 32) | 20,
                window: 1,
                incarnation: 1,
                epoch: 1,
            },
            resize_generation: 1,
            window_width: 64,
            window_height: 65,
            content: Rect {
                x: 0,
                y: 1,
                width: 64,
                height: 64,
            },
            frame_layout_hash: 1,
        },
        request: RenderRequest {
            request_id: 1,
            request_hash: 2,
            arm_generation: 1,
            revision: 1,
        },
    }
}
#[test]
fn sdk_gpu_revision_atomicity_context_recreation_and_explicit_cpu_recovery() {
    gpu_painter::install(dispatch);
    let d = osaiku_ui::compiler::compile("<main id=\"r\"><p id=\"t\">Test</p></main>", "").unwrap();
    let mut wire = vec![0; osaiku_ui::MAX_WIRE];
    let n = d.encode(&mut wire).unwrap();
    wire.truncate(n);
    for phase in 0..4 {
        if phase == 2 {
            FAIL.store(1, Relaxed);
        }
        if phase == 3 {
            FAIL.store(2, Relaxed);
        }
        let raster = Box::new(HostedRaster::new());
        let mut operations = Vec::new();
        let mut gpu_selected = false;
        let result = raster.publish_with(
            offer(),
            &wire,
            |req, data, receipt| {
                operations.push(req.operation);
                receipt.handle = req.handle;
                receipt.epoch = req.handle.target.epoch;
                if req.operation == NEGOTIATE {
                    let q = unsafe { data.as_mut_ptr().cast::<Negotiation>().read_unaligned() };
                    let selected = q
                        .resolve(&[Device {
                            id: 1,
                            generation: if phase>=2 {2}else{1},
                            integrated: true,
                            capabilities: capability::UI,
                            presentation_device: 1,
                        }])
                        .unwrap();
                    gpu_selected = selected.renderer == 1;
                    unsafe {
                        data.as_mut_ptr()
                            .cast::<Negotiation>()
                            .write_unaligned(selected)
                    };
                    *receipt = Receipt::default();
                } else if req.operation == BEGIN_GPU {
                    assert!(gpu_selected);
                    let g = unsafe { data.as_ptr().cast::<GpuOffer>().read_unaligned() };
                    assert_eq!(g.frame, offer());
                    assert_eq!((g.data, g.fence), (11, 12));
                    receipt.handle.serial = 7;
                } else if matches!(req.operation, BEGIN_FRAME | BEGIN_STREAM) {
                    receipt.handle.serial = 8;
                } else if req.operation == SEAL {
                    let p = unsafe { &*data.as_ptr().cast::<Publication>() };
                    assert_eq!(p.source_hash, osaiku_ui::wire_hash(&wire));
                    assert_eq!(p.handle.revision, 1);
                    assert!(p.validate().is_ok());
                    if phase == 1 {
                        return SYSCALL_ERR_STALE_ENDPOINT;
                    }
                }
                SYSCALL_OK
            },
            || Ok(()),
        );
        if phase == 1 {
            assert!(result.is_err());
            assert_eq!(operations.last(), Some(&CANCEL));
        } else {
            assert!(result.is_ok(), "{phase}: {result:?}");
        }
        if phase < 3 {
            assert!(!operations.contains(&UPLOAD));
            assert!(operations.contains(&BEGIN_GPU));
        } else {
            assert!(operations.contains(&UPLOAD));
            assert!(!operations.contains(&BEGIN_GPU));
        }
    }
    assert_eq!(RESETS.load(Relaxed), 3);
}
