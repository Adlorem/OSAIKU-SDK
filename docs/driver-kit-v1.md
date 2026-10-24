# Driver Kit v1 (DRV-01 and DRV-06)

`osaiku-driver-protocol` and `osaiku-driver-runtime` are public, dependency-minimal
`no_std` Rust crates. Protocol and the owner-side process contracts forbid unsafe code. Runtime depends only on protocol; its freestanding `native` module contains the small x86 syscall boundary.
They have no OS checkout, private ABI, UI, allocator or hardware dependency.
Rust structs and traits are local APIs, **not** a cross-process Rust ABI.
The initial production consumer is the OS's trusted NVMe DMA owner. A separate
client fixture lives under `tests/driver-kit-client`, never in a production image.

DRV-06 supplies a native mediated transport and a kernel-owned headless process profile. Package signature policy and direct hardware/IOMMU grants remain later stages. A manifest, decoded handle or valid signature cannot allocate hardware.

## Build profile and independence

Use the repository's pinned `nightly-2026-04-19`, edition 2021, panic abort for
native release artifacts, `targets/x86_64-osaiku-user.json`, and `rust-src`:

```sh
cargo test -p osaiku-driver-protocol -p osaiku-driver-runtime --offline --locked
python tools/check-driver-kit.py --target
```

The second command copies only the two crates, public target and separate client
fixture into a temporary directory outside the SDK/OS. It checks the complete
Cargo dependency graph, runs the client contract, and builds its host and no_std
libraries plus a separate guest executable. Its native invocation is `cargo build --release --offline --locked --features guest -Zbuild-std=core,compiler_builtins -Zbuild-std-features=compiler-builtins-mem -Zjson-target-spec --target <public-target.json>`.
The temporary directory must not be inside either checkout. No private include,
path dependency or implicit kernel target is part of the Driver Kit profile.

## Identity and authority

All IDs are nonzero u64 values. `DeviceId` is a host registry's boot-session ID,
not a persistent hardware serial. `ConnectionGeneration` changes on reattach;
`DriverInstance` changes on every restart independently of connection generation.
Together they form `Scope`. Request/resource/endpoint IDs are scoped to that
instance. Persisted package identity is separate and is not implemented here.

There is one retained `Sequence` per issuing namespace. `issue()` uses checked
addition: after exhaustion every issuance fails. Never reconstruct a sequence
or a request queue with the same live scope to reset its counter. A restarted
issuer needs a new instance, and an exhausted identity namespace is retired.
IDs and messages do not survive a host boot/channel lifetime. Stable hardware
matching, persistence and the actual issuer registry are DRV-02 responsibilities.

Decoded identities are claims. The receiving host derives caller identity and
execution context from its own process/channel registry, then compares scope,
resource ID, rights and bounds against its own live resource lease. It must copy
untrusted shared input into an owned immutable snapshot before parsing; a Rust
slice alone does not protect against another process mutating shared memory.

`ResourceLease::admitted` records a platform-admitted allocation. It does not map
or allocate hardware. Prepared resources permit descriptor construction; activation
is explicit. Quarantine revokes access but retains ownership. Only the platform's
actual stop proof permits retirement. Neither a timeout, cancel, destructor nor
peer `Stopped` record proves DMA has ended. Lease records are not Copy/Clone.

## Wire format

All integers are little endian; no padding, native pointers, trait objects,
CPU/bus addresses or function addresses cross this interface. Unknown enums,
nonzero reserved bytes, inconsistent/trailing lengths and unnegotiated feature
use are rejected. This version has no TLV extensions to silently skip.

The bootstrap offer is exactly 32 bytes: `ODH1` at 0, major u16 at 4, minor u16
at 6, supported u64 at 8, required u64 at 16, and eight zero bytes at 24.
Major must be 1. Minor is the lowest supported version (currently 0).
Unknown optional features are ignored; any required feature outside the known
intersection refuses negotiation. CANCEL is bit 0; DEADLINE is bit 1.
After negotiation, packet major/minor must match the selected 1.0 exactly.

