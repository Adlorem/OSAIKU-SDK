//! The PS/2 pointing device: a mouse, or a touchpad that answers as one.
//!
//! Every PS/2 touchpad, whoever made it, speaks the PS/2 mouse protocol until
//! it is told to do something else. That is how a firmware setup screen with
//! no driver for the pad still moves a pointer, and it is what lets one driver
//! here serve a Synaptics, an ALPS, an Elantech or a Cypress pad, a pointing
//! stick, or a plain mouse in the same socket, without a table of models. A
//! reset puts each of them into that mode, and the relative packet it produces
//! is the same for all of them. Tapping still clicks: in that mode the pad's
//! own firmware turns a tap into a press of the primary button.
//!
//! The vendor is asked anyway, with the knock each vendor's devices are known
//! to answer and using only standard commands, so that the device can be
//! named for what it is. Nothing below depends on the answer.
//!
//! No port is touched here. The controller owns the wire and lends it through
//! [`Wire`], which is what lets the whole sequence run on a host against a
//! scripted device. The order of the bring-up -- reset, self-test, touchpad
//! identification, device id, enable -- follows Redox OS's `ps2d`; see
//! THIRD_PARTY_NOTICES.md.

const SET_SCALING_1_TO_1: u8 = 0xe6;
const SET_SCALING_2_TO_1: u8 = 0xe7;
const SET_RESOLUTION: u8 = 0xe8;
const STATUS_REQUEST: u8 = 0xe9;
const GET_DEVICE_ID: u8 = 0xf2;
const ENABLE_REPORTING: u8 = 0xf4;
const DISABLE_REPORTING: u8 = 0xf5;
const RESET: u8 = 0xff;

const ACKNOWLEDGE: u8 = 0xfa;
const RESEND: u8 = 0xfe;
const FAILURE: u8 = 0xfc;
const SELF_TEST_PASSED: u8 = 0xaa;

/// One count is one pixel to the pointer, so the device's resolution is the
/// pointer's speed. At the default of four counts per millimetre a finger
/// drawn the width of a laptop pad stops well short of crossing the screen;
/// eight is the finest the standard defines.
const RESOLUTION_8_PER_MM: u8 = 3;

/// The device ids a PS/2 pointing device may report, and how long its packets
/// are. A wheel id means a fourth byte follows the standard three. The wheel
/// itself is not read -- the pointer has nowhere to send it -- but its byte
/// still has to be counted, or every packet after the first starts in the
/// wrong place. Some devices report 0xff where the standard says zero.
const STANDARD_ID: u8 = 0x00;
const WHEEL_ID: u8 = 0x03;
const FIVE_BUTTON_ID: u8 = 0x04;
const UNSPECIFIED_ID: u8 = 0xff;
const STANDARD_PACKET_LENGTH: u8 = 3;
const WHEEL_PACKET_LENGTH: u8 = 4;

/// How long a device has to answer, in milliseconds. Anything working answers
/// in a few; the bounds are for what is not working. Reset is slower because
/// some touchpads perform it before they acknowledge it, and the self-test
/// that follows slower again. These are the bounds Linux's PS/2 layer uses,
/// which have met more of these devices than anything else has.
const ACKNOWLEDGE_MS: u32 = 200;
const RESPONSE_MS: u32 = 500;
const RESET_ACKNOWLEDGE_MS: u32 = 1000;
const SELF_TEST_MS: u32 = 4000;
/// How long the wire has to stay quiet before a stopped device counts as
/// stopped. A device streaming at its fastest sends a packet every 5 ms.
const SETTLE_MS: u32 = 25;
/// A device asks for a byte again when it did not receive it cleanly. Twice
/// more is a retry; beyond that it is a device that will not take the byte.
const ATTEMPTS: u8 = 3;
/// Packets already on the wire when the device is told to stop are still
/// delivered. This many are skipped while waiting for the acknowledgement.
const STALE_BYTE_LIMIT: u8 = 32;

/// The first byte of every packet has bit 3 set, and that is the only thing a
/// packet carries that marks where it starts.
const ALWAYS_ONE: u8 = 1 << 3;
const X_NEGATIVE: u8 = 1 << 4;
const Y_NEGATIVE: u8 = 1 << 5;
/// Primary, secondary and middle, in the same bit order a USB boot report
/// uses, which is the order the pointer keeps them in.
const BUTTONS: u8 = 0b111;

