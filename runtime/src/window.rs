//! Public metadata contract for the kernel-owned window-content broker.
//!
//! The legacy names also cover generic SDK content. Codecs/hashes are field-wise
//! little-endian; FNV binds metadata for corruption detection, not authentication.
//! Decode checks length only. Status canonical validation checks format and
//! transition ordering, not caller authority or the current transaction.
//! Request metadata validation and the empty-body UI builder are shared here;
//! admission and constructors for private editor state remain OS policy.
//! Consumers must additionally match the authenticated owner endpoint, window
//! instance, request and kernel-issued publication generation.

pub use osaiku_ui::{input::Event, Identity};
mod receipt;
pub use receipt::*;
mod transaction;
pub use transaction::request_matches_window_state;

pub(crate) fn content_submit_arguments(
    frame: &UiDocumentWindowContentFrame,
    bytes: &[u8],
    status: &mut UiDocumentWindowChannelStatusRecord,
) -> [u64; 5] {
    [
        frame as *const UiDocumentWindowContentFrame as u64,
        bytes.as_ptr() as u64,
        bytes.len() as u64,
        status as *mut UiDocumentWindowChannelStatusRecord as u64,
        0,
    ]
}

pub const UI_DOCUMENT_WINDOW_MAGIC: u32 = 0x434f_4455; // "UDOC" as little-endian bytes.
pub const UI_DOCUMENT_WINDOW_ABI_VERSION: u32 = 10;

pub const UI_DOCUMENT_WINDOW_CHANNEL_STATUS_RECORD_SIZE: usize = 136;

pub const UI_DOCUMENT_WINDOW_CHANNEL_STATE_EMPTY: u8 = 0;
pub const UI_DOCUMENT_WINDOW_CHANNEL_STATE_ARMED: u8 = 1;
pub const UI_DOCUMENT_WINDOW_CHANNEL_STATE_SUBMITTED: u8 = 2;
pub const UI_DOCUMENT_WINDOW_CHANNEL_STATE_TAKEN: u8 = 3;
pub const UI_DOCUMENT_WINDOW_CHANNEL_STATE_COMPLETED: u8 = 4;
pub const UI_DOCUMENT_WINDOW_CHANNEL_STATE_CANCELLED: u8 = 5;
pub const UI_DOCUMENT_WINDOW_CHANNEL_STATE_REJECTED: u8 = 6;

pub const UI_DOCUMENT_WINDOW_CHANNEL_STATUS_OK: u8 = 0;
pub const UI_DOCUMENT_WINDOW_CHANNEL_STATUS_INVALID_REQUEST: u8 = 1;
pub const UI_DOCUMENT_WINDOW_CHANNEL_STATUS_DENIED: u8 = 2;
pub const UI_DOCUMENT_WINDOW_CHANNEL_STATUS_NO_PENDING: u8 = 3;
pub const UI_DOCUMENT_WINDOW_CHANNEL_STATUS_STALE: u8 = 4;
pub const UI_DOCUMENT_WINDOW_CHANNEL_STATUS_DUPLICATE: u8 = 5;
pub const UI_DOCUMENT_WINDOW_CHANNEL_STATUS_WRONG_CALLER: u8 = 6;
pub const UI_DOCUMENT_WINDOW_CHANNEL_STATUS_WRONG_PROFILE: u8 = 7;
pub const UI_DOCUMENT_WINDOW_CHANNEL_STATUS_CAPACITY: u8 = 8;
pub const UI_DOCUMENT_WINDOW_CHANNEL_STATUS_KERNEL_REJECTED: u8 = 9;

pub const UI_DOCUMENT_WINDOW_CHANNEL_REASON_NONE: u8 = 0;
pub const UI_DOCUMENT_WINDOW_CHANNEL_REASON_INVALID_REQUEST: u8 = 1;
pub const UI_DOCUMENT_WINDOW_CHANNEL_REASON_NO_PENDING: u8 = 2;
pub const UI_DOCUMENT_WINDOW_CHANNEL_REASON_STALE_PENDING: u8 = 3;
pub const UI_DOCUMENT_WINDOW_CHANNEL_REASON_DUPLICATE_SUBMISSION: u8 = 4;
pub const UI_DOCUMENT_WINDOW_CHANNEL_REASON_REPLAY: u8 = 5;
pub const UI_DOCUMENT_WINDOW_CHANNEL_REASON_CALLER_NOT_INITD: u8 = 6;
pub const UI_DOCUMENT_WINDOW_CHANNEL_REASON_CALLER_NOT_DOCUMENT_APP: u8 = 7;
pub const UI_DOCUMENT_WINDOW_CHANNEL_REASON_CALLER_NOT_RENDERER: u8 = 8;
pub const UI_DOCUMENT_WINDOW_CHANNEL_REASON_PROFILE_NOT_NORMAL: u8 = 9;
pub const UI_DOCUMENT_WINDOW_CHANNEL_REASON_OUTSTANDING_REQUEST: u8 = 10;
pub const UI_DOCUMENT_WINDOW_CHANNEL_REASON_FRAME_MISMATCH: u8 = 11;
pub const UI_DOCUMENT_WINDOW_CHANNEL_REASON_KERNEL_PRESENTATION_REJECTED: u8 = 12;
pub const UI_DOCUMENT_WINDOW_CHANNEL_REASON_CANCELLED: u8 = 13;

pub const UI_DOCUMENT_WINDOW_PROFILE_NORMAL: u8 = 1;

