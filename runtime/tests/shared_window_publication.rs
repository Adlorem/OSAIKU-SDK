//! Execute the native publication algorithm with a receipt/lease provider.
//! Page permissions themselves are covered by the kernel window-memory tests.
use osaiku_runtime::{abi::*, hosted::RasterError};
pub use osaiku_runtime::{abi, gpu_timing};
mod surface {
    pub use osaiku_runtime::surface::*;
    #[path = "../../src/surface/dirty.rs"]
    pub mod dirty;
    pub(crate) use dirty::DirtyTracker;
}
use surface::*;
#[path = "../src/hosted/shared.rs"]
mod shared;

mod window_buffer {
    use std::{cell::RefCell, collections::BTreeSet};
    pub use osaiku_runtime::window_buffer::Handle;
    #[derive(Default)]
    pub struct State {
        pub next: u64,
        pub pinned: BTreeSet<u64>,
        pub released: Vec<u64>,
    }
    thread_local! {pub static STATE: RefCell<State> = RefCell::new(State::default());}
    pub struct Buffer { id: u64, generation: u64, data: Vec<u8>, writing: bool }
    impl Buffer {
        pub fn create(bytes: usize) -> Result<Self, isize> {
            let id = STATE.with(|s| { let mut s = s.borrow_mut(); s.next += 1; s.next });
            Ok(Self {id, generation: 0, data: vec![0; bytes], writing: true})
        }
        pub fn len(&self) -> usize {self.data.len()}
        pub fn write(&mut self) -> Result<(), isize> {
            if STATE.with(|s|s.borrow().pinned.contains(&self.id)) {return Err(-13)}
            self.writing = true; Ok(())
        }
        pub fn bytes_mut(&mut self) -> &mut [u8] {assert!(self.writing); &mut self.data}
        pub fn seal(&mut self) -> Result<Handle, isize> {
            assert!(self.writing); self.writing = false; self.generation += 1;
            Ok(Handle {id: self.id, generation: self.generation})
        }
        pub fn release(self) -> Result<(), Self> {
            STATE.with(|s| s.borrow_mut().released.push(self.id)); Ok(())
        }
    }
}

fn offer(revision: u64, width: u32) -> FrameOffer {
    FrameOffer {
        geometry: GeometryOffer {
            version: VERSION, theme: 0,
            target: Target {owner: (1 << 32) | 8, window: 1, incarnation: 1, epoch: 1},
            resize_generation: width as u64,
            window_width: width, window_height: 130,
            content: Rect {x: 0, y: 1, width, height: 129}, frame_layout_hash: 1,
        },
        request: RenderRequest {request_id: revision, request_hash: 2, arm_generation: 3, revision},
    }
}

fn publish(pool: &mut shared::Pool, tracker: &mut DirtyTracker, revision: u64, width: u32,
    exchange: &mut impl FnMut(&Request, &mut [u8], &mut Receipt) -> isize,
) -> Result<Handle, RasterError> {
    use osaiku_ui::{layout::{Layout, Workspace}, paint::Scene};
    let doc = osaiku_ui::compiler::compile("<main id=\"root\"><button id=\"go\" action=\"go\">Test</button></main>", "").unwrap();
    let mut wire = vec![0; osaiku_ui::MAX_WIRE];
    let len = doc.encode(&mut wire).unwrap();
    let mut layout = Layout::EMPTY;
    let mut workspace = Workspace::EMPTY;
    let scene = Scene::prepare_in(&doc, width, 129, &mut layout, &mut workspace).unwrap();
    let mut metadata = Publication::EMPTY;
    shared::publish(pool, &scene, offer(revision, width), &wire[..len], &mut metadata,
        tracker, exchange, &mut || Ok(()))
}

fn accept(req: &Request, receipt: &mut Receipt) {
    receipt.handle = req.handle;
    if req.operation == BEGIN_SHARED {receipt.handle.serial = req.handle.revision + 100;}
    receipt.epoch = req.handle.target.epoch;
}

#[test]
fn alternating_shared_buffers_reuse_pixels_and_resize_retires_old_sizes() {
    window_buffer::STATE.with(|s| *s.borrow_mut() = Default::default());
    let mut pool = shared::Pool::new();
    let mut tracker = DirtyTracker::new();
    let mut ids = Vec::new();
    let mut displayed = None;
    let mut candidate = 0;
    let mut exchange = |req: &Request, data: &mut [u8], receipt: &mut Receipt| {
        match req.operation {
            BEGIN_SHARED => {
                let shared = unsafe {data.as_ptr().cast::<SharedOffer>().read_unaligned()};
                candidate = shared.buffer.id;
                ids.push((candidate, shared.buffer.generation));
                if req.handle.revision == 1 || req.handle.revision == 4 {assert_eq!(shared.base, Handle::default());}
                else {assert_eq!(shared.base.revision, req.handle.revision - 1);}
                window_buffer::STATE.with(|s| {s.borrow_mut().pinned.insert(candidate);});
            }
            SEAL => {
                let metadata = unsafe {&*data.as_ptr().cast::<Publication>()};
                metadata.validate().unwrap();
                if req.handle.revision == 2 || req.handle.revision == 3 {assert_eq!(metadata.damage_count, 0);}
                else {assert_eq!(metadata.damage_count, 1); assert_eq!(metadata.damage[0].height, 129);}
                window_buffer::STATE.with(|s| {
                    if let Some(old) = displayed.replace(candidate) {s.borrow_mut().pinned.remove(&old);}
                });
            }
            _ => panic!("unexpected pixel-copy operation {}", req.operation),
        }
        accept(req, receipt); SYSCALL_OK
    };
    for revision in 1..=3 {publish(&mut pool, &mut tracker, revision, 64, &mut exchange).unwrap();}
    publish(&mut pool, &mut tracker, 4, 80, &mut exchange).unwrap();
    assert_eq!(ids, [(1,1),(2,1),(1,2),(3,1)]);
    window_buffer::STATE.with(|s| {let s=s.borrow(); assert_eq!(s.next, 3); assert_eq!(s.released, [1,2]);});
}

#[test]
fn failed_seal_cancels_candidate_and_next_publication_has_no_stale_base() {
    window_buffer::STATE.with(|s| *s.borrow_mut() = Default::default());
    let mut pool = shared::Pool::new();
    let mut tracker = DirtyTracker::new();
    let mut cancelled = false;
    let mut exchange = |req: &Request, data: &mut [u8], receipt: &mut Receipt| {
        if req.operation == BEGIN_SHARED {
            let shared = unsafe {data.as_ptr().cast::<SharedOffer>().read_unaligned()};
            assert_eq!(shared.base, Handle::default());
        }
        if req.operation == SEAL && req.handle.revision == 1 {return -5;}
        if req.operation == CANCEL {cancelled = true;}
        if req.operation == SEAL {assert_eq!(unsafe {&*data.as_ptr().cast::<Publication>()}.damage_count, 1);}
        accept(req, receipt); SYSCALL_OK
    };
    assert_eq!(publish(&mut pool, &mut tracker, 1, 64, &mut exchange), Err(RasterError::Publish(PublishError::Transport(-5))));
    assert!(tracker.base.is_none());
    publish(&mut pool, &mut tracker, 2, 64, &mut exchange).unwrap();
    assert!(cancelled);
    assert_eq!(tracker.base.unwrap().handle.revision, 2);
}
