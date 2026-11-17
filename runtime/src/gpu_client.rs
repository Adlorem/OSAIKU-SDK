//! CPU-only Painter client. Immutable asset packs use the existing shared
//! allocator: four objects of at most 1 MiB, never a larger generic quota.
use crate::gpu_painter::{self, Command};
use osaiku_gpu_protocol::{
    buffer::{kind, Handle},
    channel::BUSY,
    render,
};
use osaiku_gpu_runtime::{
    channel::{Channel, Transport},
    upload::{self, Sealed, Upload},
};
use sha2::{Digest, Sha256};
pub trait Wait {
    fn now_ms(&self) -> Result<u64, isize>;
    fn wait(&mut self);
}
const PACKS: usize = 4;
const RECORDS: usize = 512;
#[path = "gpu_client_composition.rs"]
mod composition;
struct Pack {
    storage: Sealed,
    handle: Handle,
    used: u64,
}
#[derive(Clone, Copy)]
struct Record {
    hash: [u8; 32],
    bytes: usize,
    pack: usize,
    offset: u64,
}
pub struct Client<C: Transport, U: upload::Transport, W: Wait> {
    transport: Option<C>,
    channel: Option<Channel<C>>,
    upload: Upload<U>,
    wait: W,
    packs: [Option<Pack>; PACKS],
    records: [Option<Record>; RECORDS],
    serial: u64,
    stage: [u8; upload::MAX_BYTES],
    failed: bool,
    wire: [u8; render::MAX_BYTES],
    group: [Command; 32],
    pending: [Option<Record>; 32],
    assets: [(Handle, u64); 32],
    encoded_draws: [render::Draw; render::MAX_DRAWS],
}
impl<C: Transport, U: upload::Transport, W: Wait> Client<C, U, W> {
    pub const fn new(channel: C, upload: U, wait: W) -> Self {
        Self {
            transport: Some(channel),
            channel: None,
            upload: Upload::new(upload),
            wait,
            packs: [const { None }; PACKS],
            records: [None; RECORDS],
            serial: 1,
            stage: [0; upload::MAX_BYTES],
            failed: false,
            wire: [0; render::MAX_BYTES],
            group: [Command::EMPTY; 32],
            pending: [None; 32],
            assets: [(render::NO_ASSET, 0); 32],
            encoded_draws: gpu_painter::EMPTY_DRAWS,
        }
    }
    pub fn healthy(&self) -> bool {
        !self.failed && self.upload.healthy() && self.channel.as_ref().is_none_or(|c| c.healthy())
    }
    fn pause(&mut self, began: u64) -> Result<(), isize> {
        let now = self.wait.now_ms()?;
        if now < began || now - began >= 30_000 {
            return Err(-5);
        }
        self.wait.wait();
        Ok(())
    }
    fn connect(&mut self, width: u32, height: u32) -> Result<(), isize> {
        if self.channel.is_none() {
            let t = self.transport.take().ok_or(-5isize)?;
            self.channel = Some(Channel::open(t, width, height)?);
        }
        let began = self.wait.now_ms()?;
        while !self.channel.as_mut().unwrap().refresh()? {
            self.pause(began)?;
        }
        let i = self.channel.as_ref().unwrap().identity();
        if (i.width, i.height) != (width, height) {
            self.channel.as_mut().unwrap().resize(width, height)?;
        }
        Ok(())
    }
    fn submit(&mut self, length: usize) -> Result<[u64; 4], isize> {
        let began = self.wait.now_ms()?;
        loop {
            match self
                .channel
                .as_mut()
                .unwrap()
                .submit(&mut self.wire[..length])
            {
                Ok(_) => break,
                Err(BUSY) => self.pause(began)?,
                Err(e) => return Err(e),
            }
        }
        loop {
            if let Some(r) = self.channel.as_mut().unwrap().poll_result()? {
                return Ok(r);
            }
            self.pause(began)?;
        }
    }
    fn evict(&mut self, slot: usize) -> Result<(), isize> {
        if let Some(p) = self.packs[slot].as_ref() {
            self.channel.as_mut().unwrap().release(p.handle)?;
        }
        if let Some(p) = self.packs[slot].take() {
            if let Err((e, storage)) = self.upload.release(p.storage) {
                self.packs[slot] = Some(Pack { storage, ..p });
                return Err(e);
            }
        }
        for r in &mut self.records {
            if r.is_some_and(|r| r.pack == slot) {
                *r = None;
            }
        }
        Ok(())
    }
    /// All raw CPU sources in commands must remain readable for the entire
    /// call. They are copied before seal; no source pointer enters the channel.
    pub unsafe fn dispatch(&mut self, c: &mut Command) -> Result<(), isize> {
        if !self.healthy() {
            return Err(-5);
        }
        let result = unsafe { self.dispatch_inner(c) };
        if result.is_err() {
            self.failed = true;
        }
        result
    }
    unsafe fn dispatch_inner(&mut self, c: &mut Command) -> Result<(), isize> {
        if c.op == 1 {
            if c.rect[0] != 0
                || c.rect[1] != 0
                || !(1..=4096).contains(&c.rect[2])
                || !(1..=4096).contains(&c.rect[3])
            {
                return Err(-1);
            }
            self.connect(c.rect[2] as u32, c.rect[3] as u32)?;
        }
        if self.channel.is_none() {
            return if matches!(c.op, 7 | 8 | 10) {
                c.result = [0; 4];
                Ok(())
            } else {
                Err(-5)
            };
        }
        let identity = self.channel.as_ref().unwrap().identity();
        if matches!(c.op, 1 | 2 | 7 | 8 | 10) {
            if !c.data.is_null()
                || c.len != 0
                || c.color != 0
                || c.clip != [0; 4]
                || c.shape != [0; 4]
                || c.params != [0; 8]
                || (c.op != 1 && c.rect != [0; 4])
                || (c.op == 2 && c.result != [identity.client, 0, 0, 0])
                || (c.op != 2 && c.result != [0; 4])
            {
                return Err(-1);
            }
            let size = gpu_painter::encode_control(
                self.channel.as_ref().unwrap().next_scope(),
                identity.width,
                identity.height,
                c.op,
                &mut self.wire,
            )
            .map_err(|_| -1isize)?;
            c.result = self.submit(size)?;
            if c.op == 8 {
                for n in 0..PACKS {
                    self.evict(n)?;
                }
            }
            return Ok(());
        }
        let commands = if c.op == 9 {
            if c.data.is_null()
                || c.len == 0
                || c.len % core::mem::size_of::<Command>() != 0
                || c.len / core::mem::size_of::<Command>() > 32
            {
                return Err(-1);
            }
            // Local SDK ABI only, never a pointer supplied by another process.
            if c.data as usize % core::mem::align_of::<Command>() != 0 {
                return Err(-1);
            }
            unsafe {
                core::slice::from_raw_parts(
                    c.data.cast::<Command>(),
                    c.len / core::mem::size_of::<Command>(),
                )
            }
        } else {
            core::slice::from_ref(c)
        };
        // Complete shape validation precedes any upload or renderer command.
        let h = Handle {
            owner: identity.client,
            device: identity.device,
            generation: identity.connection,
            serial: 1,
            kind: kind::BUFFER,
            reserved: 0,
        };
        gpu_painter::encode_batch_with_scratch(
            self.channel.as_ref().unwrap().next_scope(),
            identity.width,
            identity.height,
            commands,
            &mut self.wire,
            &mut self.encoded_draws,
            |_| Ok((h, 0)),
        )
        .map_err(|_| -1isize)?;
        let mut count = 0;
        for command in commands {
            if command.op == 6 && command.len > upload::MAX_BYTES {
                let row = command.params[1] as usize * 4;
                if row == 0 || row > upload::MAX_BYTES {
                    return Err(-1);
                }
                let rows = upload::MAX_BYTES / row;
                let mut y = 0;
                while y < command.params[2] as usize {
                    let n = rows.min(command.params[2] as usize - y);
                    let mut tile = *command;
                    tile.rect[1] = tile.rect[1].checked_add(y as i32).ok_or(-1isize)?;
                    tile.rect[3] = n as i32;
                    tile.params[2] = n as u32;
                    tile.data = unsafe { command.data.add(y * row) };
                    tile.len = n * row;
                    self.group[count] = tile;
                    count += 1;
                    y += n;
                    if count == 32 {
                        unsafe { self.draws(count) }?;
                        count = 0;
                    }
                }
            } else {
                self.group[count] = *command;
                count += 1;
                if count == 32 {
                    unsafe { self.draws(count) }?;
                    count = 0;
                }
            }
        }
        if count != 0 {
            unsafe { self.draws(count) }?;
        }
        Ok(())
    }
    unsafe fn draws(&mut self, length: usize) -> Result<(), isize> {
        let mut start = 0;
        while start < length {
            let mut count = 0;
            let mut total = 0usize;
            let mut used = [false; PACKS];
            self.pending.fill(None);
            let mut selected = None;
            self.assets.fill((render::NO_ASSET, 0));
            for c in &self.group[start..length] {
                if matches!(c.op, 4 | 6) {
                    let source = unsafe { core::slice::from_raw_parts(c.data, c.len) };
                    let hash: [u8; 32] = Sha256::digest(source).into();
                    if let Some(r) =
                        self.records.iter().flatten().find(|r| {
                            r.hash == hash && r.bytes == c.len && Some(r.pack) != selected
                        })
                    {
                        used[r.pack] = true;
                        self.assets[count] =
                            (self.packs[r.pack].as_ref().unwrap().handle, r.offset);
                    } else if let Some(r) = self
                        .pending
                        .iter()
                        .flatten()
                        .find(|r| r.hash == hash && r.bytes == c.len)
                    {
                        self.assets[count] = (render::NO_ASSET, r.offset);
                    } else {
                        if total + c.len > upload::MAX_BYTES {
                            break;
                        }
                        if selected.is_none() {
                            selected = (0..PACKS)
                                .filter(|n| !used[*n])
                                .min_by_key(|n| self.packs[*n].as_ref().map_or(0, |p| p.used));
                            if selected.is_none() {
                                break;
                            }
                        }
                        self.stage[total..total + c.len].copy_from_slice(source);
                        self.pending[count] = Some(Record {
                            hash,
                            bytes: c.len,
                            pack: selected.unwrap(),
                            offset: total as u64,
                        });
                        self.assets[count] = (render::NO_ASSET, total as u64);
                        total += c.len;
                    }
                }
                count += 1;
            }
            if count == 0 {
                return Err(-5);
            }
            if let Some(slot) = selected {
                self.evict(slot)?;
                let storage = self.upload.seal(&self.stage[..total])?;
                let handle = self.channel.as_mut().unwrap().register(
                    storage.id(),
                    storage.generation(),
                    storage.bytes(),
                )?;
                self.packs[slot] = Some(Pack {
                    storage,
                    handle,
                    used: self.serial,
                });
                for r in self.pending.iter().copied().flatten() {
                    // Metadata overflow only evicts cache lookup entries. The
                    // containing sealed pack and its handle stay retained.
                    let index = self.records.iter().position(Option::is_none).unwrap_or(0);
                    self.records[index] = Some(r);
                }
                for (index, a) in self.assets[..count].iter_mut().enumerate() {
                    if matches!(self.group[start + index].op, 4 | 6) && a.0 == render::NO_ASSET {
                        a.0 = handle;
                    }
                }
                used[slot] = true;
            }
            self.serial = self.serial.checked_add(1).ok_or(-5isize)?;
            for (n, p) in self.packs.iter_mut().enumerate() {
                if used[n] {
                    p.as_mut().unwrap().used = self.serial;
                }
            }
            let channel = self.channel.as_ref().unwrap();
            let identity = channel.identity();
            let mut index = 0;
            let len = gpu_painter::encode_batch_with_scratch(
                channel.next_scope(),
                identity.width,
                identity.height,
                &self.group[start..start + count],
                &mut self.wire,
                &mut self.encoded_draws,
                |_| {
                    while !matches!(self.group[start + index].op, 4 | 6) {
                        index += 1;
                    }
                    let a = self.assets[index];
                    index += 1;
                    Ok(a)
                },
            )
            .map_err(|_| -1isize)?;
            self.submit(len)?;
            start += count;
        }
        Ok(())
    }
}

