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

DRV-07 distribution, publisher roles and durable selection are specified in
[Driver packages v1](driver-packages-v1.md). Hardware class migration supplies
the live admission facts; installing a package does not itself start a driver.


## DRV-08 installed Boot HID endpoints

The trusted USB core may grant a selected, signed Input-role package endpoint
1 (keyboard) or 2 (mouse). Scope binds the physical connection and a fresh driver
instance. The owner keeps transfer rings, DMA, descriptor reads, rearming and
slot teardown; the client receives only copied reports. It cannot register a
source, choose another interface, select focus, interpret text or access a
framebuffer. The manager's status-only clients do not inherit HID operations.

After HELLO/READY, HID_READ (13) requires zero sequence/offset/length/value. An
empty queue returns sequence zero; otherwise it returns a one-use sequence and
an 8-byte keyboard or 3-byte mouse report. A malformed report is represented by
zero length. Only one receipt can be outstanding per endpoint. HID_INPUT (14)
consumes that receipt with the same scope/release/endpoint and zero offset:

* Mouse: value=0, length=6; little-endian i16 dx/dy (-128..127), three button bits,
  and a zero reserved byte. Malformed/lost input: value=1, length=0.
* Keyboard: value=0, length=40; twenty little-endian physical `hid::Poll` values.
  Zero means no event, 1 ignored, 2 source lost, otherwise 0x4000 plus usage in
  the low byte and 0x8000 for pressed. All other bits are rejected. The shared
  consumer applies layout, modifiers, repeat and routing; the package emits no text.

The owner queue has 32 copied reports. Overflow revokes delivery and releases
source state; old receipts cannot cross detach, restart or version replacement.
Selected interfaces use exactly one decoder. Boot fallback is withdrawn before
package launch and remains withdrawn on process failure, until a bounded retry
or explicit protected deactivation. Detach never resets the shared controller.
Input wakes the service; the owner also wakes a blocked HID service every 100 ms
for heartbeat. A wake does not renew the unchanged 3 s watchdog. Each drain is
bounded to 32 reports and each HID instance to 8192 messages/s. Automatic retries
are limited to three per connection; a new explicit catalog/policy decision can
renew that budget without resetting any connection, receipt or instance ID.

These clients use the shared charged ELF/W^X loader and durable package pins.
They run in System scheduling class with no IRQ/DMA grant. This profile admits
only CPU ceiling 1000 permille (no smaller per-process cap is implemented),
I/O budget at least 4 KiB and queue budget at least 32; unsupported requirements
are refused rather than silently weakened. There is no promise
of a dedicated CPU share or hard latency bound. Trust and exact selection are
rechecked before start and confirmation. The installed image pin is released
only after process retirement. Unrelated HID interfaces keep running during a
package update; policy changes revoke affected runtime authority before restart.

The shipped decoder handles Boot Protocol only: mouse three buttons/relative
X/Y, keyboard eight modifiers and six deduplicated usages. The keyboard ignores
OEM byte 1 as required by HID 1.11 B.1/F.4. Rollover/error releases the source and
requires a neutral report. Existing layout limitations and lack of LED output
are preserved; full report-descriptor parsing, NKRO and mouse wheels are outside
this profile. Independent sample packages live in the OS's `drivers/usb-*`
directories, and compile outside the OS checkout with only Driver Kit dependencies.


## Installed i8042 clients (DRV-09)

Copied input endpoint kinds 3 and 4 identify the controller-authenticated
translated Set 1 keyboard and standard relative AUX pointer. They reuse the
owner/connection/instance/release checks and one-use receipt of HID_READ (13)
and HID_INPUT (14); the operation numbers remain unchanged. No client can
register a source or request a port address. USB endpoint kinds 1/2 are unchanged.

A pointer record is eight bytes: byte, status (0=data, 1=wire error, 2=pointer timer),
packet length 3, reserved zero, low 32 bits of monotonic milliseconds in little
endian. `ps2::Clock` extends the wire clock. A keyboard record is eight bytes:
count (1/2/6), damaged flag, and six zero-padded bytes of a complete Set 1
sequence. The controller retains prefix framing across owner replacement;
`ps2::keyboard` decodes the copied sequence. Keyboard output is one encoded
physical `hid::Poll` (two bytes). AUX output is eight bytes: signed i16 X/Y,
buttons, action, command, reserved zero. `ps2::response` and `valid_response`
define the action encoding. Pointer timer records use the same one-use receipt;
they cannot grant a keyboard or another process permission to publish input.

The controller performs initial identification and synchronizes AUX before
every owner replacement with the existing bounded SDK recovery machine.
It then routes raw bytes exclusively to the installed parser. Commands FF,
E8/03 and F4 are device commands behind a stateful allowlist, bounded to
32 per second and three device resets per grant. The reset of an uncertain
command boundary belongs to the trusted controller, never a new client.
No controller configuration, output-port write, arbitrary PIO or platform
reset operation is exposed. Kernel power admission blocks new commands.
Caps LEDs stay with the common keyboard consumer and an asynchronous
controller-owned ACK/RESEND exchange; ambiguous timeout quarantines LED
writes until reboot while ordinary keyboard capture remains available.

