//! Process-local composition input. Pointer fields are copied/sealed by the
//! client and are absent from the public renderer wire representation.
#[derive(Clone, Copy)]
#[repr(C)]
/// For local upload operations 2..=5, result is [sealed bytes, attempted tiles,
/// attempted submissions, wall milliseconds]; unknown clock is u64::MAX.
pub struct Command {
    pub op: u32,
    pub slot: u32,
    pub width: u32,
    pub height: u32,
    pub rect: [u32; 4],
    pub clip: [u32; 4],
    pub window: [u32; 4],
    pub args: [u64; 4],
    pub data: *const u8,
    pub len: usize,
    pub result: [u64; 4],
}
impl Command {
    pub const EMPTY: Self = Self {
        op: 0,
        slot: 0,
        width: 0,
        height: 0,
        rect: [0; 4],
        clip: [0; 4],
        window: [0; 4],
        args: [0; 4],
        data: core::ptr::null(),
        len: 0,
        result: [0; 4],
    };
    pub fn wire(self) -> osaiku_gpu_protocol::compositor::Command {
        osaiku_gpu_protocol::compositor::Command {
            op: self.op,
            slot: self.slot,
            width: self.width,
            height: self.height,
            rect: self.rect,
            clip: self.clip,
            window: self.window,
            args: self.args,
            ..osaiku_gpu_protocol::compositor::Command::EMPTY
        }
    }
}
const _: () = assert!(core::mem::size_of::<Command>() == 144);
pub fn deferred(c: &Command) -> bool {
    c.data.is_null() && c.len == 0 && matches!(c.op, 6 | 7 | 8 | 9 | 12 | 13)
}
