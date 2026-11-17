//! Persistent CPU window storage. Addresses stay stable; WRITE and SEAL
//! transfer access through kernel-checked page permissions and reader leases.
pub const SYSCALL: u64 = 414;
pub const START: u64 = 0xb000_0000;
pub const END: u64 = 0xc000_0000;
pub const MAX_BYTES: u64 = 64 * 1024 * 1024;
pub const TAG: u64 = 1 << 62;
pub mod op {
    pub const CREATE: u64 = 1;
    pub const WRITE: u64 = 2;
    pub const SEAL: u64 = 3;
    pub const RELEASE: u64 = 4;
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Message {
    pub operation: u64,
    pub id: u64,
    pub bytes: u64,
    pub address: u64,
    pub generation: u64,
    pub writing: u64,
    pub reserved: [u64; 2],
}
pub const BYTES: usize = core::mem::size_of::<Message>();
const _: () = assert!(BYTES == 64);
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Handle {
    pub id: u64,
    pub generation: u64,
}
/// No slice survives a transition. A lost transition receipt poisons the owner;
/// process teardown, rather than a guessed retry, retains uncertain mappings.
pub struct Buffer {
    id: u64,
    bytes: usize,
    address: u64,
    generation: u64,
    writing: bool,
    failed: bool,
}
impl Buffer {
    pub fn create(bytes: usize) -> Result<Self, isize> {
        if bytes == 0 || bytes as u64 > MAX_BYTES {
            return Err(-1);
        }
        let r = exchange(Message {
            operation: op::CREATE,
            bytes: bytes as u64,
            ..Message::default()
        })?;
        if r.id & TAG == 0
            || r.bytes != bytes as u64
            || r.address < START
            || r.address % 4096 != 0
            || r.address.checked_add(r.bytes).is_none_or(|v| v > END)
            || r.generation != 0
            || r.writing != 1
        {
            return Err(-5);
        }
        Ok(Self {
            id: r.id,
            bytes,
            address: r.address,
            generation: 0,
            writing: true,
            failed: false,
        })
    }
    pub fn len(&self) -> usize {
        self.bytes
    }
    pub fn bytes(&self) -> &[u8] {
        assert!(!self.failed);
        unsafe { core::slice::from_raw_parts(self.address as *const u8, self.bytes) }
    }
    pub fn bytes_mut(&mut self) -> &mut [u8] {
        assert!(self.writing && !self.failed);
        unsafe { core::slice::from_raw_parts_mut(self.address as *mut u8, self.bytes) }
    }
    pub fn write(&mut self) -> Result<(), isize> {
        self.transition(op::WRITE).map(|_| ())
    }
    pub fn seal(&mut self) -> Result<Handle, isize> {
        self.transition(op::SEAL)?;
        Ok(Handle {
            id: self.id,
            generation: self.generation,
        })
    }
    fn transition(&mut self, operation: u64) -> Result<(), isize> {
        if self.failed {
            return Err(-5);
        }
        let r = match exchange(Message {
            operation,
            id: self.id,
            ..Message::default()
        }) {
            Ok(r) => r,
            Err(-13) => return Err(-13),
            Err(e) => {
                self.failed = true;
                return Err(e);
            }
        };
        if r.id != self.id
            || r.bytes != self.bytes as u64
            || r.address != self.address
            || r.writing != u64::from(operation == op::WRITE)
            || (operation == op::SEAL && (r.generation == 0 || r.generation < self.generation))
        {
            self.failed = true;
            return Err(-5);
        }
        self.generation = r.generation;
        self.writing = r.writing == 1;
        Ok(())
    }
    pub fn release(self) -> Result<(), Self> {
        if self.failed {
            return Err(self);
        }
        match exchange(Message {
            operation: op::RELEASE,
            id: self.id,
            ..Message::default()
        }) {
            Ok(r)
                if r == Message {
                    operation: op::RELEASE,
                    id: self.id,
                    ..Message::default()
                } =>
            {
                Ok(())
            }
            _ => Err(self),
        }
    }
}
fn exchange(mut message: Message) -> Result<Message, isize> {
    let operation = message.operation;
    let status = unsafe {
        crate::raw::syscall(
            SYSCALL,
            [&mut message as *mut _ as u64, BYTES as u64, 0, 0, 0],
        )
    } as isize;
    if status != 0 {
        return Err(status);
    }
    if message.operation != operation || message.reserved != [0; 2] {
        return Err(-5);
    }
    Ok(message)
}