pub const UI_DOCUMENT_WINDOW_CHANNEL_KERNEL_BROKER: u32 = 1 << 0;
pub const UI_DOCUMENT_WINDOW_CHANNEL_EXACT_PARTICIPANT_PIDS: u32 = 1 << 1;
pub const UI_DOCUMENT_WINDOW_CHANNEL_NORMAL_PROFILE_ONLY: u32 = 1 << 2;
pub const UI_DOCUMENT_WINDOW_CHANNEL_ONE_OUTSTANDING: u32 = 1 << 3;
pub const UI_DOCUMENT_WINDOW_CHANNEL_SINGLE_USE: u32 = 1 << 4;
pub const UI_DOCUMENT_WINDOW_CHANNEL_REPLAY_REJECTED: u32 = 1 << 5;
pub const UI_DOCUMENT_WINDOW_CHANNEL_DUPLICATE_REJECTED: u32 = 1 << 6;
pub const UI_DOCUMENT_WINDOW_CHANNEL_SCRUB_AFTER_TAKE: u32 = 1 << 7;
pub const UI_DOCUMENT_WINDOW_CHANNEL_INITD_ARMS: u32 = 1 << 8;
pub const UI_DOCUMENT_WINDOW_CHANNEL_DOCUMENT_APP_SUBMITS: u32 = 1 << 9;
pub const UI_DOCUMENT_WINDOW_CHANNEL_RENDERER_TAKES: u32 = 1 << 10;
pub const UI_DOCUMENT_WINDOW_CHANNEL_METADATA_ONLY_STATUS: u32 = 1 << 11;
pub const UI_DOCUMENT_WINDOW_CHANNEL_NO_AI_QUERY: u32 = 1 << 12;
pub const UI_DOCUMENT_WINDOW_CHANNEL_NO_GENERIC_IPC: u32 = 1 << 13;
pub const UI_DOCUMENT_WINDOW_CHANNEL_FLAGS_CANONICAL: u32 = UI_DOCUMENT_WINDOW_CHANNEL_KERNEL_BROKER
    | UI_DOCUMENT_WINDOW_CHANNEL_EXACT_PARTICIPANT_PIDS
    | UI_DOCUMENT_WINDOW_CHANNEL_NORMAL_PROFILE_ONLY
    | UI_DOCUMENT_WINDOW_CHANNEL_ONE_OUTSTANDING
    | UI_DOCUMENT_WINDOW_CHANNEL_SINGLE_USE
    | UI_DOCUMENT_WINDOW_CHANNEL_REPLAY_REJECTED
    | UI_DOCUMENT_WINDOW_CHANNEL_DUPLICATE_REJECTED
    | UI_DOCUMENT_WINDOW_CHANNEL_SCRUB_AFTER_TAKE
    | UI_DOCUMENT_WINDOW_CHANNEL_INITD_ARMS
    | UI_DOCUMENT_WINDOW_CHANNEL_DOCUMENT_APP_SUBMITS
    | UI_DOCUMENT_WINDOW_CHANNEL_RENDERER_TAKES
    | UI_DOCUMENT_WINDOW_CHANNEL_METADATA_ONLY_STATUS
    | UI_DOCUMENT_WINDOW_CHANNEL_NO_AI_QUERY
    | UI_DOCUMENT_WINDOW_CHANNEL_NO_GENERIC_IPC;

#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UiDocumentWindowChannelStatusRecord {
    pub magic: u32,
    pub abi_version: u32,
    pub channel_state: u8,
    pub status: u8,
    pub reason: u8,
    pub profile: u8,
    pub channel_flags: u32,
    pub broker_generation: u64,
    pub request_id: u64,
    pub request_hash: u64,
    pub arm_generation: u64,
    pub submit_generation: u64,
    pub take_generation: u64,
    pub complete_generation: u64,
    pub initd_process_id: u64,
    pub document_app_process_id: u64,
    /// The whole address of the instance this channel belongs to.
    ///
    /// The identity beside it is reused; this is not. A record that named only
    /// the identity stayed canonical for whichever instance took the number
    /// next, so a reader could accept a closed instance's status as its
    /// successor's.
    pub document_app_process_endpoint: u64,
    /// Which launch of that instance, the other half of the broker's key.
    pub document_app_instance_id: u64,
    /// The publication generation the armed transaction will produce.
    ///
    /// Issued by the kernel, which holds the window's counter, rather than
    /// predicted by the two services that consume it. They predicted the same
    /// value from the same rule and compared -- which agreed until a refused
    /// presentation moved one side's count and not the other's.
    ///
    /// Zero exactly when there is no transaction: present whenever an arm
    /// generation is, absent with it.
    pub next_public_generation: u64,
    pub renderer_process_id: u64,
    pub transition_provenance_hash: u64,
    pub record_hash: u64,
}

impl UiDocumentWindowChannelStatusRecord {
    pub const EMPTY: Self = Self {
        magic: 0,
        abi_version: 0,
        channel_state: UI_DOCUMENT_WINDOW_CHANNEL_STATE_EMPTY,
        status: UI_DOCUMENT_WINDOW_CHANNEL_STATUS_OK,
        reason: UI_DOCUMENT_WINDOW_CHANNEL_REASON_NONE,
        profile: 0,
        channel_flags: 0,
        broker_generation: 0,
        request_id: 0,
        request_hash: 0,
        arm_generation: 0,
        submit_generation: 0,
        take_generation: 0,
        complete_generation: 0,
        initd_process_id: 0,
        document_app_process_id: 0,
        document_app_process_endpoint: 0,
        document_app_instance_id: 0,
        next_public_generation: 0,
        renderer_process_id: 0,
        transition_provenance_hash: 0,
        record_hash: 0,
    };

    pub fn encode(&self) -> [u8; UI_DOCUMENT_WINDOW_CHANNEL_STATUS_RECORD_SIZE] {
        let mut out = [0u8; UI_DOCUMENT_WINDOW_CHANNEL_STATUS_RECORD_SIZE];
        put_u32(&mut out, 0, self.magic);
        put_u32(&mut out, 4, self.abi_version);
        out[8] = self.channel_state;
        out[9] = self.status;
        out[10] = self.reason;
        out[11] = self.profile;
        put_u32(&mut out, 12, self.channel_flags);
        let fields = [
            self.broker_generation,
            self.request_id,
            self.request_hash,
            self.arm_generation,
            self.submit_generation,
            self.take_generation,
            self.complete_generation,
            self.initd_process_id,
            self.document_app_process_id,
            self.document_app_process_endpoint,
            self.document_app_instance_id,
            self.next_public_generation,
            self.renderer_process_id,
            self.transition_provenance_hash,
            self.record_hash,
        ];
        for (index, value) in fields.into_iter().enumerate() {
            put_u64(&mut out, 16 + index * 8, value);
        }
        out
    }

    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != UI_DOCUMENT_WINDOW_CHANNEL_STATUS_RECORD_SIZE {
            return None;
        }
        Some(Self {
            magic: get_u32(bytes, 0),
            abi_version: get_u32(bytes, 4),
            channel_state: bytes[8],
            status: bytes[9],
            reason: bytes[10],
            profile: bytes[11],
            channel_flags: get_u32(bytes, 12),
            broker_generation: get_u64(bytes, 16),
            request_id: get_u64(bytes, 24),
            request_hash: get_u64(bytes, 32),
            arm_generation: get_u64(bytes, 40),
            submit_generation: get_u64(bytes, 48),
            take_generation: get_u64(bytes, 56),
            complete_generation: get_u64(bytes, 64),
            initd_process_id: get_u64(bytes, 72),
            document_app_process_id: get_u64(bytes, 80),
            document_app_process_endpoint: get_u64(bytes, 88),
            document_app_instance_id: get_u64(bytes, 96),
            next_public_generation: get_u64(bytes, 104),
            renderer_process_id: get_u64(bytes, 112),
            transition_provenance_hash: get_u64(bytes, 120),
            record_hash: get_u64(bytes, 128),
        })
    }
}