The native clients retain the existing Set 1 vocabulary and standard relative
pointer mode. Extended touchpad modes and I2C-HID are not implemented. Linux
reference algorithms were compared, not copied; existing Redox notices apply
to the relocated bootstrap/protocol code unchanged.

## Block class and mediated BOT process (DRV-10)

`block` defines backend-independent geometry, identities, relative ranges,
sealed requests and completion ownership. `block_queue::Queue<N, B>` adds an
owned byte copy for each bounded asynchronous request. Admission validates the
owner-provided write range before dispatch. It serializes backend operations,
orders write/flush, rejects stale generations and preserves uncertain requests
until the trusted transport owner proves hardware stopped. A caller's cancel,
timeout or process death is never such a proof. New media require a new identity.

Read fragments are copied in order and published only after complete success.
Submitted writes and flushes with errors return `UnknownCommit`, stop admission
and cannot be replayed. Durability distinguishes volatile RAM, device cache and
acknowledged stable storage according to the advertised device capability; the
last category is a device protocol acknowledgement, not physical qualification.
The OS independently reports physical persistence as unverified. Queue storage
is static and bounded by N × B plus metadata; no borrowed buffer is retained.

`block_io::Media` is the synchronous compatibility view used by FAT and existing
storage clients. It must consume each slice before returning, including failure.
The current OS adapters retain this interface while USB transfers use copied
owner buffers internally. The async queue is available to clients that adopt
owned asynchronous requests; its presence does not make the legacy filesystem
call stack asynchronous. NVMe protocol/deadline code is also shared by Driver Kit;
NVMe controller ownership remains in the trusted OS until DRV-14.

`class::Kind::UsbStorage` is kind 5. `hid::Kind` remains an alias for compatibility.
BLOCK_FETCH (15) / BLOCK_STEP (16) carry a copied 64-byte protocol event/action,
with a monotonically increasing one-use receipt in `Message.sequence`.
`block_process::Channel` rejects late replies, duplicates and a reply crossing
process loss/rebind. The kernel additionally binds every operation to its exact
process endpoint, instance and selected release. A storage endpoint cannot use
input publication operations; an input endpoint cannot use block operations.
The owner admits this channel with `Instance::admit_block`. After validating
each one-use action it calls `block_progress`, reimbursing up to five calls
(heartbeat/fetch/reply plus an optional idle heartbeat/fetch). This permits
solicited bulk traffic without relaxing the 8192-call window for unsolicited
traffic. A peer cannot refund its own calls or revive a quarantined instance.

Event byte 0 is 1 (new command) or 2 (transfer completion). Commands encode LUN
in byte 1, CDB length in byte 2, IN in byte 3, tag at 4, transfer length at 8,
and 16-byte CDB at 16. Completions encode status in byte 1, actual length at 4,
and CSW bytes at 16 when applicable. Actions encode the BOT stage in byte 0
and exact 31-byte CBW at 1 for the CBW stage. All other bytes are zero.
The mediator owns the original request/data and checks the returned action;
the process never supplies an address or a new range to the controller.

Storage role and a valid signature do not authorize arbitrary disk access. The
host retains partition/grant checks; a direct controller owner must still be
trusted for whole-medium integrity even with IOMMU protection. Tests remain in
the separate `driver-runtime/tests/block_queue.rs` and `block_process.rs` files.


## Admitted xHCI controller process (DRV-12)

`controller` adds endpoint 6 operations IO=17, FETCH=18 and COMPLETE=19 to
existing process messages. Only a platform-admitted controller instance may
use them. Request payloads are canonical 64-byte records; replies are 32 bytes.
The kernel assigns monotonic receipts and checks controller generation, target,
length and single consumption. Queue slots are dispatched in receipt order.
Class HID/BOT messages and USB-core endpoint handles are unchanged.

`controller_io::Gate` accepts offsets in an observed register inventory and
IOVA addresses in one retained 344064-byte arena. It exposes no PCI writes,
physical allocator, IRQ destination or arbitrary mapping. MSI-X table/PBA pages
are excluded entirely. Data copies are at most 256 bytes per call; shared user
pages are not implemented in this profile. READ/WRITE_REGISTER, READ/WRITE_ARENA,
PUBLISH, ACQUIRE, CLOCK and GRANT are mediated operations, not capabilities to
perform raw CPU I/O. Failed retirement closes access even if hardware stop
cannot be confirmed; platform ownership must remain retained.

The OS example is `drivers/xhci`, built independently with only this runtime
and driver-protocol. Firmware ownership, PCI discovery, isolation, remapped IRQ
and reset/power-group qualification remain prerequisites supplied by a trusted
platform owner. The native allowlist currently admits no physical platform.
A manifest requesting `iommu` does not grant one. Replacing the signed package
requires no kernel rebuild, but activation needs qualified native admission.

Native `wait_for` accepts microseconds (1..100000). xHCI waits for IRQ, queued
work or the nearest operation deadline; heartbeat wakeups do not scan MMIO.
Failure invalidates all children of the controller generation. The initial
native policy stages replacement and recovery for reboot, retaining the group.