/// The aux port as this module sees it: bytes out, bytes back.
pub trait Wire {
    /// Hands one byte to the device. False when the controller would not take
    /// it, which no retry here can change.
    fn send(&mut self, byte: u8) -> bool;
    /// The next byte from the device, or nothing once `milliseconds` pass
    /// without one.
    fn receive(&mut self, milliseconds: u32) -> Option<u8>;
}

/// Why a device could not be brought up.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Fault {
    /// The controller marked a byte as damaged or timed out on the wire.
    Transmission,
    /// Nothing answered. On a port with nothing behind it, this is the answer.
    Silent,
    /// The controller would not accept a byte for the device.
    Blocked,
    /// The device refused a command, or kept asking for it again.
    Refused,
    /// An answer the command does not have.
    Unexpected(u8),
    /// The device reset, and its self-test reported something other than a
    /// pass.
    SelfTestFailed(u8),
}

impl Fault {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Transmission => "transmission",
            Self::Silent => "silent",
            Self::Blocked => "blocked",
            Self::Refused => "refused",
            Self::Unexpected(_) => "unexpected",
            Self::SelfTestFailed(_) => "self-test-failed",
        }
    }

    /// The byte that made this a fault, where there was one.
    pub const fn byte(self) -> Option<u8> {
        match self {
            Self::Unexpected(byte) | Self::SelfTestFailed(byte) => Some(byte),
            _ => None,
        }
    }
}

/// Who made the device, as far as its answers say.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Family {
    /// Nothing answered any vendor's knock. A mouse, or a pad whose maker this
    /// does not know -- the protocol is the same either way.
    Generic,
    Synaptics,
    Cypress,
    Elantech,
    Alps,
}

impl Family {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Generic => "generic",
            Self::Synaptics => "synaptics",
            Self::Cypress => "cypress",
            Self::Elantech => "elantech",
            Self::Alps => "alps",
        }
    }

    /// Whether the knock that named this family is one only a touchpad
    /// answers. A generic device may still be a pad; it just did not say so.
    pub const fn touchpad(self) -> bool {
        !matches!(self, Self::Generic)
    }
}

/// A device that is streaming.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Device {
    pub family: Family,
    /// The three bytes the identifying query answered with: model and firmware
    /// revision for whoever made it, recorded rather than interpreted.
    pub signature: [u8; 3],
    pub id: u8,
    pub packet_length: u8,
}

/// Brings whatever is on the port into streaming relative packets.
///
/// Stops it first, because firmware may have left it streaming and its
/// packets would be read as answers; asks who made it; resets it, which is
/// what puts every vendor's device back into the standard mode and also
/// undoes whatever the knocks changed; sets the resolution; reads the id the
/// packet length follows from; and enables it.
pub fn probe(wire: &mut impl Wire) -> Result<Device, Fault> {
    let (family, signature) = match quiesce(wire) {
        Ok(()) => identify(wire),
        // Nothing behind the port, or a controller that will not talk to it.
        Err(fault @ (Fault::Silent | Fault::Blocked)) => return Err(fault),
        // Something is there and did not answer as asked -- a device left in
        // wrap mode echoes instead of acknowledging. A reset still reaches it.
        Err(_) => (Family::Generic, [0; 3]),
    };
    reset(wire)?;
    command_with_argument(wire, SET_RESOLUTION, RESOLUTION_8_PER_MM)?;
    command(wire, GET_DEVICE_ID)?;
    let id = wire.receive(RESPONSE_MS).ok_or(Fault::Silent)?;
    let packet_length = packet_length(id).ok_or(Fault::Unexpected(id))?;
    command(wire, ENABLE_REPORTING)?;
    Ok(Device {
        family,
        signature,
        id,
        packet_length,
    })
}

const fn packet_length(id: u8) -> Option<u8> {
    match id {
        STANDARD_ID | UNSPECIFIED_ID => Some(STANDARD_PACKET_LENGTH),
        WHEEL_ID | FIVE_BUTTON_ID => Some(WHEEL_PACKET_LENGTH),
        _ => None,
    }
}

