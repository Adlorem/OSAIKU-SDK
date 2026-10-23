//! One-shot assistant document-status exchange. No text, title or body hash.
//! The kernel authorizes the exact exchange from a physical Ctrl+I gesture;
//! these bytes alone confer no authority. Scheduling admission is unavailable.
use crate::process::ProcessEndpoint;

pub const BYTES: usize = 80;
const MAGIC: u64 = 0x3353544154534344; // DCSTATS3
pub const START: u64 = 1;
pub const QUERY: u64 = 2;
pub const METADATA: u64 = 3;
pub const RESULT: u64 = 4;
pub const DISPLAY: u64 = 5;
pub const CANCEL: u64 = 6;
/// Assistant forwards the actual metadata for bounded worker assessment.
pub const ASSESS: u64 = 7;
pub const EVENT_BYTES: usize = BYTES + 8;
pub const REPORT: u64 = 1;
pub const DISPLAYED: u64 = 2;
pub const FAILED: u64 = 3;
/// A physical gesture awaiting forwarding by the lifecycle host.
pub const REQUESTED: u64 = 4;
pub const SAVED: u8 = 1;
pub const UNSAVED: u8 = 2;
pub const VOLATILE: u8 = 3;
pub const STORAGE_ERROR: u8 = 4;
pub const UNKNOWN: u8 = 5;
pub const UNAVAILABLE: u8 = 6;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Message {
    pub kind: u64,
    pub request: u64,
    pub owner: ProcessEndpoint,
    pub window: u64,
    pub generation: u64,
    pub revision: u64,
    pub length: u64,
    pub save: u64,
    pub result: u64,
}

impl Message {
    pub fn encode(self) -> [u8; BYTES] {
        let mut bytes = [0; BYTES];
        for (value, chunk) in [
            MAGIC,
            self.kind,
            self.request,
            self.owner.encode(),
            self.window,
            self.generation,
            self.revision,
            self.length,
            self.save,
            self.result,
        ]
        .into_iter()
        .zip(bytes.chunks_exact_mut(8))
        {
            chunk.copy_from_slice(&value.to_le_bytes());
        }
        bytes
    }

    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != BYTES || !candidate(bytes) {
            return None;
        }
        let word = |i: usize| u64::from_le_bytes(bytes[i * 8..i * 8 + 8].try_into().unwrap());
        let message = Self {
            kind: word(1),
            request: word(2),
            owner: ProcessEndpoint::decode(word(3)),
            window: word(4),
            generation: word(5),
            revision: word(6),
            length: word(7),
            save: word(8),
            result: word(9),
        };
        if !(START..=ASSESS).contains(&message.kind)
            || message.request == 0
            || !message.owner.names_instance()
            || message.window == 0
            || message.generation == 0
            || message.revision == 0
            || message.length > 4096
            || message.save > u8::MAX as u64
            || message.result > u64::from(UNAVAILABLE)
        {
            return None;
        }
        match message.kind {
            START | QUERY | CANCEL
                if message.length != 0 || message.save != 0 || message.result != 0 =>
            {
                None
            }
            METADATA | ASSESS if message.result != 0 => None,
            RESULT | DISPLAY if message.result == 0 => None,
            _ => Some(message),
        }
    }

    pub fn same_request(self, other: Self) -> bool {
        self.request == other.request
            && self.owner == other.owner
            && self.window == other.window
            && self.generation == other.generation
            && self.revision == other.revision
    }
}

pub fn candidate(bytes: &[u8]) -> bool {
    bytes.get(..8) == Some(MAGIC.to_le_bytes().as_slice())
}

/// Only durable-current means saved. Volatile saves remain explicitly volatile.
pub fn assessment(save: u64) -> u8 {
    use crate::compat::*;
    match u8::try_from(save).ok() {
        Some(DOCUMENT_EDITOR_SAVE_DURABLE_CURRENT) => SAVED,
        Some(DOCUMENT_EDITOR_SAVE_VOLATILE_CURRENT) => VOLATILE,
        Some(
            DOCUMENT_EDITOR_SAVE_NEVER_SAVED
            | DOCUMENT_EDITOR_SAVE_VOLATILE_STALE
            | DOCUMENT_EDITOR_SAVE_DURABLE_STALE,
        ) => UNSAVED,
        Some(state) if document_editor_storage_refused_state(state) => STORAGE_ERROR,
        _ => UNKNOWN,
    }
}

/// A desktop-only receipt, separate from ordinary RPC mailboxes. FAILED never
/// represents an application acknowledgement or authorizes a repaint.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Event {
    pub kind: u64,
    pub message: Message,
}
impl Event {
    pub fn encode(self) -> [u8; EVENT_BYTES] {
        let mut bytes = [0; EVENT_BYTES];
        bytes[..8].copy_from_slice(&self.kind.to_le_bytes());
        bytes[8..].copy_from_slice(&self.message.encode());
        bytes
    }
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != EVENT_BYTES {
            return None;
        }
        let kind = u64::from_le_bytes(bytes[..8].try_into().ok()?);
        let message = Message::decode(&bytes[8..])?;
        match kind {
            REQUESTED if message.kind == START => Some(Self { kind, message }),
            REPORT if message.kind == RESULT => Some(Self { kind, message }),
            DISPLAYED if message.kind == DISPLAY => Some(Self { kind, message }),
            FAILED
                if message.kind == RESULT
                    && message.result == u64::from(UNAVAILABLE)
                    && message.length == 0
                    && message.save == 0 =>
            {
                Some(Self { kind, message })
            }
            _ => None,
        }
    }
}
