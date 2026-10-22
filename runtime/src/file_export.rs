//! Metadata-only destination display. Authority comes only from trusted HID.
pub const SCAN: u64 = 20;
pub const NEXT: u64 = 21;
pub const SELECT: u64 = 22;
pub const CONFIRM: u64 = 23;
pub const BACK: u64 = 24;
pub const ENTER: u64 = 25;
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct View {
    pub revision: u64,
    pub mode: u64,
    pub phase: u64,
    pub ports: u64,
    pub selected: u64,
    pub blocks: u64,
    pub status: u64,
    pub label: [u8; 28],
    pub filename: [u8; 12],
    /// Bounded, printable kernel diagnostic; no device identifiers or addresses.
    pub diagnostic: [u8; 64],
}
impl View {
    pub const EMPTY: Self = Self {
        revision: 0,
        mode: 0,
        phase: 0,
        ports: 0,
        selected: 0,
        blocks: 0,
        status: 0,
        label: [0; 28],
        filename: [0; 12],
        diagnostic: [0; 64],
    };
    pub fn canonical(&self) -> bool {
        self.mode <= 1
            && self.phase <= 5
            && self.ports <= u32::MAX as u64
            && self.selected <= 32
            && self.status <= 14
            && self.label.iter().all(|b| *b == 0 || (32..=126).contains(b))
            && self
                .diagnostic
                .iter()
                .all(|b| *b == 0 || (32..=126).contains(b))
            && self
                .filename
                .iter()
                .all(|b| *b == 0 || b.is_ascii_alphanumeric() || *b == b'.')
    }
    pub fn words(&self) -> [u64; 7] {
        [
            self.revision,
            self.mode,
            self.phase,
            self.ports,
            self.selected,
            self.blocks,
            self.status,
        ]
    }
}
