//! Endpoint-aware IPC. Raw operations make one attempt; bounded backpressure
//! is opt-in. A message is data, not authority to execute a command.
use crate::{
    abi::{number, SYSCALL_OK},
    process::ProcessEndpoint,
};

/// The failed phase of a single reply/handoff exchange, preserving native status.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReplyError {
    /// No successful send; the handoff was not attempted.
    Send(isize),
    /// The message was already sent. Do not resend it to retry the handoff.
    Handoff(isize),
}

/// Retry only MAILBOX_FULL, which guarantees that the message was not delivered.
/// The fixed two-second deadline includes checkpoints. A fast yield is not a
/// scheduling guarantee, so an attempt count alone must not reject a busy peer.
/// Success is final. Clock failure/backwards time and a stalled-clock fuse stop
/// retries; no path may resend an accepted message.
pub fn send_with_backpressure(
    send: impl FnMut() -> isize,
    checkpoint: impl FnMut() -> isize,
    clock: impl FnMut() -> Result<u64, isize>,
) -> isize {
    send_with_backpressure_report(send, checkpoint, clock).status
}

/// Failure provenance without message contents. Elapsed time is the last
/// successful observation, not time spent inside a failed syscall.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SendReport {
    pub status: isize,
    pub last_send: isize,
    pub stage: &'static str,
    pub attempts: u32,
    pub elapsed_ms: Option<u64>,
}

impl SendReport {
    /// Two bounded metadata-only lines; hex statuses preserve native bit patterns.
    pub fn diagnostic(
        &self,
        out: &mut impl core::fmt::Write,
        part: usize,
        peer: u64,
        kind: u64,
        id: u64,
    ) -> core::fmt::Result {
        if part == 0 {
            write!(out,"ipc-failure:v1;p={peer:016x};k={kind:x};id={id:016x};stage={};s={:016x};send={:016x}",self.stage,self.status as u64,self.last_send as u64)
        } else {
            write!(
                out,
                "ipc-attempts:v1;p={peer:016x};id={id:016x};n={:x};ms={:016x};valid={}",
                self.attempts,
                self.elapsed_ms.unwrap_or(0),
                u8::from(self.elapsed_ms.is_some())
            )
        }
    }
}

pub fn send_with_backpressure_report(
    mut send: impl FnMut() -> isize,
    mut checkpoint: impl FnMut() -> isize,
    mut clock: impl FnMut() -> Result<u64, isize>,
) -> SendReport {
    let first = send();
    let mut report = SendReport {
        status: first,
        last_send: first,
        stage: "send",
        attempts: 1,
        elapsed_ms: None,
    };
    macro_rules! finish {
        ($status:expr, $stage:expr) => {{
            report.status = $status;
            report.stage = $stage;
            return report;
        }};
    }
    if first != crate::abi::SYSCALL_ERR_MAILBOX_FULL {
        return report;
    }
    let start = match clock() {
        Ok(now) => now,
        Err(code) => finish!(code, "initial-clock"),
    };
    report.elapsed_ms = Some(0);
    let mut previous = start;
    for _ in 0..65_536 {
        let yielded = checkpoint();
        if yielded != SYSCALL_OK {
            finish!(yielded, "checkpoint");
        }
        let now = match clock() {
            Ok(now) if now >= previous => now,
            Ok(_) => finish!(crate::abi::SYSCALL_ERR_INVALID_STATE, "backwards-clock"),
            Err(code) => finish!(code, "clock"),
        };
        report.elapsed_ms = Some(now - start);
        if now - start >= 2_000 {
            finish!(crate::abi::SYSCALL_ERR_TIMEOUT, "deadline");
        }
        previous = now;
        let result = send();
        report.attempts += 1;
        report.last_send = result;
        if result != crate::abi::SYSCALL_ERR_MAILBOX_FULL {
            finish!(result, "send");
        }
    }
    finish!(crate::abi::SYSCALL_ERR_TIMEOUT, "attempt-limit");
}

