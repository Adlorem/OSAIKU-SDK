//! Installed mediated device classes. Values are stable process endpoints.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Kind {
    Keyboard = 1,
    Mouse = 2,
    Ps2Keyboard = 3,
    Ps2Pointer = 4,
    UsbStorage = 5,
}
impl Kind {
    pub const ALL: [Self; 5] = [
        Self::Keyboard,
        Self::Mouse,
        Self::Ps2Keyboard,
        Self::Ps2Pointer,
        Self::UsbStorage,
    ];
    pub const fn index(self) -> usize {
        self as usize - 1
    }
    pub const fn report_bytes(self) -> usize {
        match self {
            Self::Keyboard | Self::Ps2Keyboard | Self::Ps2Pointer => 8,
            Self::Mouse => 3,
            Self::UsbStorage => 0,
        }
    }
}