/// Stops a device that may be streaming, and waits for the wire to go quiet.
///
/// The one command whose acknowledgement may arrive behind other bytes, so the
/// bytes in front of it are skipped, up to a bound. A packet byte can happen to
/// equal the acknowledgement; whatever of that packet and the real
/// acknowledgement follows is drained before anything else is asked.
///
/// Bytes followed by silence are not an empty port: something answered, just
/// not with an acknowledgement, as a device left in wrap mode echoes the stop.
fn quiesce(wire: &mut impl Wire) -> Result<(), Fault> {
    for _ in 0..ATTEMPTS {
        if !wire.send(DISABLE_REPORTING) {
            return Err(Fault::Blocked);
        }
        let mut skipped = 0u8;
        let mut last = None;
        loop {
            match wire.receive(ACKNOWLEDGE_MS) {
                Some(ACKNOWLEDGE) => {
                    drain(wire);
                    return Ok(());
                }
                Some(RESEND) => break,
                Some(FAILURE) => return Err(Fault::Refused),
                Some(byte) if skipped == STALE_BYTE_LIMIT => return Err(Fault::Unexpected(byte)),
                Some(byte) => {
                    skipped += 1;
                    last = Some(byte);
                }
                None => return Err(last.map_or(Fault::Silent, Fault::Unexpected)),
            }
        }
    }
    Err(Fault::Refused)
}

fn drain(wire: &mut impl Wire) {
    let mut drained = 0u8;
    while drained < STALE_BYTE_LIMIT && wire.receive(SETTLE_MS).is_some() {
        drained += 1;
    }
}

/// Asks each vendor's question in turn and keeps the first answer that is
/// that vendor's.
///
/// A device that refuses a knock, or stops answering in the middle of one, is
/// simply not that vendor: the reset that follows decides whether it works at
/// all, and puts right whatever the knock left behind.
fn identify(wire: &mut impl Wire) -> (Family, [u8; 3]) {
    // Query zero of the touchpad special command is Identify. Synaptics
    // answers with 0x47 in the middle; Cypress, which reuses the encoding,
    // with 0x33 0xcc. A mouse answers with its status.
    if let Ok(answer) = special_query(wire, 0) {
        if answer[1] == 0x47 {
            return (Family::Synaptics, answer);
        }
        if answer[0] == 0x33 && answer[1] == 0xcc {
            return (Family::Cypress, answer);
        }
    }
    // Elantech's knock is three 1:1 scalings, then the status request; its
    // pads answer 3c 03 and then c8 or 00.
    if let Ok(answer) = repeated_query(wire, DISABLE_REPORTING, None, SET_SCALING_1_TO_1) {
        if answer[0] == 0x3c && answer[1] == 0x03 && matches!(answer[2], 0xc8 | 0x00) {
            return (Family::Elantech, answer);
        }
    }
    if let Some(answer) = alps_signature(wire) {
        return (Family::Alps, answer);
    }
    (Family::Generic, [0; 3])
}

/// The touchpad special command: a 1:1 scaling, then four resolution settings
/// carrying two bits of the query each, highest first, then a status request,
/// which answers the query they spelled instead of reporting status.
fn special_query(wire: &mut impl Wire, query: u8) -> Result<[u8; 3], Fault> {
    command(wire, SET_SCALING_1_TO_1)?;
    for shift in [6, 4, 2, 0] {
        command_with_argument(wire, SET_RESOLUTION, (query >> shift) & 3)?;
    }
    status(wire)
}

/// One setup command, an optional resolution, the same command three times
/// over, and the status request that answers the knock.
fn repeated_query(
    wire: &mut impl Wire,
    setup: u8,
    resolution: Option<u8>,
    repeated: u8,
) -> Result<[u8; 3], Fault> {
    command(wire, setup)?;
    if let Some(resolution) = resolution {
        command_with_argument(wire, SET_RESOLUTION, resolution)?;
    }
    for _ in 0..3 {
        command(wire, repeated)?;
    }
    status(wire)
}