| Offset | Field |
| --- | --- |
| 0 | `ODRV` (4 bytes) |
| 4 / 6 | major / minor, u16 each |
| 8 / 10 | kind u16 / reserved zero u16 |
| 12 | exact total bytes, u32 |
| 16 / 24 / 32 | DeviceId / ConnectionGeneration / DriverInstance, u64 each |
| 40 | nonzero RequestId, u64 |
| 48 / 52 | payload bytes u32 / reserved zero u32 |
| 56 | features used by this message, u64 |
| 64 | payload, at most 256 bytes |

Kinds: Hello=1, Submit=2, Complete=3, Cancel=4, Quiesce=5, Stopped=6,
Ready=7, Fault=8. Cancel requires the negotiated CANCEL bit in the message;
Submit requires DEADLINE. The envelope parser does not interpret class payloads:
dispatch must additionally select and validate the correct payload schema.

`ResourceHandle` is exactly 32 bytes: three scope u64s, then ResourceId u64.
`Transfer` is exactly 56 bytes: that handle, offset u64, byte length u64,
absolute deadline u64. Its scope must match the envelope. The buffer must be
registered by the receiving owner; bounds/direction are checked against its lease.
Transfer length is 1..262144 bytes; overflow and zero deadline are refused.
Deadlines use the owner's monotonic nanoseconds within the current boot, never
wall time or peer-provided CPU cycles. The owner supplies `now`; clock exhaustion
must refuse new work rather than wrap into a new clock era.

`Completion` is 16 bytes: actual length u64, outcome u16, six zero bytes.
Outcomes are Completed=0, Cancelled=1, TimedOut=2, Failed=3, UncertainWrite=4.
The queue separately validates actual length against the accepted request.
A short transfer is representable; whether it is success is the class's policy.
An uncertain write is never reclassified as success or automatically replayed.
Lifecycle kinds reserve their envelope dispatch values; their class/manager
payload schemas and IPC endpoints are introduced with real DRV-02/06 consumers.
This library does not dispatch unknown payloads or fabricate those services.

## Lifecycle

| From | Operation | To / condition |
| --- | --- | --- |
| Detected | wait_for_dependency | Waiting |
| Detected, Waiting | matched | Matched; code/match only, no resources |
| Matched | resources_admitted | Allocated; trusted owner's actual grant |
| Allocated | probe | Probing |
| Probing | ready | Ready; matching, registered live class endpoint required |
| Allocated, Probing, Ready, Faulted | quiesce | Quiescing; endpoint withdrawn first |
| Detected, Waiting, Matched | dependency_lost | Waiting |
| Allocated, Probing, Ready, Faulted | dependency_lost | Quiescing |
| Any except Stopped, Removed, Quarantined | fault | Faulted; endpoint withdrawn |
| Faulted, Quiescing | quarantine | Quarantined; resources remain pinned |
| Quiescing, Quarantined | platform_stopped | Stopped; actual platform synchronization proof |
| Detected, Waiting, Matched, Stopped | remove | Removed |

All other transitions refuse without mutation. Stopped/Removed bindings cannot
restart; construct a new binding with a freshly issued instance. Partial probe
failure cannot skip cleanup. A trusted class owner implements `ClassEndpoint`;
registration holds a reference to that actual endpoint and Ready rechecks its
scope and liveness. Provider loss must withdraw publication before endpoint
retirement. This record is not an untrusted peer's boolean assertion of Ready.

## Requests, buffers and context

Queues hold 1..32 metadata slots. Submission requires service context, valid size
and an unexpired deadline; full queues return Backpressure without consuming an
ID. Cancel and expiry mark requests but **do not free slots or buffers**. A late
completion can retire its exact matching scope/ID and reports whether cancel
was requested. Duplicate/stale completions and oversize lengths leave slots
unchanged. Quiesce refuses new submissions while allowing pending completions.
Only a trusted platform stop proof permits bulk retirement of remaining work.