pub fn compute_ui_document_window_channel_transition_provenance_hash(
    record: &UiDocumentWindowChannelStatusRecord,
) -> u64 {
    let mut hash = domain_hash(b"osaiku.ui-document-window.renderer-broker-transition.v3");
    hash = fold_u32(hash, record.magic);
    hash = fold_u32(hash, record.abi_version);
    hash = fold_u8(hash, record.channel_state);
    hash = fold_u8(hash, record.status);
    hash = fold_u8(hash, record.reason);
    hash = fold_u8(hash, record.profile);
    hash = fold_u32(hash, record.channel_flags);
    hash = fold_u64(hash, record.broker_generation);
    hash = fold_u64(hash, record.request_id);
    hash = fold_u64(hash, record.request_hash);
    hash = fold_u64(hash, record.arm_generation);
    hash = fold_u64(hash, record.submit_generation);
    hash = fold_u64(hash, record.take_generation);
    hash = fold_u64(hash, record.complete_generation);
    hash = fold_u64(hash, record.initd_process_id);
    hash = fold_u64(hash, record.document_app_process_id);
    hash = fold_u64(hash, record.document_app_process_endpoint);
    hash = fold_u64(hash, record.document_app_instance_id);
    hash = fold_u64(hash, record.next_public_generation);
    nonzero_hash(fold_u64(hash, record.renderer_process_id))
}

pub fn compute_ui_document_window_channel_status_record_hash(
    record: &UiDocumentWindowChannelStatusRecord,
) -> u64 {
    let hash = domain_hash(b"osaiku.ui-document-window.renderer-broker-status.v3");
    let hash = fold_bytes(hash, &record.encode()[..120]);
    nonzero_hash(fold_u64(hash, record.transition_provenance_hash))
}

pub fn ui_document_window_channel_status_record_canonical(
    record: &UiDocumentWindowChannelStatusRecord,
) -> bool {
    if record.magic != UI_DOCUMENT_WINDOW_MAGIC
        || record.abi_version != UI_DOCUMENT_WINDOW_ABI_VERSION
        || record.profile != UI_DOCUMENT_WINDOW_PROFILE_NORMAL
        || record.channel_flags != UI_DOCUMENT_WINDOW_CHANNEL_FLAGS_CANONICAL
        || record.broker_generation == 0
        || record.initd_process_id == 0
        || record.document_app_process_id == 0
        // A record describes one channel, and a channel belongs to one
        // instance. Zero here would be a status with nobody's name on it.
        || record.document_app_process_endpoint == 0
        || record.document_app_instance_id == 0
        // A transaction has a publication generation and nothing else does.
        || (record.next_public_generation != 0) != (record.arm_generation != 0)
        || record.renderer_process_id == 0
        || !channel_status_reason_canonical(record.status, record.reason)
        || record.transition_provenance_hash
            != compute_ui_document_window_channel_transition_provenance_hash(record)
        || record.record_hash == 0
        || record.record_hash != compute_ui_document_window_channel_status_record_hash(record)
    {
        return false;
    }

    match record.channel_state {
        UI_DOCUMENT_WINDOW_CHANNEL_STATE_EMPTY => {
            record.status == UI_DOCUMENT_WINDOW_CHANNEL_STATUS_OK
                && record.reason == UI_DOCUMENT_WINDOW_CHANNEL_REASON_NONE
                && channel_request_fields_zero(record)
        }
        UI_DOCUMENT_WINDOW_CHANNEL_STATE_ARMED => {
            channel_request_fields_present(record)
                && record.status == UI_DOCUMENT_WINDOW_CHANNEL_STATUS_OK
                && record.reason == UI_DOCUMENT_WINDOW_CHANNEL_REASON_NONE
                && record.arm_generation != 0
                && record.submit_generation == 0
                && record.take_generation == 0
                && record.complete_generation == 0
        }
        UI_DOCUMENT_WINDOW_CHANNEL_STATE_SUBMITTED => {
            channel_request_fields_present(record)
                && record.status == UI_DOCUMENT_WINDOW_CHANNEL_STATUS_OK
                && record.reason == UI_DOCUMENT_WINDOW_CHANNEL_REASON_NONE
                && record.arm_generation != 0
                && record.submit_generation > record.arm_generation
                && record.take_generation == 0
                && record.complete_generation == 0
        }
        UI_DOCUMENT_WINDOW_CHANNEL_STATE_TAKEN => {
            channel_request_fields_present(record)
                && record.status == UI_DOCUMENT_WINDOW_CHANNEL_STATUS_OK
                && record.reason == UI_DOCUMENT_WINDOW_CHANNEL_REASON_NONE
                && record.arm_generation != 0
                && record.submit_generation > record.arm_generation
                && record.take_generation > record.submit_generation
                && record.complete_generation == 0
        }
        UI_DOCUMENT_WINDOW_CHANNEL_STATE_COMPLETED => {
            channel_request_fields_present(record)
                && record.status == UI_DOCUMENT_WINDOW_CHANNEL_STATUS_OK
                && record.reason == UI_DOCUMENT_WINDOW_CHANNEL_REASON_NONE
                && record.arm_generation != 0
                && record.submit_generation > record.arm_generation
                && record.take_generation > record.submit_generation
                && record.complete_generation > record.take_generation
        }
        UI_DOCUMENT_WINDOW_CHANNEL_STATE_CANCELLED | UI_DOCUMENT_WINDOW_CHANNEL_STATE_REJECTED => {
            record.status != UI_DOCUMENT_WINDOW_CHANNEL_STATUS_OK
                && record.reason != UI_DOCUMENT_WINDOW_CHANNEL_REASON_NONE
                && terminal_channel_generations_canonical(record)
        }
        _ => false,
    }
}

