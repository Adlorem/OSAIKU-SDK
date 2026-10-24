//! Copied i8042 wire records and a device-command allowlist. No port addresses.
use crate::ps2_pointer::Event;
/// Byte/status/packet length/reserved/monotonic milliseconds, little endian.
pub fn record(byte: u8, status: u8, now: u64) -> [u8; 8] {
    let mut b = [byte, status, 3, 0, 0, 0, 0, 0];
    b[4..].copy_from_slice(&(now as u32).to_le_bytes());
    b
}
/// Timestamp extension across the 32-bit wire clock wrap.
pub struct Clock {
    last: u32,
    high: u64,
}
impl Clock {
    pub const fn new() -> Self {
        Self { last: 0, high: 0 }
    }
    pub fn accept(&mut self, low: u32) -> u64 {
        if low < self.last {
            self.high += 1u64 << 32;
        }
        self.last = low;
        self.high | u64::from(low)
    }
}
/// Package response: delta X/Y, buttons, action, command, reserved.
pub fn response(event: Event) -> [u8; 8] {
    let mut b = [0; 8];
    match event {
        Event::Pending => {}
        Event::Motion { dx, dy, buttons } => {
            b[..2].copy_from_slice(&dx.to_le_bytes());
            b[2..4].copy_from_slice(&dy.to_le_bytes());
            b[4] = buttons;
            b[5] = 1;
        }
        Event::Send(c) => {
            b[5] = 2;
            b[6] = c;
        }
        Event::Restarted(c) => {
            b[5] = 3;
            b[6] = c;
        }
        Event::Failed(_) => b[5] = 4,
        Event::Recovered => b[5] = 5,
    }
    b
}
pub fn valid_response(b: &[u8]) -> bool {
    if b.len() != 8 || b[7] != 0 {
        return false;
    }
    match b[5] {
        1 => {
            b[4] & !7 == 0
                && b[6] == 0
                && (-255..=255).contains(&i16::from_le_bytes([b[0], b[1]]))
                && (-255..=255).contains(&i16::from_le_bytes([b[2], b[3]]))
        }
        0 | 4 | 5 => b[..5] == [0; 5] && b[6] == 0,
        2 | 3 => b[..5] == [0; 5] && matches!(b[6], 0xff | 0xe8 | 3 | 0xf4),
        _ => false,
    }
}
/// Stateful allowlist for the AUX device, never the controller command port.
/// A timed-out command cannot be reused: only a device reset resolves its boundary.
pub struct Commands {
    argument: bool,
    pending: Option<u8>,
    deadline: u64,
    reset: u8,
    resets: u8,
    window: u64,
    commands: u8,
}
impl Commands {
    pub const fn new() -> Self {
        Self {
            argument: false,
            pending: None,
            deadline: 0,
            reset: 0,
            resets: 0,
            window: 0,
            commands: 0,
        }
    }
    pub fn admit(&mut self, byte: u8, now: u64) -> bool {
        if now.saturating_sub(self.window) >= 1000 {
            self.window = now;
            self.commands = 0;
        }
        if self.commands >= 32 || (byte == 0xff && self.resets >= 3) {
            return false;
        }
        if byte == 0xff {
            self.resets += 1;
            self.argument = false;
            self.reset = 1;
        } else {
            if self.pending.is_some() || self.reset != 0 {
                return false;
            }
            if self.argument {
                if byte != 3 {
                    return false;
                }
            } else if !matches!(byte, 0xe8 | 0xf4) {
                return false;
            }
        }
        self.commands += 1;
        self.pending = Some(byte);
        self.deadline = now.saturating_add(if byte == 0xff { 1000 } else { 200 });
        true
    }
    pub fn receive(&mut self, byte: u8, now: u64) {
        if self.pending.is_none() || now > self.deadline {
            return;
        }
        if self.reset == 2 && byte == 0xaa {
            self.reset = 3;
            self.deadline = now.saturating_add(500);
            return;
        }
        if self.reset == 3 && matches!(byte, 0 | 0xff) {
            self.reset = 0;
            self.pending = None;
            return;
        }
        if self.reset > 1 {
            return;
        }
        match byte {
            0xfa if self.reset == 1 => {
                self.reset = 2;
                self.deadline = now.saturating_add(4000);
            }
            0xfa => {
                self.argument = self.pending == Some(0xe8);
                self.pending = None;
            }
            0xfe => {
                self.reset = 0;
                self.pending = None;
            }
            _ => {}
        }
    }
}
/// Controller-owned Caps LED exchange; it never consumes scancodes or AUX bytes.
pub struct Lamps {
    desired: u8,
    applied: u8,
    phase: u8,
    attempts: u8,
    deadline: u64,
}
impl Lamps {
    pub const fn new() -> Self {
        Self {
            desired: 0,
            applied: 0,
            phase: 0,
            attempts: 0,
            deadline: 0,
        }
    }
    pub fn request(&mut self, on: bool) {
        self.desired = if on { 4 } else { 0 };
    }
    pub fn wait_ms(&self, now: u64) -> Option<u64> {
        match self.phase {
            5 => None,
            2 | 4 => Some(self.deadline.saturating_sub(now)),
            1 | 3 => Some(0),
            _ if self.desired != self.applied => Some(0),
            _ => None,
        }
    }
    pub fn next(&mut self, now: u64) -> Option<u8> {
        if self.phase == 5 {
            return None;
        }
        if self.phase == 0 {
            if self.desired == self.applied || now < self.deadline {
                return None;
            }
            self.phase = 1;
            self.attempts = 0;
        }
        if self.phase == 2 || self.phase == 4 {
            if now < self.deadline {
                return None;
            }
            // An uncertain argument boundary cannot be replayed as a command.
            self.phase = 5;
            self.applied = self.desired;
            self.deadline = 0;
            return None;
        }
        let byte = if self.phase == 1 { 0xed } else { self.applied };
        if self.phase == 1 {
            self.phase = 2;
        } else {
            self.phase = 4;
        }
        self.deadline = now.saturating_add(200);
        Some(byte)
    }
    pub fn receive(&mut self, byte: u8) -> bool {
        if !matches!(byte, 0xfa | 0xfe | 0xfc) {
            return false;
        }
        if self.phase == 5 {
            return true;
        }
        match (self.phase, byte) {
            (2, 0xfa) => {
                self.applied = self.desired;
                self.phase = 3;
                self.attempts = 0;
            }
            (4, 0xfa) => {
                self.phase = 0;
                self.deadline = 0;
            }
            (2 | 4, 0xfe) if self.attempts < 2 => {
                self.attempts += 1;
                self.phase -= 1;
            }
            (2 | 4, _) => {
                self.phase = 5;
                self.deadline = 0;
            }
            _ => {}
        }
        true
    }
    pub fn blocked(&mut self) {
        self.phase = 5;
        self.applied = self.desired;
        self.deadline = 0;
    }
}