/// Send once, then hand off only after a successful send. Never retry either
/// phase. Adapters supply their own lifecycle-aware handoff; its checkpoints
/// run only after the message has been sent. This is not an atomic exchange.
pub fn send_then_handoff(
    send: impl FnOnce() -> isize,
    handoff: impl FnOnce() -> isize,
) -> Result<(), ReplyError> {
    let sent = send();
    if sent != SYSCALL_OK {
        return Err(ReplyError::Send(sent));
    }
    let yielded = handoff();
    if yielded != SYSCALL_OK {
        return Err(ReplyError::Handoff(yielded));
    }
    Ok(())
}

pub(crate) fn yield_invocation(endpoint: ProcessEndpoint) -> (u64, [u64; 5]) {
    (number::YIELD_TO, [endpoint.encode(), 0, 0, 0, 0])
}

/// Receive was interrupted by lifecycle preparation. No message was delivered.
/// Handle preparation according to the application's lifecycle before retrying.
pub const RECEIVE_INTERRUPTED: u64 = (-64i64) as u64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReceiveMode {
    /// Wait until delivery or an explicit kernel/lifecycle outcome.
    Blocking,
    /// Return MAILBOX_EMPTY without blocking or consuming an absent message.
    Poll,
    /// Relative receive deadline in microseconds (currently 1..=200000).
    /// Requires the calibrated native clock. Retry starts a new interval;
    /// adapters must not silently retry and extend a caller's deadline.
    Timed { timeout_us: u64 },
}