/// ALPS pads are told apart from a mouse by two reports.
///
/// After a resolution of zero and three 1:1 scalings, both answer with a status
/// of no flags, resolution zero, and a rate of 10 or 100. After three 2:1
/// scalings instead, a mouse can only echo its status -- the 2:1 flag, the same
/// resolution, the same rate -- because that is what the status request is.
/// An ALPS pad answers with a signature that names its model. So an answer
/// that is not the echo is the pad's; there is no table of models to keep.
fn alps_signature(wire: &mut impl Wire) -> Option<[u8; 3]> {
    let first = repeated_query(wire, DISABLE_REPORTING, Some(0), SET_SCALING_1_TO_1).ok()?;
    if first[0] & !BUTTONS != 0 || first[1] != 0 || !matches!(first[2], 10 | 100) {
        return None;
    }
    let second = repeated_query(wire, DISABLE_REPORTING, Some(0), SET_SCALING_2_TO_1).ok()?;
    let echo = second[0] & !BUTTONS == 0x10 && second[1] == 0 && second[2] == first[2];
    (!echo).then_some(second)
}

/// A reset, its acknowledgement, the self-test result, and the id that comes
/// after it. The id read later by `GET_DEVICE_ID` is the one acted on.
fn reset(wire: &mut impl Wire) -> Result<(), Fault> {
    send(wire, RESET, RESET_ACKNOWLEDGE_MS)?;
    match wire.receive(SELF_TEST_MS) {
        Some(SELF_TEST_PASSED) => {}
        Some(other) => return Err(Fault::SelfTestFailed(other)),
        None => return Err(Fault::Silent),
    }
    wire.receive(RESPONSE_MS).map(|_| ()).ok_or(Fault::Silent)
}

fn status(wire: &mut impl Wire) -> Result<[u8; 3], Fault> {
    command(wire, STATUS_REQUEST)?;
    let mut answer = [0; 3];
    for byte in &mut answer {
        *byte = wire.receive(RESPONSE_MS).ok_or(Fault::Silent)?;
    }
    Ok(answer)
}

fn command(wire: &mut impl Wire, byte: u8) -> Result<(), Fault> {
    send(wire, byte, ACKNOWLEDGE_MS)
}

/// A command that takes an argument is two bytes, and each is acknowledged on
/// its own.
fn command_with_argument(wire: &mut impl Wire, byte: u8, argument: u8) -> Result<(), Fault> {
    command(wire, byte)?;
    command(wire, argument)
}

/// One byte to the device and its acknowledgement, sent again when the device
/// asks for it again.
fn send(wire: &mut impl Wire, byte: u8, acknowledge_ms: u32) -> Result<(), Fault> {
    for _ in 0..ATTEMPTS {
        if !wire.send(byte) {
            return Err(Fault::Blocked);
        }
        match wire.receive(acknowledge_ms) {
            Some(ACKNOWLEDGE) => return Ok(()),
            Some(RESEND) => {}
            Some(FAILURE) => return Err(Fault::Refused),
            Some(other) => return Err(Fault::Unexpected(other)),
            None => return Err(Fault::Silent),
        }
    }
    Err(Fault::Refused)
}

/// What one byte from a streaming device amounts to.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Event {
    /// Part of a packet, or something with no meaning to anyone above.
    Pending,
    /// A whole packet: motion with positive `dy` downward, as on the screen,
    /// and the buttons it says are held.
    Motion {
        dx: i16,
        dy: i16,
        buttons: u8,
    },
    /// The device announced that it reset itself: whatever it held is gone,
    /// and it has to be configured again, starting with this byte.
    Restarted(u8),
    /// The next byte of that configuration.
    Send(u8),
    Recovered,
    Failed(Fault),
}

/// What a device that reset itself needs before it streams again: the
/// resolution set at bring-up, then reporting. Its id after a reset is the
/// zero it just announced, so the packet length is the standard one.
const RECONFIGURE: [u8; 3] = [SET_RESOLUTION, RESOLUTION_8_PER_MM, ENABLE_REPORTING];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Phase {
    Streaming,
    /// Waiting for the acknowledgement of `RECONFIGURE[step]`.
    Configuring {
        step: u8,
        attempts: u8,
    },
    /// Reset ACK, self-test and device id, in that order.
    Resetting {
        step: u8,
    },
    /// The device would not take its configuration. Only another reset
    /// announcement gets it out of this.
    Abandoned,
}

