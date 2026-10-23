# Driver Kit v1 (DRV-01)

`osaiku-driver-protocol` and `osaiku-driver-runtime` are public, dependency-minimal
`no_std` Rust crates. Both forbid unsafe code; runtime depends only on protocol.
They have no OS checkout, private ABI, UI, allocator or hardware dependency.
Rust structs and traits are local APIs, **not** a cross-process Rust ABI.
The initial production consumer is the OS's trusted NVMe DMA owner. A separate
client fixture lives under `tests/driver-kit-client`, never in a production image.

This stage supplies contracts, not a process launcher, device manager, package
signature policy, hardware syscalls or IOMMU isolation. Those belong to subsequent
DRV stages. A manifest, decoded handle or valid signature cannot allocate hardware.

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
libraries. Its native invocation is `cargo build --release --offline --locked
-Zbuild-std=core -Zjson-target-spec --target <public-target.json>`.
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