fn channel_status_reason_canonical(status: u8, reason: u8) -> bool {
    match status {
        UI_DOCUMENT_WINDOW_CHANNEL_STATUS_OK => reason == UI_DOCUMENT_WINDOW_CHANNEL_REASON_NONE,
        UI_DOCUMENT_WINDOW_CHANNEL_STATUS_INVALID_REQUEST => {
            reason == UI_DOCUMENT_WINDOW_CHANNEL_REASON_INVALID_REQUEST
                || reason == UI_DOCUMENT_WINDOW_CHANNEL_REASON_FRAME_MISMATCH
        }
        UI_DOCUMENT_WINDOW_CHANNEL_STATUS_DENIED => matches!(
            reason,
            UI_DOCUMENT_WINDOW_CHANNEL_REASON_CALLER_NOT_INITD
                | UI_DOCUMENT_WINDOW_CHANNEL_REASON_CALLER_NOT_DOCUMENT_APP
                | UI_DOCUMENT_WINDOW_CHANNEL_REASON_CALLER_NOT_RENDERER
                | UI_DOCUMENT_WINDOW_CHANNEL_REASON_CANCELLED
        ),
        UI_DOCUMENT_WINDOW_CHANNEL_STATUS_NO_PENDING => {
            reason == UI_DOCUMENT_WINDOW_CHANNEL_REASON_NO_PENDING
        }
        UI_DOCUMENT_WINDOW_CHANNEL_STATUS_STALE => matches!(
            reason,
            UI_DOCUMENT_WINDOW_CHANNEL_REASON_STALE_PENDING
                | UI_DOCUMENT_WINDOW_CHANNEL_REASON_REPLAY
                | UI_DOCUMENT_WINDOW_CHANNEL_REASON_FRAME_MISMATCH
        ),
        UI_DOCUMENT_WINDOW_CHANNEL_STATUS_DUPLICATE => {
            reason == UI_DOCUMENT_WINDOW_CHANNEL_REASON_DUPLICATE_SUBMISSION
                || reason == UI_DOCUMENT_WINDOW_CHANNEL_REASON_REPLAY
        }
        UI_DOCUMENT_WINDOW_CHANNEL_STATUS_WRONG_CALLER => matches!(
            reason,
            UI_DOCUMENT_WINDOW_CHANNEL_REASON_CALLER_NOT_INITD
                | UI_DOCUMENT_WINDOW_CHANNEL_REASON_CALLER_NOT_DOCUMENT_APP
                | UI_DOCUMENT_WINDOW_CHANNEL_REASON_CALLER_NOT_RENDERER
        ),
        UI_DOCUMENT_WINDOW_CHANNEL_STATUS_WRONG_PROFILE => {
            reason == UI_DOCUMENT_WINDOW_CHANNEL_REASON_PROFILE_NOT_NORMAL
        }
        UI_DOCUMENT_WINDOW_CHANNEL_STATUS_CAPACITY => {
            reason == UI_DOCUMENT_WINDOW_CHANNEL_REASON_OUTSTANDING_REQUEST
        }
        UI_DOCUMENT_WINDOW_CHANNEL_STATUS_KERNEL_REJECTED => {
            reason == UI_DOCUMENT_WINDOW_CHANNEL_REASON_KERNEL_PRESENTATION_REJECTED
        }
        _ => false,
    }
}

fn channel_request_fields_zero(record: &UiDocumentWindowChannelStatusRecord) -> bool {
    record.request_id == 0
        && record.request_hash == 0
        && record.arm_generation == 0
        && record.submit_generation == 0
        && record.take_generation == 0
        && record.complete_generation == 0
}

fn channel_request_fields_present(record: &UiDocumentWindowChannelStatusRecord) -> bool {
    record.request_id != 0 && record.request_hash != 0
}

fn terminal_channel_generations_canonical(record: &UiDocumentWindowChannelStatusRecord) -> bool {
    if record.request_id == 0 || record.request_hash == 0 {
        return record.arm_generation == 0
            && record.submit_generation == 0
            && record.take_generation == 0
            && record.complete_generation == 0;
    }
    if record.arm_generation == 0 {
        return record.submit_generation == 0
            && record.take_generation == 0
            && record.complete_generation == 0;
    }
    if record.submit_generation == 0 {
        return record.take_generation == 0 && record.complete_generation >= record.arm_generation;
    }
    if record.submit_generation <= record.arm_generation {
        return false;
    }
    if record.take_generation == 0 {
        return record.complete_generation >= record.submit_generation;
    }
    record.take_generation > record.submit_generation
        && (record.complete_generation == 0 || record.complete_generation >= record.take_generation)
}

fn domain_hash(domain: &[u8]) -> u64 {
    fold_bytes(0xcbf2_9ce4_8422_2325, domain)
}

fn nonzero_hash(hash: u64) -> u64 {
    if hash == 0 {
        1
    } else {
        hash
    }
}

fn fold_u8(hash: u64, value: u8) -> u64 {
    fold_bytes(hash, &[value])
}

fn fold_u32(hash: u64, value: u32) -> u64 {
    fold_bytes(hash, &value.to_le_bytes())
}

fn fold_u64(hash: u64, value: u64) -> u64 {
    fold_bytes(hash, &value.to_le_bytes())
}

fn fold_bytes(mut hash: u64, bytes: &[u8]) -> u64 {
    let mut index = 0usize;
    while index < bytes.len() {
        hash ^= bytes[index] as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        index += 1;
    }
    hash
}

fn put_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn put_u64(bytes: &mut [u8], offset: usize, value: u64) {
    bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

fn get_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ])
}

fn get_u64(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
        bytes[offset + 4],
        bytes[offset + 5],
        bytes[offset + 6],
        bytes[offset + 7],
    ])
}

pub const UI_DOCUMENT_WINDOW_PRESENT_REQUEST_SIZE: usize = 160;
pub const UI_DOCUMENT_WINDOW_CONTENT_FRAME_SIZE: usize = 168;
/// Inline body slot retained for ABI compatibility with legacy Document content.
pub const UI_DOCUMENT_WINDOW_BODY_CAPACITY: usize = 32;

pub const UI_DOCUMENT_WINDOW_REQUEST_STATE_EMPTY: u8 = 0;
pub const UI_DOCUMENT_WINDOW_REQUEST_STATE_PREPARED: u8 = 1;
pub const UI_DOCUMENT_WINDOW_REQUEST_KIND_PRESENT: u8 = 1;

pub const UI_DOCUMENT_WINDOW_REQUEST_EXACT_BINDING: u16 = 1 << 0;
pub const UI_DOCUMENT_WINDOW_REQUEST_EXACT_EDITOR_STATE: u16 = 1 << 1;
pub const UI_DOCUMENT_WINDOW_REQUEST_ARM_ONE_SHOT: u16 = 1 << 2;
pub const UI_DOCUMENT_WINDOW_REQUEST_FOR_RENDERER_BROKER: u16 = 1 << 3;
pub const UI_DOCUMENT_WINDOW_REQUEST_FOR_DOCUMENT_APP: u16 = 1 << 4;
pub const UI_DOCUMENT_WINDOW_REQUEST_METADATA_ONLY: u16 = 1 << 5;
pub const UI_DOCUMENT_WINDOW_REQUEST_EXPECTED_FRAME_BOUND: u16 = 1 << 6;
pub const UI_DOCUMENT_WINDOW_REQUEST_FLAGS_CANONICAL: u16 = UI_DOCUMENT_WINDOW_REQUEST_EXACT_BINDING
    | UI_DOCUMENT_WINDOW_REQUEST_EXACT_EDITOR_STATE
    | UI_DOCUMENT_WINDOW_REQUEST_ARM_ONE_SHOT
    | UI_DOCUMENT_WINDOW_REQUEST_FOR_RENDERER_BROKER
    | UI_DOCUMENT_WINDOW_REQUEST_FOR_DOCUMENT_APP
    | UI_DOCUMENT_WINDOW_REQUEST_METADATA_ONLY
    | UI_DOCUMENT_WINDOW_REQUEST_EXPECTED_FRAME_BOUND;

pub const UI_DOCUMENT_WINDOW_FRAME_STATE_EMPTY: u8 = 0;
pub const UI_DOCUMENT_WINDOW_FRAME_STATE_READY: u8 = 1;