/// Decode only a sampled controller status, without consuming a data byte.
/// 0xff means absent; a damaged byte remains attributed to its actual channel.
pub fn output(status: u8) -> Option<(bool, bool)> {
    if status == 0xff || status & 1 == 0 {
        None
    } else {
        Some((status & 0x20 != 0, status & 0xc0 != 0))
    }
}

/// Controller-owned framing only: one complete translated Set 1 sequence per
/// receipt. A package replacement cannot inherit half of an E0/E1 prefix.
/// Record: byte count, damaged flag, up to six wire bytes. No layout/key map.
pub struct KeyboardFrames {
    bytes: [u8; 6],
    len: usize,
    want: usize,
    last: u64,
}
impl KeyboardFrames {
    pub const fn new() -> Self {
        Self {
            bytes: [0; 6],
            len: 0,
            want: 0,
            last: 0,
        }
    }
    pub fn accept(&mut self, byte: u8, damaged: bool, now: u64) -> Option<[u8; 8]> {
        if damaged
            || (self.len != 0 && now.saturating_sub(self.last) > 500)
            || (self.want == 2 && self.len == 1 && matches!(byte, 0xe0 | 0xe1))
        {
            self.len = 0;
            self.want = 0;
            self.bytes.fill(0);
            return Some([0, 1, 0, 0, 0, 0, 0, 0]);
        }
        self.last = now;
        if self.len == 0 {
            self.want = match byte {
                0xe0 => 2,
                0xe1 => 6,
                _ => 1,
            };
        }
        self.bytes[self.len] = byte;
        self.len += 1;
        if self.len != self.want {
            return None;
        }
        let mut out = [0; 8];
        out[0] = self.len as u8;
        out[2..].copy_from_slice(&self.bytes);
        self.len = 0;
        self.want = 0;
        self.bytes.fill(0);
        Some(out)
    }
}
/// Decode one controller-framed keyboard record. Any malformed or damaged
/// record clears parser prefixes and releases only this keyboard source.
pub fn keyboard(decoder: &mut crate::set1::Decoder, b: &[u8]) -> crate::hid::Poll {
    use crate::hid::Poll;
    if b.len() != 8
        || b[1] != 0
        || !matches!(b[0], 1 | 2 | 6)
        || b[2 + usize::from(b[0])..].iter().any(|v| *v != 0)
        || (b[0] == 1 && matches!(b[2], 0xe0 | 0xe1))
        || (b[0] == 2 && (b[2] != 0xe0 || matches!(b[3], 0xe0 | 0xe1)))
        || (b[0] == 6 && b[2] != 0xe1)
    {
        *decoder = crate::set1::Decoder::new();
        return Poll::Lost;
    }
    let mut event = Poll::Ignored;
    for byte in &b[2..2 + usize::from(b[0])] {
        event = decoder.accept(*byte);
    }
    event
}
