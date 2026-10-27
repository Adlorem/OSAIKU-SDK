//! Metadata-only diagnostic save receipt. Authority comes from trusted HID.
// USB export actions 20..=25 are retired; never reuse their meanings.
pub const SAVE: u64 = 53;
/// Explicit, separately labelled HID action; append to the boot USB's FAT root.
pub const SAVE_BOOT_USB: u64 = 54;
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct View {
    pub revision: u64,
    /// Destination: 0 = native Diagnostics, 1 = explicit boot USB report.
    /// Retains the former reserved field's wire offset and the 160-byte layout.
    pub reserved_mode: u64,
    pub phase: u64,
    pub reserved_ports: [u64; 2],
    pub mount: u64,
    pub status: u64,
    pub reserved_label: [u8; 28],
    pub filename: [u8; 12],
    /// Bounded technical detail, supplementary to the application's error text.
    pub diagnostic: [u8; 64],
}
impl View {
    pub const EMPTY: Self = Self {
        revision: 0,
        reserved_mode: 0,
        phase: 0,
        reserved_ports: [0; 2],
        mount: 0,
        status: 0,
        reserved_label: [0; 28],
        filename: [0; 12],
        diagnostic: [0; 64],
    };
    pub fn canonical(&self) -> bool {
        self.reserved_mode <= 1
            && matches!(self.phase, 0 | 3 | 4 | 5)
            && self.reserved_ports == [0; 2]
            && self.reserved_label == [0; 28]
            && self.status <= 14
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
            self.reserved_mode,
            self.phase,
            self.reserved_ports[0],
            self.reserved_ports[1],
            self.mount,
            self.status,
        ]
    }
}
const _: () = assert!(core::mem::size_of::<View>() == 160);
const _: () = assert!(SAVE > crate::file_broker::INPUT);