pub const UI_DOCUMENT_WINDOW_BROKER_APP_OWNED_CONTENT: u8 = 1 << 0;
pub const UI_DOCUMENT_WINDOW_BROKER_DEDICATED_ONE_WAY: u8 = 1 << 1;
pub const UI_DOCUMENT_WINDOW_BROKER_INITD_ARMED: u8 = 1 << 2;
pub const UI_DOCUMENT_WINDOW_BROKER_SINGLE_USE: u8 = 1 << 3;
pub const UI_DOCUMENT_WINDOW_BROKER_NORMAL_PROFILE_ONLY: u8 = 1 << 4;
pub const UI_DOCUMENT_WINDOW_BROKER_SCRUB_AFTER_TAKE: u8 = 1 << 5;
pub const UI_DOCUMENT_WINDOW_BROKER_VOLATILE_CURRENT_BOOT_SAVE_HONEST: u8 = 1 << 6;
pub const UI_DOCUMENT_WINDOW_BROKER_DEVELOPMENT_DURABLE_SAVE: u8 = 1 << 7;
pub const UI_DOCUMENT_WINDOW_BROKER_FLAGS_CANONICAL: u8 =
    UI_DOCUMENT_WINDOW_BROKER_APP_OWNED_CONTENT
        | UI_DOCUMENT_WINDOW_BROKER_DEDICATED_ONE_WAY
        | UI_DOCUMENT_WINDOW_BROKER_INITD_ARMED
        | UI_DOCUMENT_WINDOW_BROKER_SINGLE_USE
        | UI_DOCUMENT_WINDOW_BROKER_NORMAL_PROFILE_ONLY
        | UI_DOCUMENT_WINDOW_BROKER_SCRUB_AFTER_TAKE
        | UI_DOCUMENT_WINDOW_BROKER_VOLATILE_CURRENT_BOOT_SAVE_HONEST;

pub const UI_DOCUMENT_WINDOW_PRIVACY_RENDERER_DISPLAY_USE_ONLY: u8 = 1 << 0;
pub const UI_DOCUMENT_WINDOW_PRIVACY_NO_INITD_BODY_PROJECTION: u8 = 1 << 1;
pub const UI_DOCUMENT_WINDOW_PRIVACY_NO_AI_OBSERVATION: u8 = 1 << 2;
pub const UI_DOCUMENT_WINDOW_PRIVACY_NO_FRAMEBUFFER_READBACK: u8 = 1 << 3;
pub const UI_DOCUMENT_WINDOW_PRIVACY_ZERO_TAILED_BODY: u8 = 1 << 4;
pub const UI_DOCUMENT_WINDOW_PRIVACY_NO_TITLE_PREVIEW_PAYLOAD: u8 = 1 << 5;
pub const UI_DOCUMENT_WINDOW_PRIVACY_FLAGS_CANONICAL: u8 =
    UI_DOCUMENT_WINDOW_PRIVACY_RENDERER_DISPLAY_USE_ONLY
        | UI_DOCUMENT_WINDOW_PRIVACY_NO_INITD_BODY_PROJECTION
        | UI_DOCUMENT_WINDOW_PRIVACY_NO_AI_OBSERVATION
        | UI_DOCUMENT_WINDOW_PRIVACY_NO_FRAMEBUFFER_READBACK
        | UI_DOCUMENT_WINDOW_PRIVACY_ZERO_TAILED_BODY
        | UI_DOCUMENT_WINDOW_PRIVACY_NO_TITLE_PREVIEW_PAYLOAD;

pub const UI_DOCUMENT_WINDOW_AUTHORITY_NO_INPUT: u16 = 1 << 0;
pub const UI_DOCUMENT_WINDOW_AUTHORITY_NO_ACTION: u16 = 1 << 1;
pub const UI_DOCUMENT_WINDOW_AUTHORITY_NO_EDIT: u16 = 1 << 2;
pub const UI_DOCUMENT_WINDOW_AUTHORITY_NO_SAVE: u16 = 1 << 3;
pub const UI_DOCUMENT_WINDOW_AUTHORITY_NO_APP_MUTATION: u16 = 1 << 4;
pub const UI_DOCUMENT_WINDOW_AUTHORITY_NO_POLICY_MUTATION: u16 = 1 << 5;
pub const UI_DOCUMENT_WINDOW_AUTHORITY_NO_STORAGE_MUTATION: u16 = 1 << 6;
pub const UI_DOCUMENT_WINDOW_AUTHORITY_NO_AUDIT_MUTATION: u16 = 1 << 7;
pub const UI_DOCUMENT_WINDOW_AUTHORITY_NO_TICKET_MUTATION: u16 = 1 << 8;
pub const UI_DOCUMENT_WINDOW_AUTHORITY_NO_CAPABILITY_MUTATION: u16 = 1 << 9;
pub const UI_DOCUMENT_WINDOW_AUTHORITY_NO_APPROVAL_MUTATION: u16 = 1 << 10;
pub const UI_DOCUMENT_WINDOW_AUTHORITY_NO_LEASE_MUTATION: u16 = 1 << 11;
pub const UI_DOCUMENT_WINDOW_AUTHORITY_NO_MODEL: u16 = 1 << 12;
pub const UI_DOCUMENT_WINDOW_AUTHORITY_NO_TOOL: u16 = 1 << 13;
pub const UI_DOCUMENT_WINDOW_AUTHORITY_NO_SHELL: u16 = 1 << 14;
pub const UI_DOCUMENT_WINDOW_AUTHORITY_NO_NETWORK: u16 = 1 << 15;
pub const UI_DOCUMENT_WINDOW_AUTHORITY_FLAGS_CANONICAL: u16 = UI_DOCUMENT_WINDOW_AUTHORITY_NO_INPUT
    | UI_DOCUMENT_WINDOW_AUTHORITY_NO_ACTION
    | UI_DOCUMENT_WINDOW_AUTHORITY_NO_EDIT
    | UI_DOCUMENT_WINDOW_AUTHORITY_NO_SAVE
    | UI_DOCUMENT_WINDOW_AUTHORITY_NO_APP_MUTATION
    | UI_DOCUMENT_WINDOW_AUTHORITY_NO_POLICY_MUTATION
    | UI_DOCUMENT_WINDOW_AUTHORITY_NO_STORAGE_MUTATION
    | UI_DOCUMENT_WINDOW_AUTHORITY_NO_AUDIT_MUTATION
    | UI_DOCUMENT_WINDOW_AUTHORITY_NO_TICKET_MUTATION
    | UI_DOCUMENT_WINDOW_AUTHORITY_NO_CAPABILITY_MUTATION
    | UI_DOCUMENT_WINDOW_AUTHORITY_NO_APPROVAL_MUTATION
    | UI_DOCUMENT_WINDOW_AUTHORITY_NO_LEASE_MUTATION
    | UI_DOCUMENT_WINDOW_AUTHORITY_NO_MODEL
    | UI_DOCUMENT_WINDOW_AUTHORITY_NO_TOOL
    | UI_DOCUMENT_WINDOW_AUTHORITY_NO_SHELL
    | UI_DOCUMENT_WINDOW_AUTHORITY_NO_NETWORK;