The queue neither owns payload bytes nor performs I/O. Its owner must retain/pin
buffers until validated completion/retirement. Callers supply context from trusted
execution state, never from a message. No operation in these crates spins, sleeps,
allocates or waits. The event loop waits only after releasing locks, outside IRQ;
an IRQ may record bounded pending work and wake the owner. DMA cache/order/IRQ
synchronization remains in the platform adapter. Reset, timeout and device-specific
stop verification are not substituted by these state machines.

All regression tests live in `driver-*/tests` or the external client fixture.
Production sources have no test methods, failure hooks or test-only switches.

## DRV-06 native process transport

`driver_runtime::native::{identity, exchange, wait, exit}` is available only for
`x86_64` / `target_os=none`. No OS crate or UI runtime is needed. `identity()`
returns the host-issued scope, release and endpoint, plus role in `value` (1 is
manager, 2 is driver). Clear `value`, send HELLO then READY after initialization.
The channel accepts version 1 exactly. READY publishes only the admitted channel;
it cannot create an endpoint, change the selected release or grant hardware.

Syscall 400 uses RDI=0 for exchange, RSI points to one exclusively borrowed
384-byte record, RDX=384, R8/R9=0. RDI=2 obtains identity in the same record;
RDI=1 with other arguments zero is reserved to initd for manager startup.
The kernel verifies role and its issued process endpoint before touching the
buffer, collects other user CPUs, then copies the entire record once. No user
pointer survives the call. `wait()` uses the existing timed event receiver with
a 100 ms deadline; it does not spin or create a scheduler.

The wire magic is `ODPROC01` at 0. Ten little-endian u64 fields follow at offset
8: operation, device, connection, instance, release, endpoint, sequence, offset,
length, value. Bytes 88..128 must be zero; bytes 128..384 hold copied data.
Offsets/lengths address that 256-byte payload and use checked addition. Metadata
is kernel-owned and separate from queued payloads. See `process::op` for operation
numbers. Applications cannot select this role by package name, PID or manifest.

The initial driver endpoint is **read-only device status**: SUBMIT requires zero
offset/length/value, returns an owner-issued sequence, and TAKE returns 16 bytes
(state and selected running release). CANCEL consumes only that copied receipt.
Eight receipts fit per instance; a ninth receives backpressure. Arbitrary hardware
commands, writes, user completions, shared buffers, MMIO, port access, DMA and IRQ
registration are denied. Hardware class migrations add approved endpoints in
later stages; this endpoint is not a raw hardware control API.

The manager can SNAPSHOT the actual bus registry, RECONCILE retained platform
owners, APPROVE the kernel-selected device/release, START the authenticated
headless image for a device status endpoint and query retained STATUS failures.
An initial snapshot has sequence/value zero; replies carry next cursor/revision
and up to four 48-byte rows: device, connection, selected driver, selected release,
class endpoint, state. APPROVE supplies the first 32 bytes of one row. START
supplies only device/connection (16 bytes). STATUS holds five failure/attempt byte
pairs, then five active bytes. Failed device generations must never be replayed.

The profile admits one manager plus four clients, three lifetime launch attempts
per binding, image <=64 frames, page tables <=16 and stack <=4, all through the
existing charged allocator and protected reserve. CPU shares remain in the
System scheduling class; no peer IRQ budget exists because IRQ grants are zero.
Start deadline is 5 s, driver heartbeat deadline 3 s, manager 15 s, and each peer
has 128 admitted messages per second. Failures and attempt counts survive process
restart in the kernel until reboot. Watchdog runs on collected timer/trap/idle
boundaries even for a looping process. Admission and copies close before process
retirement. This copy-only profile owns no DMA; trusted platform leases never
retire because a peer died or claimed Stopped. No uncertain write is replayed.

Use `python tools/check-driver-kit.py --target --guest-output <ELF>` for the
independent negative guest, or add `--oversize` for loader-budget refusal. Both
live exclusively in `tests/driver-kit-client`, are excluded from production base
images and require a separately signed disposable QEMU fixture to execute.