/// The bytes of a streaming device, put back together into packets.
pub struct Stream {
    length: u8,
    packet: [u8; 4],
    filled: u8,
    last_byte_at: u64,
    gap_cycles: u64,
    phase: Phase,
    deadline: u64,
    reset_attempts: u8,
}

impl Stream {
    /// `gap_cycles` is how long a packet may stall between two of its bytes
    /// before what was collected of it is thrown away.
    ///
    /// A device sends a packet's bytes back to back, so a long gap inside one
    /// means a byte was lost and the next one starts a new packet. Half a
    /// second is the bound Linux settled on: long enough that a slow reader is
    /// never mistaken for a lost byte, and still short next to the pause of a
    /// finger being lifted, which is where a misaligned stream comes right.
    pub const fn new(device: &Device, gap_cycles: u64) -> Self {
        Self {
            length: device.packet_length,
            packet: [0; 4],
            filled: 0,
            last_byte_at: 0,
            gap_cycles,
            phase: Phase::Streaming,
            deadline: 0,
            reset_attempts: 0,
        }
    }

    pub fn accept(&mut self, byte: u8, now: u64) -> Event {
        if self.filled != 0 && now.wrapping_sub(self.last_byte_at) > self.gap_cycles {
            self.filled = 0;
        }
        self.last_byte_at = now;
        match self.phase {
            Phase::Streaming => self.packet_byte(byte),
            Phase::Configuring { .. } | Phase::Abandoned => self.configuration_byte(byte),
            Phase::Resetting { step } => self.reset_byte(step, byte),
        }
    }

    /// Read by inputd's readiness check, including when no further IRQ arrives.
    pub fn work_due(&self, now: u64) -> bool {
        self.deadline != 0 && now >= self.deadline
    }

    pub fn work_deadline(&self) -> u64 {
        self.deadline
    }

    pub fn poll(&mut self, now: u64) -> Event {
        if self.work_due(now) {
            self.recover(now, Fault::Silent)
        } else {
            Event::Pending
        }
    }

    pub fn transmission_error(&mut self, now: u64) -> Event {
        // Once quarantined, damaged bytes cannot restart an endless command
        // loop. Only a clean device reset announcement can restore the source.
        if matches!(self.phase, Phase::Abandoned) {
            return Event::Pending;
        }
        self.recover(now, Fault::Transmission)
    }

    /// Start the response timeout when the controller has accepted the command.
    pub fn command_sent(&mut self, now: u64) {
        self.arm_wait(now);
    }

    fn arm_wait(&mut self, now: u64) {
        let milliseconds = match self.phase {
            Phase::Resetting { step: 0 } => RESET_ACKNOWLEDGE_MS,
            Phase::Resetting { step: 1 } => SELF_TEST_MS,
            Phase::Resetting { .. } => RESPONSE_MS,
            Phase::Configuring { .. } => ACKNOWLEDGE_MS,
            Phase::Streaming | Phase::Abandoned => {
                self.deadline = 0;
                return;
            }
        };
        // gap_cycles is half a second, from the calibrated controller clock.
        let cycles = self.gap_cycles.saturating_mul(u64::from(milliseconds)) / 500;
        self.deadline = now.saturating_add(cycles.max(1));
    }

    fn recover(&mut self, now: u64, fault: Fault) -> Event {
        self.filled = 0;
        if self.reset_attempts == ATTEMPTS {
            self.abandon();
            return Event::Failed(fault);
        }
        self.reset_attempts += 1;
        self.phase = Phase::Resetting { step: 0 };
        self.arm_wait(now);
        // A reset also resolves an uncertain E8 argument/ACK boundary. Blindly
        // repeating E8 could otherwise send a command as its own argument.
        Event::Restarted(RESET)
    }