/// Metadata for one armed presentation. Decoding is not authentication or
/// admission: match the lifecycle sender, window binding and current state.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UiDocumentWindowPresentRequest {
    pub magic: u32,
    pub abi_version: u32,
    pub request_state: u8,
    pub request_kind: u8,
    pub request_flags: u16,
    pub reserved: u32,
    pub request_id: u64,
    pub expected_app_id: u64,
    pub expected_app_instance_id: u64,
    pub expected_app_instance_generation: u64,
    pub expected_window_id: u64,
    pub expected_window_generation: u64,
    pub expected_surface_id: u64,
    pub expected_surface_generation: u64,
    pub expected_binding_generation: u64,
    pub expected_binding_hash: u64,
    pub expected_binding_record_hash: u64,
    pub expected_document_id: u64,
    pub expected_document_revision: u64,
    pub expected_editor_generation: u64,
    pub expected_semantic_state_generation: u64,
    pub expected_body_hash: u64,
    pub expected_editor_state_hash: u64,
    pub request_hash: u64,
}

impl UiDocumentWindowPresentRequest {
    pub const EMPTY: Self = Self {
        magic: 0,
        abi_version: 0,
        request_state: UI_DOCUMENT_WINDOW_REQUEST_STATE_EMPTY,
        request_kind: 0,
        request_flags: 0,
        reserved: 0,
        request_id: 0,
        expected_app_id: 0,
        expected_app_instance_id: 0,
        expected_app_instance_generation: 0,
        expected_window_id: 0,
        expected_window_generation: 0,
        expected_surface_id: 0,
        expected_surface_generation: 0,
        expected_binding_generation: 0,
        expected_binding_hash: 0,
        expected_binding_record_hash: 0,
        expected_document_id: 0,
        expected_document_revision: 0,
        expected_editor_generation: 0,
        expected_semantic_state_generation: 0,
        expected_body_hash: 0,
        expected_editor_state_hash: 0,
        request_hash: 0,
    };

    pub fn encode(&self) -> [u8; UI_DOCUMENT_WINDOW_PRESENT_REQUEST_SIZE] {
        let mut out = [0u8; UI_DOCUMENT_WINDOW_PRESENT_REQUEST_SIZE];
        put_u32(&mut out, 0, self.magic);
        put_u32(&mut out, 4, self.abi_version);
        out[8] = self.request_state;
        out[9] = self.request_kind;
        put_u16(&mut out, 10, self.request_flags);
        put_u32(&mut out, 12, self.reserved);
        let fields = [
            self.request_id,
            self.expected_app_id,
            self.expected_app_instance_id,
            self.expected_app_instance_generation,
            self.expected_window_id,
            self.expected_window_generation,
            self.expected_surface_id,
            self.expected_surface_generation,
            self.expected_binding_generation,
            self.expected_binding_hash,
            self.expected_binding_record_hash,
            self.expected_document_id,
            self.expected_document_revision,
            self.expected_editor_generation,
            self.expected_semantic_state_generation,
            self.expected_body_hash,
            self.expected_editor_state_hash,
            self.request_hash,
        ];
        for (index, value) in fields.into_iter().enumerate() {
            put_u64(&mut out, 16 + index * 8, value);
        }
        out
    }

    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != UI_DOCUMENT_WINDOW_PRESENT_REQUEST_SIZE {
            return None;
        }
        Some(Self {
            magic: get_u32(bytes, 0),
            abi_version: get_u32(bytes, 4),
            request_state: bytes[8],
            request_kind: bytes[9],
            request_flags: get_u16(bytes, 10),
            reserved: get_u32(bytes, 12),
            request_id: get_u64(bytes, 16),
            expected_app_id: get_u64(bytes, 24),
            expected_app_instance_id: get_u64(bytes, 32),
            expected_app_instance_generation: get_u64(bytes, 40),
            expected_window_id: get_u64(bytes, 48),
            expected_window_generation: get_u64(bytes, 56),
            expected_surface_id: get_u64(bytes, 64),
            expected_surface_generation: get_u64(bytes, 72),
            expected_binding_generation: get_u64(bytes, 80),
            expected_binding_hash: get_u64(bytes, 88),
            expected_binding_record_hash: get_u64(bytes, 96),
            expected_document_id: get_u64(bytes, 104),
            expected_document_revision: get_u64(bytes, 112),
            expected_editor_generation: get_u64(bytes, 120),
            expected_semantic_state_generation: get_u64(bytes, 128),
            expected_body_hash: get_u64(bytes, 136),
            expected_editor_state_hash: get_u64(bytes, 144),
            request_hash: get_u64(bytes, 152),
        })
    }
}

/// Pointer-free metadata submitted separately from the bounded UI recipe.
/// A valid hash does not establish authority or prove kernel admission. The
/// inline body is legacy Document content, not a slot for the UI recipe bytes.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UiDocumentWindowContentFrame {
    pub magic: u32,
    pub abi_version: u32,
    pub frame_state: u8,
    pub encoding: u8,
    pub body_len: u8,
    pub content_state: u8,
    pub dirty_state: u8,
    pub save_state: u8,
    pub broker_flags: u8,
    pub privacy_flags: u8,
    pub authority_flags: u16,
    /// 0=Document, 2=File Manager, 3=System Updates, 4=SDK. Kind 1 is retired.
    pub app_view_kind: u8,
    /// Explicit user-requested assistant save-status report; zero hides it.
    pub assistant_report: u8,
    pub reserved_header: [u8; 4],
    pub app_instance_id: u64,
    pub window_id: u64,
    pub surface_id: u64,
    pub document_id: u64,
    pub surface_generation: u64,
    pub binding_hash: u64,
    pub document_revision: u64,
    pub editor_generation: u64,
    pub semantic_state_generation: u64,
    pub editor_state_hash: u64,
    pub body_hash: u64,
    pub source_request_hash: u64,
    pub body: [u8; UI_DOCUMENT_WINDOW_BODY_CAPACITY],
    pub app_view_hash: u64,
    pub record_hash: u64,
}