#[cfg(all(target_os = "none", target_arch = "x86_64"))]
mod native {
    use super::*;
    use core::{
        cell::UnsafeCell,
        sync::atomic::{AtomicBool, Ordering},
    };
    use osaiku_gpu_runtime::native::{self, Native};
    struct Clock;
    impl Wait for Clock {
        fn now_ms(&self) -> Result<u64, isize> {
            native::clock_millis()
        }
        // Painter/compositord run on the main thread. THREAD_SLEEP is worker
        // only; falling back to YIELD keeps the UI class continuously runnable
        // while its renderer/backend compete for System service. A local
        // event wait relinquishes demand without consuming application IPC.
        // Completion is latched, including the POLL -> WAIT race.
        fn wait(&mut self) {
            let _ = crate::desktop::graphics_event_wait(200_000);
        }
    }
    struct Global(UnsafeCell<Client<Native, Native, Clock>>);
    unsafe impl Sync for Global {}
    static CLIENT: Global = Global(UnsafeCell::new(Client::new(Native, Native, Clock)));
    static LOCK: AtomicBool = AtomicBool::new(false);
    unsafe extern "C" fn dispatch(c: *mut Command) -> i32 {
        if c.is_null() {
            return 22;
        }
        if LOCK
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            return 16;
        }
        let result = unsafe { (&mut *CLIENT.0.get()).dispatch(&mut *c) };
        LOCK.store(false, Ordering::Release);
        result.err().map_or(0, |e| match e {
            BUSY => 16,
            -1 => 22,
            -8 => 13,
            _ => 5,
        })
    }
    pub fn install(application: u32) {
        gpu_painter::label(application);
        // The same HostedRaster/Scene/Painter API serves both modes. Ordinary
        // UI never opens a channel, loads Mesa or uploads GPU asset packs.
        // This is only an optimization; the kernel rechecks authority on use.
        if crate::launch::current()
            .is_ok_and(|c| c.permissions & crate::launch::permission::GPU_RENDER != 0)
        {
            gpu_painter::install(dispatch);
            crate::log_write("app-render:v1;mode=gpu-opt-in;fallback=cpu;compositor=independent\n");
        } else {
            crate::log_write("app-render:v1;mode=cpu;reason=no-gpu-grant;compositor=independent\n");
        }
    }
    pub unsafe fn composition(c: &mut crate::gpu_compositor::Command) -> Result<(), u32> {
        if LOCK
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            return Err(16);
        }
        let result = unsafe { (&mut *CLIENT.0.get()).compose(c) };
        LOCK.store(false, Ordering::Release);
        result.map_err(|e| match e {
            BUSY => 16,
            -1 => 22,
            -8 => 13,
            _ => 5,
        })
    }
}
#[cfg(all(target_os = "none", target_arch = "x86_64"))]
pub use native::composition;
#[cfg(all(target_os = "none", target_arch = "x86_64"))]
pub use native::install;
