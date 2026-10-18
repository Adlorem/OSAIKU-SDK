use super::*;
use crate::{
    abi::number,
    report::artifact::{self, storage},
};
use core::{
    cell::UnsafeCell,
    mem::ManuallyDrop,
    sync::atomic::{AtomicBool, Ordering},
};
#[path = "lease.rs"]
mod lease;
union Buffer {
    scan: ManuallyDrop<Scan>,
    bytes: [u8; artifact::MAX_BYTES],
}
struct Work {
    buffer: Buffer,
    scope: Scope,
    slot: u64,
    save: bool,
    length: usize,
    status: u64,
}
struct Shared(UnsafeCell<Work>);
unsafe impl Sync for Shared {}
#[repr(C, align(4096))]
struct Stack(UnsafeCell<[u8; 32768]>);
unsafe impl Sync for Stack {}
#[repr(C, align(64))]
struct Tls(UnsafeCell<[u8; 64]>);
unsafe impl Sync for Tls {}
static RESULT: Shared = Shared(UnsafeCell::new(Work {
    buffer: Buffer {
        bytes: [0; artifact::MAX_BYTES],
    },
    scope: Scope {
        window: 0,
        incarnation: 0,
    },
    slot: 0,
    save: false,
    length: 0,
    status: 0,
}));
static STACK: Stack = Stack(UnsafeCell::new([0; 32768]));
static TLS: Tls = Tls(UnsafeCell::new([0; 64]));
static LEASE: lease::Lease = lease::Lease::new();
static CANCEL: AtomicBool = AtomicBool::new(false);
#[repr(C)]
struct Create {
    version: u64,
    entry: u64,
    argument: u64,
    stack_base: u64,
    stack_bytes: u64,
    tls: u64,
}
/// One worker/stack and a union of query and artifact scratch. Join precedes
/// access, callback invocation, scrubbing and reuse. No session is borrowed.
pub struct Query {
    thread: u64,
    artifact: bool,
}
impl Query {
    pub const EMPTY: Self = Self {
        thread: 0,
        artifact: false,
    };
    pub fn active(&self) -> bool {
        self.thread != 0
    }
    fn acquire(&self) -> bool {
        if self.active() {
            return false;
        }
        LEASE.acquire(join, || unsafe { (*RESULT.0.get()).buffer.bytes.fill(0) })
    }
    fn launch(&mut self, artifact: bool) -> bool {
        CANCEL.store(false, Ordering::Release);
        let r = Create {
            version: 1,
            entry: worker as *const () as u64,
            argument: artifact as u64,
            stack_base: STACK.0.get() as u64,
            stack_bytes: 32768,
            tls: TLS.0.get() as u64,
        };
        let id = unsafe { trap(number::THREAD_CREATE, &r as *const _ as u64, 0, 0, 0, 0) } as i64;
        if id <= 0 {
            self.release();
            return false;
        }
        self.thread = id as u64;
        self.artifact = artifact;
        true
    }
    pub fn start(&mut self, scope: Scope) -> bool {
        if !self.acquire() {
            return false;
        }
        // Initialize each field through raw addresses before creating a Scan
        // reference; the union may previously contain arbitrary file bytes.
        unsafe {
            let p = core::ptr::addr_of_mut!((*RESULT.0.get()).buffer.scan).cast::<Scan>();
            macro_rules! set {
                ($f:ident,$v:expr) => {
                    core::ptr::addr_of_mut!((*p).$f).write($v)
                };
            }
            set!(state, State::Idle);
            set!(scope, scope);
            set!(revision, 0);
            set!(mount, 0);
            for i in 0..CAPACITY {
                core::ptr::addr_of_mut!((*p).entries)
                    .cast::<Entry>()
                    .add(i)
                    .write(Entry::EMPTY);
            }
            set!(len, 0);
            set!(omitted, 0);
            set!(directories, [0; DIRECTORIES]);
            set!(dir_count, 0);
            set!(dir_index, 0);
            set!(cursor, 0);
            set!(sequence, 0);
            set!(pending, None);
            set!(retries, 0);
            set!(steps, 0);
            set!(partial, false);
            (&mut *p).start(scope);
        }
        self.launch(false)
    }
    /// Build is called only after acquiring exclusive scratch. None opens;
    /// Some(length) saves the complete sealed artifact prepared by the caller.
    pub fn start_artifact(
        &mut self,
        scope: Scope,
        slot: u64,
        build: impl FnOnce(&mut [u8]) -> Result<Option<usize>, u64>,
    ) -> Result<(), u64> {
        if !self.acquire() {
            return Err(status::BUSY);
        }
        let work = unsafe { &mut *RESULT.0.get() };
        let bytes = unsafe { &mut work.buffer.bytes };
        bytes.fill(0);
        let n = match build(bytes) {
            Ok(n) => n,
            Err(e) => {
                self.release();
                return Err(e);
            }
        };
        work.scope = scope;
        work.slot = slot;
        work.save = n.is_some();
        work.length = n.unwrap_or(0);
        work.status = 0;
        if self.launch(true) {
            Ok(())
        } else {
            Err(status::BUSY)
        }
    }
    pub fn cancel(&self) {
        if self.active() {
            CANCEL.store(true, Ordering::Release);
        }
    }
    fn joined(&mut self) -> bool {
        if !self.active() || !join(self.thread) {
            return false;
        }
        self.thread = 0;
        true
    }
    fn release(&self) {
        unsafe { (*RESULT.0.get()).buffer.bytes.fill(0) };
        LEASE.release();
    }
    pub fn poll(&mut self, output: &mut Scan) -> bool {
        if self.artifact || !self.joined() {
            return false;
        }
        unsafe { (&*RESULT.0.get()).buffer.scan.copy_result(output) };
        self.release();
        true
    }
    pub fn poll_artifact<T>(&mut self, consume: impl FnOnce(Result<&[u8], u64>) -> T) -> Option<T> {
        if !self.artifact || !self.joined() {
            return None;
        }
        let work = unsafe { &*RESULT.0.get() };
        let result = if work.status == 0 {
            Ok(unsafe { &work.buffer.bytes[..work.length] })
        } else {
            Err(work.status)
        };
        let out = consume(result);
        self.release();
        Some(out)
    }
}
impl Drop for Query {
    fn drop(&mut self) {
        self.cancel();
        if self.active() {
            // Nonblocking: admission joins before reusing this worker's memory.
            LEASE.abandon(self.thread);
        }
    }
}
fn join(thread: u64) -> bool {
    let mut code = 0u64;
    let result = unsafe {
        trap(
            number::THREAD_JOIN,
            thread,
            &mut code as *mut _ as u64,
            0,
            0,
            0,
        )
    } as isize;
    if result == crate::abi::SYSCALL_ERR_BUSY {
        return false;
    }
    if result != crate::abi::SYSCALL_OK || code != 0 {
        crate::process_exit(61);
    }
    core::sync::atomic::fence(Ordering::Acquire);
    true
}
fn exchange(r: &FileRequest) -> FileReply {
    let mut reply = FileReply::EMPTY;
    if unsafe {
        trap(
            number::FILE_OPERATION,
            r as *const _ as u64,
            &mut reply as *mut _ as u64,
            0,
            0,
            0,
        )
    } != 0
    {
        reply = FileReply {
            sequence: r.sequence,
            ..FileReply::EMPTY
        };
    }
    reply
}
extern "C" fn worker(artifact: u64) -> ! {
    let work = unsafe { &mut *RESULT.0.get() };
    if artifact != 0 {
        match storage::run(
            work.scope,
            work.slot,
            work.save.then_some(work.length),
            unsafe { &mut work.buffer.bytes },
            || CANCEL.load(Ordering::Acquire),
            exchange,
        ) {
            Ok(n) => {
                work.length = n;
                work.status = 0;
            }
            Err(e) => {
                work.length = 0;
                work.status = e;
            }
        }
    } else {
        let scan = unsafe { &mut work.buffer.scan };
        while scan.state == State::Running {
            if CANCEL.load(Ordering::Acquire) {
                scan.cancel();
                break;
            }
            scan.step(exchange);
            if scan.state == State::Running
                && unsafe { trap(number::THREAD_SLEEP, 5000, 0, 0, 0, 0) } != 0
            {
                scan.clear(State::Unavailable);
                break;
            }
        }
        if CANCEL.load(Ordering::Acquire) {
            scan.cancel();
        }
    }
    core::sync::atomic::fence(Ordering::Release);
    unsafe {
        trap(number::THREAD_EXIT, 0, 0, 0, 0, 0);
    }
    crate::process_exit(62)
}
unsafe fn trap(n: u64, a: u64, b: u64, c: u64, d: u64, e: u64) -> u64 {
    unsafe { crate::raw::syscall(n, [a, b, c, d, e]) }
}
const _: () = assert!(core::mem::size_of::<Scan>() <= artifact::MAX_BYTES);
