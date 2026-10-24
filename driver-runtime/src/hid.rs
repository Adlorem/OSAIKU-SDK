//! USB HID Boot Protocol only. Physical usages and relative motion, never text/focus.
pub use crate::class::Kind;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct KeyEvent {
    pub usage: u8,
    pub pressed: bool,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Poll {
    NoEvent,
    Ignored,
    Event(KeyEvent),
    Lost,
}
impl Poll {
    pub const fn encode(self) -> u16 {
        match self {
            Self::NoEvent => 0,
            Self::Ignored => 1,
            Self::Lost => 2,
            Self::Event(e) => 0x4000 | ((e.pressed as u16) << 15) | e.usage as u16,
        }
    }
    pub fn decode(value: u16) -> Option<Self> {
        match value {
            0 => Some(Self::NoEvent),
            1 => Some(Self::Ignored),
            2 => Some(Self::Lost),
            _ if value & 0x3f00 == 0 && value & 0x4000 != 0 => Some(Self::Event(KeyEvent {
                usage: value as u8,
                pressed: value & 0x8000 != 0,
            })),
            _ => None,
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Pointer {
    pub dx: i16,
    pub dy: i16,
    pub buttons: u8,
}
pub fn mouse(bytes: &[u8]) -> Option<Pointer> {
    if bytes.len() != 3 {
        return None;
    }
    Some(Pointer {
        dx: bytes[1] as i8 as i16,
        dy: bytes[2] as i8 as i16,
        buttons: bytes[0] & 7,
    })
}
pub struct Decoder {
    modifiers: u8,
    keys: [u8; 6],
    neutral: bool,
}
impl Decoder {
    pub const fn new() -> Self {
        Self {
            modifiers: 0,
            keys: [0; 6],
            neutral: false,
        }
    }
    pub fn lose(&mut self) {
        self.modifiers = 0;
        self.keys = [0; 6];
        self.neutral = true;
    }
    pub fn report(&mut self, modifiers: u8, keys: [u8; 6]) -> [Poll; 20] {
        let mut events = [Poll::NoEvent; 20];
        if keys.iter().any(|key| (1..=3).contains(key) || *key >= 0xe0) {
            self.lose();
            events[0] = Poll::Lost;
            return events;
        }
        if self.neutral {
            if modifiers == 0 && keys == [0; 6] {
                self.neutral = false;
            }
            return events;
        }
        let mut count = 0;
        for bit in 0..8 {
            if (modifiers ^ self.modifiers) & (1 << bit) != 0 {
                events[count] = Poll::Event(KeyEvent {
                    usage: 0xe0 + bit,
                    pressed: modifiers & (1 << bit) != 0,
                });
                count += 1;
            }
        }
        for (index, usage) in self.keys.iter().copied().enumerate() {
            if usage != 0 && !keys.contains(&usage) && !self.keys[..index].contains(&usage) {
                events[count] = Poll::Event(KeyEvent {
                    usage,
                    pressed: false,
                });
                count += 1;
            }
        }
        for (index, usage) in keys.iter().copied().enumerate() {
            if usage != 0 && !self.keys.contains(&usage) && !keys[..index].contains(&usage) {
                events[count] = Poll::Event(KeyEvent {
                    usage,
                    pressed: true,
                });
                count += 1;
            }
        }
        self.modifiers = modifiers;
        self.keys = keys;
        events
    }
}

impl Default for Decoder {
    fn default() -> Self {
        Self::new()
    }
}
impl Decoder {
    pub fn bytes(&mut self, bytes: &[u8]) -> [Poll; 20] {
        // HID 1.11 B.1/F.4: byte 1 is OEM/reserved and must be ignored.
        if bytes.len() != 8 {
            self.lose();
            let mut out = [Poll::NoEvent; 20];
            out[0] = Poll::Lost;
            return out;
        }
        self.report(bytes[0], bytes[2..8].try_into().unwrap())
    }
}