impl UiDocumentWindowContentFrame {
    /// Build a declarative UI frame with an empty legacy editor body and clean,
    /// never-saved compatibility state. The caller must authenticate the request
    /// and match its current lifecycle/session before using it. Kernel admission
    /// still checks the actual owner, armed transaction and decoded recipe.
    /// This validates request metadata and recipe length, then hashes the bytes;
    /// it does not decode the recipe or allocate a Document on the app stack.
    pub fn for_ui(request: &UiDocumentWindowPresentRequest, bytes: &[u8]) -> Option<Self> {
        if !ui_document_window_present_request_well_formed(request)
            || request.expected_body_hash != compute_window_body_hash(&[])
            || bytes.is_empty()
            || bytes.len() > osaiku_ui::MAX_WIRE
        {
            return None;
        }
        let mut frame = Self {
            magic: UI_DOCUMENT_WINDOW_MAGIC,
            abi_version: UI_DOCUMENT_WINDOW_ABI_VERSION,
            frame_state: UI_DOCUMENT_WINDOW_FRAME_STATE_READY,
            // ABI v10 legacy ASCII encoding, empty/clean/never-saved body.
            encoding: 1,
            broker_flags: UI_DOCUMENT_WINDOW_BROKER_FLAGS_CANONICAL,
            privacy_flags: UI_DOCUMENT_WINDOW_PRIVACY_FLAGS_CANONICAL,
            authority_flags: UI_DOCUMENT_WINDOW_AUTHORITY_FLAGS_CANONICAL,
            app_view_kind: 4,
            app_instance_id: request.expected_app_instance_id,
            window_id: request.expected_window_id,
            surface_id: request.expected_surface_id,
            document_id: request.expected_document_id,
            surface_generation: request.expected_surface_generation,
            binding_hash: request.expected_binding_hash,
            document_revision: request.expected_document_revision,
            editor_generation: request.expected_editor_generation,
            semantic_state_generation: request.expected_semantic_state_generation,
            editor_state_hash: request.expected_editor_state_hash,
            body_hash: request.expected_body_hash,
            source_request_hash: request.request_hash,
            app_view_hash: osaiku_ui::wire_hash(bytes),
            ..Self::EMPTY
        };
        frame.record_hash = compute_ui_document_window_content_frame_record_hash(&frame);
        Some(frame)
    }

    pub const EMPTY: Self = Self {
        magic: 0,
        abi_version: 0,
        frame_state: UI_DOCUMENT_WINDOW_FRAME_STATE_EMPTY,
        encoding: 0,
        body_len: 0,
        content_state: 0,
        dirty_state: 0,
        save_state: 0,
        broker_flags: 0,
        privacy_flags: 0,
        authority_flags: 0,
        app_view_kind: 0,
        assistant_report: 0,
        reserved_header: [0; 4],
        app_instance_id: 0,
        window_id: 0,
        surface_id: 0,
        document_id: 0,
        surface_generation: 0,
        binding_hash: 0,
        document_revision: 0,
        editor_generation: 0,
        semantic_state_generation: 0,
        editor_state_hash: 0,
        body_hash: 0,
        source_request_hash: 0,
        body: [0; UI_DOCUMENT_WINDOW_BODY_CAPACITY],
        app_view_hash: 0,
        record_hash: 0,
    };

    pub fn encode(&self) -> [u8; UI_DOCUMENT_WINDOW_CONTENT_FRAME_SIZE] {
        let mut out = [0u8; UI_DOCUMENT_WINDOW_CONTENT_FRAME_SIZE];
        put_u32(&mut out, 0, self.magic);
        put_u32(&mut out, 4, self.abi_version);
        out[8] = self.frame_state;
        out[9] = self.encoding;
        out[10] = self.body_len;
        out[11] = self.content_state;
        out[12] = self.dirty_state;
        out[13] = self.save_state;
        out[14] = self.broker_flags;
        out[15] = self.privacy_flags;
        put_u16(&mut out, 16, self.authority_flags);
        out[18] = self.app_view_kind;
        out[19] = self.assistant_report;
        out[20..24].copy_from_slice(&self.reserved_header);
        let fields = [
            self.app_instance_id,
            self.window_id,
            self.surface_id,
            self.document_id,
            self.surface_generation,
            self.binding_hash,
            self.document_revision,
            self.editor_generation,
            self.semantic_state_generation,
            self.editor_state_hash,
            self.body_hash,
            self.source_request_hash,
        ];
        for (index, value) in fields.into_iter().enumerate() {
            put_u64(&mut out, 24 + index * 8, value);
        }
        out[120..152].copy_from_slice(&self.body);
        put_u64(&mut out, 152, self.app_view_hash);
        put_u64(&mut out, 160, self.record_hash);
        out
    }

    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != UI_DOCUMENT_WINDOW_CONTENT_FRAME_SIZE {
            return None;
        }
        let mut reserved_header = [0u8; 4];
        reserved_header.copy_from_slice(&bytes[20..24]);
        let mut body = [0u8; UI_DOCUMENT_WINDOW_BODY_CAPACITY];
        body.copy_from_slice(&bytes[120..152]);
        Some(Self {
            magic: get_u32(bytes, 0),
            abi_version: get_u32(bytes, 4),
            frame_state: bytes[8],
            encoding: bytes[9],
            body_len: bytes[10],
            content_state: bytes[11],
            dirty_state: bytes[12],
            save_state: bytes[13],
            broker_flags: bytes[14],
            privacy_flags: bytes[15],
            authority_flags: get_u16(bytes, 16),
            app_view_kind: bytes[18],
            assistant_report: bytes[19],
            reserved_header,
            app_instance_id: get_u64(bytes, 24),
            window_id: get_u64(bytes, 32),
            surface_id: get_u64(bytes, 40),
            document_id: get_u64(bytes, 48),
            surface_generation: get_u64(bytes, 56),
            binding_hash: get_u64(bytes, 64),
            document_revision: get_u64(bytes, 72),
            editor_generation: get_u64(bytes, 80),
            semantic_state_generation: get_u64(bytes, 88),
            editor_state_hash: get_u64(bytes, 96),
            body_hash: get_u64(bytes, 104),
            source_request_hash: get_u64(bytes, 112),
            body,
            app_view_hash: get_u64(bytes, 152),
            record_hash: get_u64(bytes, 160),
        })
    }
}

pub fn compute_ui_document_window_present_request_hash(
    request: &UiDocumentWindowPresentRequest,
) -> u64 {
    let mut hash = domain_hash(b"osaiku.ui-document-window.present-request.v1");
    hash = fold_u32(hash, request.magic);
    hash = fold_u32(hash, request.abi_version);
    hash = fold_u8(hash, request.request_state);
    hash = fold_u8(hash, request.request_kind);
    hash = fold_u16(hash, request.request_flags);
    hash = fold_u32(hash, request.reserved);
    hash = fold_u64(hash, request.request_id);
    hash = fold_u64(hash, request.expected_app_id);
    hash = fold_u64(hash, request.expected_app_instance_id);
    hash = fold_u64(hash, request.expected_app_instance_generation);
    hash = fold_u64(hash, request.expected_window_id);
    hash = fold_u64(hash, request.expected_window_generation);
    hash = fold_u64(hash, request.expected_surface_id);
    hash = fold_u64(hash, request.expected_surface_generation);
    hash = fold_u64(hash, request.expected_binding_generation);
    hash = fold_u64(hash, request.expected_binding_hash);
    hash = fold_u64(hash, request.expected_binding_record_hash);
    hash = fold_u64(hash, request.expected_document_id);
    hash = fold_u64(hash, request.expected_document_revision);
    hash = fold_u64(hash, request.expected_editor_generation);
    hash = fold_u64(hash, request.expected_semantic_state_generation);
    hash = fold_u64(hash, request.expected_body_hash);
    nonzero_hash(fold_u64(hash, request.expected_editor_state_hash))
}

