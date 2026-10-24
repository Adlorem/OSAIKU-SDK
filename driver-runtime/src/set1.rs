//! Bounded set-1 wire decoder; layout belongs to the input consumer.
use crate::hid::{KeyEvent, Poll};
pub struct Decoder {
    extended: bool,
    pause: u8,
}
impl Decoder {
    pub const fn new() -> Self {
        Self {
            extended: false,
            pause: 0,
        }
    }
    pub fn accept(&mut self, byte: u8) -> Poll {
        if self.pause != 0 {
            self.pause -= 1;
            return Poll::Ignored;
        }
        if byte == 0xe1 {
            self.extended = false;
            self.pause = 5;
            return Poll::Ignored;
        }
        if byte == 0xe0 {
            self.extended = true;
            return Poll::Ignored;
        }
        let extended = core::mem::replace(&mut self.extended, false);
        let code = byte & 0x7f;
        let usage = if extended {
            match code {
                0x1d => 0xe4,
                0x38 => 0xe6,
                0x49 => 0x4b,
                0x51 => 0x4e,
                0x4b => 0x50,
                0x4d => 0x4f,
                0x48 => 0x52,
                0x50 => 0x51,
                _ => return Poll::Ignored,
            }
        } else {
            match code {
                0x1e => 0x4,
                0x30 => 0x5,
                0x2e => 0x6,
                0x20 => 0x7,
                0x12 => 0x8,
                0x21 => 0x9,
                0x22 => 0xa,
                0x23 => 0xb,
                0x17 => 0xc,
                0x24 => 0xd,
                0x25 => 0xe,
                0x26 => 0xf,
                0x32 => 0x10,
                0x31 => 0x11,
                0x18 => 0x12,
                0x19 => 0x13,
                0x10 => 0x14,
                0x13 => 0x15,
                0x1f => 0x16,
                0x14 => 0x17,
                0x16 => 0x18,
                0x2f => 0x19,
                0x11 => 0x1a,
                0x2d => 0x1b,
                0x15 => 0x1c,
                0x2c => 0x1d,
                0x02 => 0x1e,
                0x03 => 0x1f,
                0x04 => 0x20,
                0x05 => 0x21,
                0x06 => 0x22,
                0x07 => 0x23,
                0x08 => 0x24,
                0x09 => 0x25,
                0x0a => 0x26,
                0x0b => 0x27,
                0x39 => 0x2c,
                0x33 => 0x36,
                0x34 => 0x37,
                0x35 => 0x38,
                0x01 => 0x29,
                0x0e => 0x2a,
                0x0f => 0x2b,
                0x1c => 0x28,
                0x1d => 0xe0,
                0x2a => 0xe1,
                0x36 => 0xe5,
                0x38 => 0xe2,
                0x3a => 0x39,
                _ => return Poll::Ignored,
            }
        };
        Poll::Event(KeyEvent {
            usage,
            pressed: byte & 0x80 == 0,
        })
    }
}