    fn reset_byte(&mut self, step: u8, byte: u8) -> Event {
        match (step, byte) {
            (0, ACKNOWLEDGE) | (1, SELF_TEST_PASSED) => {
                self.phase = Phase::Resetting { step: step + 1 };
                self.arm_wait(self.last_byte_at);
                Event::Pending
            }
            (2, STANDARD_ID | UNSPECIFIED_ID) => self.restart(),
            (0, RESEND | FAILURE) => self.recover(self.last_byte_at, Fault::Refused),
            // A packet already on the wire can precede the reset ACK. Its
            // bytes neither advance the handshake nor extend its deadline.
            (0, _) => Event::Pending,
            (1, byte) => self.recover(self.last_byte_at, Fault::SelfTestFailed(byte)),
            (_, byte) => self.recover(self.last_byte_at, Fault::Unexpected(byte)),
        }
    }

    /// A byte meant for the device could not be sent, so no acknowledgement is
    /// coming and waiting for one would wait forever.
    pub fn abandon(&mut self) {
        self.phase = Phase::Abandoned;
        self.filled = 0;
        self.deadline = 0;
    }

    fn packet_byte(&mut self, byte: u8) -> Event {
        // Out of step. Every byte is dropped until one that can start a packet,
        // which is the only resynchronisation the protocol allows.
        if self.filled == 0 && byte & ALWAYS_ONE == 0 {
            return Event::Pending;
        }
        self.packet[usize::from(self.filled)] = byte;
        self.filled += 1;
        // A device that resets itself -- reconnected, or reset by the embedded
        // controller -- announces it with its self-test pass and its id, and
        // then sends nothing until it is enabled again. Read as a packet the
        // pair is legal, which is why it is only taken as an announcement at
        // the start of one.
        if self.filled == 2 && self.packet[0] == SELF_TEST_PASSED && byte == STANDARD_ID {
            return self.restart();
        }
        if self.filled < self.length {
            return Event::Pending;
        }
        self.filled = 0;
        decode(self.packet)
    }

    fn configuration_byte(&mut self, byte: u8) -> Event {
        let announced = self.filled == 1 && self.packet[0] == SELF_TEST_PASSED;
        self.filled = 0;
        if announced && byte == STANDARD_ID {
            return self.restart();
        }
        if byte == SELF_TEST_PASSED {
            self.packet[0] = byte;
            self.filled = 1;
            return Event::Pending;
        }
        let Phase::Configuring { step, attempts } = self.phase else {
            return Event::Pending;
        };
        match byte {
            ACKNOWLEDGE => {
                let step = step + 1;
                if usize::from(step) == RECONFIGURE.len() {
                    self.phase = Phase::Streaming;
                    self.deadline = 0;
                    self.reset_attempts = 0;
                    return Event::Recovered;
                }
                self.phase = Phase::Configuring { step, attempts: 0 };
                self.arm_wait(self.last_byte_at);
                Event::Send(RECONFIGURE[usize::from(step)])
            }
            RESEND if attempts + 1 < ATTEMPTS => {
                self.phase = Phase::Configuring {
                    step,
                    attempts: attempts + 1,
                };
                self.arm_wait(self.last_byte_at);
                Event::Send(RECONFIGURE[usize::from(step)])
            }
            _ => self.recover(self.last_byte_at, Fault::Unexpected(byte)),
        }
    }

    fn restart(&mut self) -> Event {
        self.filled = 0;
        self.length = STANDARD_PACKET_LENGTH;
        self.phase = Phase::Configuring {
            step: 0,
            attempts: 0,
        };
        self.arm_wait(self.last_byte_at);
        Event::Restarted(RECONFIGURE[0])
    }
}

/// The two motion bytes are nine-bit two's complement, their sign bits in the
/// first byte. Overflow bits are not read: a device that sets them has already
/// saturated the delta, and the saturated value still says which way it went.
/// A wheel byte, when there is one, is not read either.
fn decode(packet: [u8; 4]) -> Event {
    let flags = packet[0];
    Event::Motion {
        dx: axis(packet[1], flags & X_NEGATIVE != 0),
        // PS/2 counts upward motion as positive; the screen counts rows down.
        dy: -axis(packet[2], flags & Y_NEGATIVE != 0),
        buttons: flags & BUTTONS,
    }
}

/// A zero magnitude is zero whatever its sign bit says. Some devices set the
/// sign on an axis that did not move, and read literally that is a jump of 256.
fn axis(magnitude: u8, negative: bool) -> i16 {
    if magnitude == 0 {
        return 0;
    }
    i16::from(magnitude) - if negative { 256 } else { 0 }
}