pub fn compute_ui_document_window_content_frame_record_hash(
    frame: &UiDocumentWindowContentFrame,
) -> u64 {
    let mut hash = domain_hash(b"osaiku.ui-document-window.content-frame.v1");
    hash = fold_u32(hash, frame.magic);
    hash = fold_u32(hash, frame.abi_version);
    hash = fold_u8(hash, frame.frame_state);
    hash = fold_u8(hash, frame.encoding);
    hash = fold_u8(hash, frame.body_len);
    hash = fold_u8(hash, frame.content_state);
    hash = fold_u8(hash, frame.dirty_state);
    hash = fold_u8(hash, frame.save_state);
    hash = fold_u8(hash, frame.broker_flags);
    hash = fold_u8(hash, frame.privacy_flags);
    hash = fold_u16(hash, frame.authority_flags);
    hash = fold_bytes(hash, &[frame.app_view_kind, frame.assistant_report]);
    hash = fold_bytes(hash, &frame.reserved_header);
    hash = fold_u64(hash, frame.app_instance_id);
    hash = fold_u64(hash, frame.window_id);
    hash = fold_u64(hash, frame.surface_id);
    hash = fold_u64(hash, frame.document_id);
    hash = fold_u64(hash, frame.surface_generation);
    hash = fold_u64(hash, frame.binding_hash);
    hash = fold_u64(hash, frame.document_revision);
    hash = fold_u64(hash, frame.editor_generation);
    hash = fold_u64(hash, frame.semantic_state_generation);
    hash = fold_u64(hash, frame.editor_state_hash);
    hash = fold_u64(hash, frame.body_hash);
    hash = fold_u64(hash, frame.source_request_hash);
    hash = fold_u64(hash, frame.app_view_hash);
    nonzero_hash(fold_bytes(hash, &frame.body))
}

/// An authenticated application may decline preparation of an armed update.
/// This is not a presentation receipt. Initd cancels that request and retains
/// the previous surface; an initial opening without a surface can still fail.
pub fn ui_content_declined(request: &UiDocumentWindowPresentRequest) -> [u8; 24] {
    let mut bytes = [0; 24];
    bytes[..8].copy_from_slice(b"OSUINO01");
    bytes[8..16].copy_from_slice(&request.request_id.to_le_bytes());
    bytes[16..24].copy_from_slice(&request.request_hash.to_le_bytes());
    bytes
}

/// Correlated, scalar-only preparation detail. No text or authority is carried.
pub fn ui_content_declined_with_code(
    request: &UiDocumentWindowPresentRequest,
    code: i64,
) -> [u8; 32] {
    let mut bytes = [0; 32];
    bytes[..24].copy_from_slice(&ui_content_declined(request));
    bytes[..8].copy_from_slice(b"OSUINO02");
    bytes[24..].copy_from_slice(&code.to_le_bytes());
    bytes
}

fn put_u16(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}
fn get_u16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
}
fn fold_u16(hash: u64, value: u16) -> u64 {
    fold_bytes(hash, &value.to_le_bytes())
}

#[cfg(test)]
mod submission_tests {
    extern crate std;
    use super::*;

    #[test]
    fn content_submission_preserves_all_inputs_and_does_not_truncate() {
        let frame = UiDocumentWindowContentFrame::EMPTY;
        let mut status = UiDocumentWindowChannelStatusRecord::EMPTY;
        for len in [0, 1, 160, osaiku_ui::MAX_WIRE, osaiku_ui::MAX_WIRE + 1] {
            let bytes = std::vec![0xa5; len];
            let status_ptr = &mut status as *mut UiDocumentWindowChannelStatusRecord as u64;
            assert_eq!(
                content_submit_arguments(&frame, &bytes, &mut status),
                [
                    &frame as *const UiDocumentWindowContentFrame as u64,
                    bytes.as_ptr() as u64,
                    len as u64,
                    status_ptr,
                    0,
                ],
            );
            assert_eq!(status, UiDocumentWindowChannelStatusRecord::EMPTY);
            assert!(bytes.iter().all(|b| *b == 0xa5));
        }
    }
}

/// Legacy body digest used by the window protocol; not authentication.
pub fn compute_window_body_hash(body: &[u8]) -> u64 {
    let mut hash = domain_hash(b"osaiku.document-editor.body.v1");
    hash = fold_u64(hash, body.len() as u64);
    nonzero_hash(fold_bytes(hash, body))
}

/// Derive the document identity used by the existing window binding protocol.
pub fn compute_window_document_id(
    app_instance_id: u64,
    app_instance_generation: u64,
    surface_id: u64,
    surface_generation: u64,
    binding_generation: u64,
    binding_hash: u64,
) -> u64 {
    let mut hash = domain_hash(b"osaiku.document-editor.document-id.v1");
    hash = fold_u64(hash, app_instance_id);
    hash = fold_u64(hash, app_instance_generation);
    hash = fold_u64(hash, surface_id);
    hash = fold_u64(hash, surface_generation);
    hash = fold_u64(hash, binding_generation);
    nonzero_hash(fold_u64(hash, binding_hash))
}

/// Validate wire metadata only. Does not authenticate a sender, admit an app,
/// or establish that this request names a current armed window/session.
pub fn ui_document_window_present_request_well_formed(
    request: &UiDocumentWindowPresentRequest,
) -> bool {
    request.magic == UI_DOCUMENT_WINDOW_MAGIC
        && request.abi_version == UI_DOCUMENT_WINDOW_ABI_VERSION
        && request.request_state == UI_DOCUMENT_WINDOW_REQUEST_STATE_PREPARED
        && request.request_kind == UI_DOCUMENT_WINDOW_REQUEST_KIND_PRESENT
        && request.request_flags == UI_DOCUMENT_WINDOW_REQUEST_FLAGS_CANONICAL
        && request.reserved == 0
        && request.request_id != 0
        && request.expected_app_id != 0
        && request.expected_app_instance_id != 0
        && request.expected_app_instance_generation != 0
        && request.expected_window_id != 0
        && request.expected_window_generation != 0
        && request.expected_surface_id != 0
        && request.expected_surface_generation != 0
        && request.expected_binding_generation != 0
        && request.expected_binding_hash != 0
        && request.expected_binding_record_hash != 0
        && request.expected_document_id
            == compute_window_document_id(
                request.expected_app_instance_id,
                request.expected_app_instance_generation,
                request.expected_surface_id,
                request.expected_surface_generation,
                request.expected_binding_generation,
                request.expected_binding_hash,
            )
        && request.expected_document_revision != 0
        && request.expected_editor_generation != 0
        && request.expected_semantic_state_generation == request.expected_editor_generation
        && request.expected_body_hash != 0
        && request.expected_editor_state_hash != 0
        && request.request_hash != 0
        && request.request_hash == compute_ui_document_window_present_request_hash(request)
}