// Shared argument construction is also available to host-side contract tests.
// These helpers neither invoke traps nor retain pointers beyond the call.
pub(crate) fn send_invocation(endpoint: ProcessEndpoint, message: &[u8]) -> (u64, [u64; 5]) {
    (
        number::IPC_SEND,
        [
            endpoint.encode(),
            message.as_ptr() as u64,
            message.len() as u64,
            0,
            0,
        ],
    )
}
impl ReceiveMode {
    pub(crate) fn invocation(
        self,
        buffer: &mut [u8],
        sender: &mut ProcessEndpoint,
    ) -> (u64, [u64; 5]) {
        let (number, timeout) = match self {
            Self::Blocking => (number::IPC_RECEIVE_FROM_ENDPOINT, 0),
            Self::Poll => (number::IPC_TRY_RECEIVE_FROM_ENDPOINT, 0),
            Self::Timed { timeout_us } => (number::IPC_RECEIVE_TIMED, timeout_us),
        };
        (
            number,
            [
                buffer.as_mut_ptr() as u64,
                buffer.len() as u64,
                sender as *mut ProcessEndpoint as u64,
                timeout,
                0,
            ],
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{abi::MAX_IPC_MESSAGE_LEN, process::ProcessId};

    #[test]
    fn failure_diagnostic_fits_log_syscall_at_numeric_extremes() {
        extern crate std;
        for stage in [
            "send",
            "reserved-reply",
            "checkpoint",
            "initial-clock",
            "clock",
            "backwards-clock",
            "deadline",
            "attempt-limit",
        ] {
            let report = SendReport {
                status: isize::MIN,
                last_send: isize::MAX,
                stage,
                attempts: u32::MAX,
                elapsed_ms: Some(u64::MAX),
            };
            for part in 0..2 {
                let mut line = std::string::String::new();
                report
                    .diagnostic(&mut line, part, u64::MAX, u64::MAX, u64::MAX)
                    .unwrap();
                assert!(
                    line.len() <= crate::abi::MAX_LOG_WRITE_LEN,
                    "{} bytes: {}",
                    line.len(),
                    line
                );
            }
        }
    }

    #[test]
    fn failure_report_preserves_origin_and_last_send() {
        let full = crate::abi::SYSCALL_ERR_MAILBOX_FULL;
        for (checkpoint, readings, stage, status, elapsed) in [
            (-99, [Ok(10), Ok(11)], "checkpoint", -99, Some(0)),
            (0, [Err(-98), Ok(11)], "initial-clock", -98, None),
            (0, [Ok(10), Err(-97)], "clock", -97, Some(0)),
            (
                0,
                [Ok(10), Ok(9)],
                "backwards-clock",
                crate::abi::SYSCALL_ERR_INVALID_STATE,
                Some(0),
            ),
            (
                0,
                [Ok(10), Ok(2010)],
                "deadline",
                crate::abi::SYSCALL_ERR_TIMEOUT,
                Some(2000),
            ),
        ] {
            let mut clock = readings.into_iter();
            let report =
                send_with_backpressure_report(|| full, || checkpoint, || clock.next().unwrap());
            assert_eq!(
                report,
                SendReport {
                    status,
                    last_send: full,
                    stage,
                    attempts: 1,
                    elapsed_ms: elapsed
                }
            );
        }
        let report = send_with_backpressure_report(|| full, || 0, || Ok(u64::MAX));
        assert_eq!(report.stage, "attempt-limit");
        assert_eq!(report.attempts, 65_537);
        assert_eq!(report.last_send, full);
    }

    #[test]
    fn backpressure_allows_peer_to_run_without_duplicate_delivery() {
        let mut sends = 0;
        let ticks = core::cell::Cell::new(0);
        assert_eq!(
            send_with_backpressure(
                || {
                    sends += 1;
                    if sends <= 100 {
                        crate::abi::SYSCALL_ERR_MAILBOX_FULL
                    } else {
                        SYSCALL_OK
                    }
                },
                || {
                    ticks.set(ticks.get() + 1);
                    SYSCALL_OK
                },
                || Ok(ticks.get()),
            ),
            SYSCALL_OK
        );
        assert_eq!((sends, ticks.get()), (101, 100));
    }

    #[test]
    fn backpressure_deadline_includes_checkpoint_and_never_refreshes() {
        let mut sends = 0;
        let ticks = core::cell::Cell::new(17);
        assert_eq!(
            send_with_backpressure(
                || {
                    sends += 1;
                    crate::abi::SYSCALL_ERR_MAILBOX_FULL
                },
                || {
                    ticks.set(ticks.get() + 500);
                    SYSCALL_OK
                },
                || Ok(ticks.get()),
            ),
            crate::abi::SYSCALL_ERR_TIMEOUT
        );
        assert_eq!((sends, ticks.get()), (4, 2017));
    }

    #[test]
    fn backpressure_stops_on_checkpoint_clock_loss_or_backwards_time() {
        for (checkpoint, readings, expected) in [
            (-99, [Ok(10), Ok(11)], -99),
            (SYSCALL_OK, [Err(-98), Ok(11)], -98),
            (SYSCALL_OK, [Ok(10), Err(-98)], -98),
            (
                SYSCALL_OK,
                [Ok(10), Ok(9)],
                crate::abi::SYSCALL_ERR_INVALID_STATE,
            ),
        ] {
            let mut clock = readings.into_iter();
            let mut sends = 0;
            assert_eq!(
                send_with_backpressure(
                    || {
                        sends += 1;
                        crate::abi::SYSCALL_ERR_MAILBOX_FULL
                    },
                    || checkpoint,
                    || clock.next().unwrap(),
                ),
                expected
            );
            assert_eq!(sends, 1);
        }
    }

    #[test]
    fn backpressure_stalled_clock_is_bounded_even_near_overflow() {
        let mut attempts = 0;
        assert_eq!(
            send_with_backpressure(
                || {
                    attempts += 1;
                    crate::abi::SYSCALL_ERR_MAILBOX_FULL
                },
                || SYSCALL_OK,
                || Ok(u64::MAX),
            ),
            crate::abi::SYSCALL_ERR_TIMEOUT
        );
        assert_eq!(attempts, 65_537);
    }

    #[test]
    fn backpressure_preserves_success_and_other_failures_without_clock_or_retry() {
        for code in (-64..=1).chain([isize::MIN, isize::MAX]) {
            if code != crate::abi::SYSCALL_ERR_MAILBOX_FULL {
                assert_eq!(
                    send_with_backpressure(
                        || code,
                        || panic!("checkpoint without mailbox backpressure"),
                        || panic!("clock without mailbox backpressure"),
                    ),
                    code
                );
            }
        }
    }

    #[test]
    fn handoff_preserves_instance_and_zeroes_unused_registers() {
        for endpoint in [
            ProcessEndpoint::unbound(ProcessId(1)),
            ProcessEndpoint::new(ProcessId(u32::MAX), 0x8000_0001),
        ] {
            assert_eq!(
                yield_invocation(endpoint),
                (4, [endpoint.encode(), 0, 0, 0, 0])
            );
        }
    }

    #[test]
    fn failed_send_never_runs_handoff_or_retries() {
        // Include all current negative statuses and unexpected positive values.
        for status in (-64..=1)
            .chain([isize::MIN, isize::MAX])
            .filter(|s| *s != 0)
        {
            let calls = core::cell::Cell::new(0);
            assert_eq!(
                send_then_handoff(
                    || {
                        calls.set(calls.get() + 1);
                        status
                    },
                    || panic!("handoff after failed send")
                ),
                Err(ReplyError::Send(status))
            );
            assert_eq!(calls.get(), 1);
        }
    }

    #[test]
    fn successful_send_precedes_exactly_one_handoff_even_on_failure() {
        for status in (-64..=1).chain([isize::MIN, isize::MAX]) {
            let phase = core::cell::Cell::new(0);
            let result = send_then_handoff(
                || {
                    assert_eq!(phase.replace(1), 0);
                    SYSCALL_OK
                },
                || {
                    assert_eq!(phase.replace(2), 1);
                    status
                },
            );
            assert_eq!(
                result,
                if status == SYSCALL_OK {
                    Ok(())
                } else {
                    Err(ReplyError::Handoff(status))
                }
            );
            assert_eq!(phase.get(), 2);
        }
    }

    #[test]
    fn send_keeps_full_endpoint_and_exact_borrowed_bytes() {
        let bytes = [0xa5; MAX_IPC_MESSAGE_LEN + 1];
        for endpoint in [
            ProcessEndpoint::unbound(ProcessId(1)),
            ProcessEndpoint::new(ProcessId(u32::MAX), 0x8000_0001),
        ] {
            // Preserve the kernel's validation/error precedence, even for an
            // empty or oversized buffer. Do not truncate or silently split it.
            for len in [0, 1, MAX_IPC_MESSAGE_LEN, MAX_IPC_MESSAGE_LEN + 1] {
                let (number, args) = send_invocation(endpoint, &bytes[..len]);
                assert_eq!(number, 2);
                assert_eq!(
                    args,
                    [endpoint.encode(), bytes.as_ptr() as u64, len as u64, 0, 0]
                );
            }
        }
    }

    #[test]
    fn receive_modes_use_endpoint_output_and_only_timed_receive_sets_r8() {
        let mut bytes = [0xa5; MAX_IPC_MESSAGE_LEN];
        let mut sender = ProcessEndpoint::new(ProcessId(27), 19);
        let original = sender;
        let ptr = bytes.as_mut_ptr() as u64;
        let sender_ptr = &mut sender as *mut ProcessEndpoint as u64;
        for (mode, expected, timeout) in [
            (ReceiveMode::Blocking, 337, 0),
            (ReceiveMode::Poll, 352, 0),
            (ReceiveMode::Timed { timeout_us: 1 }, 359, 1),
            (ReceiveMode::Timed { timeout_us: 200000 }, 359, 200000),
            (
                ReceiveMode::Timed {
                    timeout_us: u64::MAX,
                },
                359,
                u64::MAX,
            ),
        ] {
            let (number, args) = mode.invocation(&mut bytes, &mut sender);
            assert_eq!(number, expected);
            assert_eq!(
                args,
                [ptr, MAX_IPC_MESSAGE_LEN as u64, sender_ptr, timeout, 0]
            );
            assert_eq!(sender, original);
            assert_eq!(bytes, [0xa5; MAX_IPC_MESSAGE_LEN]);
        }
        assert_eq!(RECEIVE_INTERRUPTED as isize, -64);
    }
}
