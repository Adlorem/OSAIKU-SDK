# GPU package manifest v2

[Manifest v3](driver-packages-v3.md) adds an optional signed public key while
preserving this GPU contract and requiring explicit host-owned trust consent.

This format is implemented by `osaiku-package` and `tools/package.py`. The
GPU-12 bridge system advertises protocols 11–13 version 1.0 with no optional
features. It loads separate backend and renderer roles, supervises their
incarnations and brokers resources. Its single-ELF driver loader still refuses
GPU packages. The CML two-ELF package builds independently; physical execution
and acceptance remain pending. Signing or compiling is not hardware acceptance.

The envelope remains `OSADRV01` v1. The authenticated manifest is `OSADMF01`
version 2; old parsers refuse it. Class 64 means GPU hardware access and
requires **both** Driver (4) and DriverGpu (9) approval for the exact publisher
and signing epoch. DriverController (7), System and application trust do not
imply GPU trust. The package remains Installable; controller restrictions are
unchanged. An actual resource grant will additionally require device ownership,
platform admission, resource accounting and protocol negotiation.

The v1 prefix layout is retained, with the contract length 192 at offset 474
(u16); 476–511 are zero. Classes must be exactly 64, isolation must be IOMMU,
and entry must be native. After compatibility come the ordinary 32-byte match
records and exact 208-byte dependencies, then the 192-byte GPU descriptor and
112-byte component records. All fields are little endian, without Rust layout.

Each GPU match requires PCI, exact vendor/device and display class 3. Bytes
14 and 15 hold inclusive revision bounds; 16–31 are zero. Missing observed
revision never matches. The legacy `matches`/`match_records` API refuses v2;
the device owner uses `matches_revision`. A revision range cannot be silently
dropped by a legacy registry adapter.

| GPU descriptor offset | Size | Meaning |
|---|---:|---|
| 0 | 64 | Lowercase family ID, digits/dot/hyphen, zero padded |
| 64, 80, 96 | 16 each | Platform/backend, backend/renderer, client/renderer interfaces: u16 major/minor, four zero bytes, u64 required features |
| 112, 144 | 32 each | Backend/renderer budgets: u64 memory, u64 DMA, u32 stack, u32 TLS, u16 threads, u16 instances, four zero bytes |
| 176, 177 | 1 each | DMA/GPU address widths, 32–64; these are distinct address spaces |
| 178 | 1 | 1 unified memory, 2 device-local memory; descriptor, not platform qualification |
| 179 | 13 | Reserved, zero |

The mandatory compatibility protocols 11, 12 and 13 must match the respective
major/minor descriptors. A host must not advertise them until implemented;
`Manifest::compatibility().check` also requires every signed feature bit in
the corresponding `ProtocolSupport::features` offer. A matching version or
features of another protocol cannot substitute for the required feature.
Runtime admission must supply a current platform-owned offer. SDK release
strings and private kernel build hashes do not determine compatibility.

Memory is per process, 4 KiB aligned, at most 1 GiB. Stacks are 4 KiB–1 MiB,
TLS 0–64 KiB, threads/instances 1–64; their stack/TLS footprint must fit inside
the declared memory. The backend has exactly one instance and at most 4 GiB
separately charged DMA. Renderer DMA is zero. The common manifest memory,
threads and instances equal the backend budget. Normal v1 prefix limits still
apply. These are validation limits, not evidence that memory has been allocated.

The first two components are exactly kind 4 `gpu-backend` (at most 16 MiB)
and kind 5 `gpu-renderer` (at most 128 MiB). Both must identify x86-64 ET_EXEC
ELFs and pass the common W^X/segment loader. Kind=data is never
executable. Remaining named firmware/data components retain the 1 MiB limit;
there are at most 128 resources, additionally bounded by the 16 KiB metadata
limit. The complete package retains the 256 MiB bound. Both contiguous and streaming
verification check every component hash, exact offsets, padding and ELF header.

Authoring adds a `[gpu]` table and replaces `package.executable` with explicit
`backend` and `renderer` paths. For example, after an ordinary `[package]`
whose memory is 262144, threads 1, instances 1:

```toml
[compatibility]
abi_major = 1
protocols = [{id=11, major=1}, {id=12, major=1}, {id=13, major=1}]
[driver]
classes = 64
cpu_permille = 50
queue = 8
isolation = "iommu"
reset = "cold"
[[matches]]
bus = "pci"
vendor = 32902
device = 39876
class = 3
revision_min = 0
revision_max = 5
[gpu]
family = "intel-cometlake"
backend = "backend.elf"
renderer = "renderer.elf"
platform_abi = {major=1}
renderer_abi = {major=1}
client_abi = {major=1}
backend_budget = {memory=262144, dma=4096, stack=16384, threads=1, instances=1}
renderer_budget = {memory=262144, stack=16384, threads=1, instances=1}
dma_address_bits = 39
gpu_address_bits = 48
memory_model = "unified"
```

These small example budgets illustrate serialization, not a measured Mesa
configuration. A real release must derive budgets from its binaries and workload.
Use `driver-pack`, `driver-inspect` and `driver-verify` as for v1, approving
`driver-gpu` separately. `driver-key --classes 64` emits a public-only trust
request. Do not substitute test signing keys for installation keys. Every content
change requires a new release; the existing OSAIFS store rejects equivocation.

The activation catalog format 2 persists a boot counter and monotonic attempt
counter. Each GPU selection retains the exact previous digest; each start,
including a previously Active release, persists its selection, policy epoch,
device/connection and boot/attempt before an image can be planned. Only a
platform-observed fence and at least three presentations can confirm it. A
failed write issues no start token. Incomplete older attempts restore only a
currently trusted, compatible predecessor above the release floor, or disable
the selection. A failed predecessor is not automatically tried again.

Backend and renderer use separate authenticated image plans and process memory
budgets. Renderer plans receive zero device permissions. Loading either image
does not grant MMIO/DMA or prove hardware health. Published package pins survive
partial startup and process failure; the previous package and its dependencies
remain retained until deactivation and a subsequent boot.

`gpu-protocol::control` defines the 128-byte process control envelope (syscall
410). Identity comes from the kernel's exact endpoint binding. Heartbeats and
process claims cannot create a device grant or confirm a trial. The kernel
advertises GPU protocols 11–13 independently of the subsequent hardware grant.

`gpu-runtime::memory` publishes the retained allocation/DMA interfaces used by
the extracted Comet Lake hardware library. They do not perform native syscalls
or allocate physical memory. The initial address limits preserve the qualified
CML profile; other address widths in a descriptor do not widen these limits.
The buffer records in `gpu-protocol::buffer` are re-exported by the existing
runtime graphics module, preserving its ABI and type identities.

`gpu-runtime::process::Process` binds once to the kernel's returned identity.
The freestanding `Native` transport implements syscall 410 without private ABI
imports. A changed reply or failed transport poisons the client, preventing an
unbounded retry after a stale owner. Renderer role cannot report backend health.

`gpu-protocol::render` defines the pointerless SDK drawing batch. The 64-byte
`OSAGRB01` header contains session/generation/sequence (u64 at 8/16/24), draw
count and exact byte length (u32 at 32/36), and viewport width/height (40/44).
Bytes 48–63 are zero. There are 1–32 records of 152 bytes, at most 4928 bytes
per batch. Each record contains operation/color, rect/clip/shape, eight u32
parameters, zero bytes 88–95, a full 40-byte buffer handle at 96, and u64
offset/length at 136/144. Only fill, packed glyph, rounded shape and image
operations are accepted; nested batches and native pointers are not encoded.
Glyph dimensions are at most 64×64 and assets at most 4 MiB. The format does
not transport output pixels or authorize a buffer merely by naming its handle.

`runtime::gpu_painter::encode_batch` translates local painter records through
an asset-sealing owner. `gpu-runtime::render::Prepared` checks the expected
session, sequence and viewport, validates every record, then resolves all
asset ranges before returning the first draw. Its `Assets` implementation must
enforce the actual kernel-owned seal and session binding; this trait itself
does not create a seal. Receiver mappings stay borrowed through execution.
The private renderer's Mesa adapter creates local FFI pointers from those
borrows. Texture cache keys include owner/device/generation/serial/offset/length,
so recycled mapping addresses cannot reuse a different image. Native syscall
412 supplies bounded scene sessions and immutable asset mappings; 413 supplies
copied backend RPC. The complete client/renderer/device/connection identity is
checked on both paths. Applications use `runtime::gpu_client`; composition uses
`runtime::gpu_compositor`. Neither client links Mesa or family code.

Backend RPC also supports blocking `CALL (6)` and `RECEIVE_WAIT (8)` through
`gpu-runtime::backend::Client::call` and `Server::receive_wait`. CALL uses the
same authenticated peer and copied private payload as SEND. Its packet length
is the response capacity; the header length is the request length. Successful
return contains a CALL/DONE envelope and the copied reply. BUSY means that no
request was admitted; a timeout or loss after admission is terminal and must
never replay an execution. The legacy SEND/POLL pair remains supported.

The kernel saves the exact calling thread and an internal `WAIT (7)` envelope
with the issued serial, then revalidates memory and authority on resumed entry.
No kernel stack or user-memory borrow survives the wait. Registration rechecks
completion; replies, cancellation, peer departure and a bounded deadline wake
the saved thread, not an arbitrary renderer control thread. Reply publication
is a scheduler handoff boundary. Peer preference stays within the selected
service class and retains normal fairness/budget checks. RECEIVE_WAIT wakes
on a queued request or returns BUSY after 10 ms for maintenance/heartbeat; the
backend must not add another fixed sleep after that return. Family batching
remains opaque to the kernel. Comet Lake package 11 uses one private operation
for a validated target binding snapshot followed by execution; unchanged
bindings stay installed, and only the retired hardware receipt commits the
renderer ledger. It requires a kernel implementing the blocking operations.

Ordinary `graphics=auto` boot now preserves admitted firmware GOP before reading
the package catalog. It does not automatically invoke the statically linked
CML engine. Explicit hardware diagnostics retain the legacy path; `graphics=cpu`
also bypasses installed GPU activation. AMD-Vi/IVRS remains unimplemented.

The native platform endpoint is syscall 411, `gpu-protocol::platform`.
`OSAGPL01` is exactly 256 bytes: operation at 8, attempt/selection/device/
connection at 16/24/32/40, package digest at 48, sixteen u64 arguments at 80,
and zero bytes 208–255. Every operation rejects unused nonzero arguments.
The kernel binds this scope to the exact signed backend endpoint and its
manifest budget. `GpuRenderer`, ordinary drivers and application workers have
no authority to call it. The public `gpu-runtime::platform::Platform` and
freestanding `Native` transport implement this boundary without private ABI.

| Operation | Inputs | Output |
| --- | --- | --- |
| INFO (1) | None | PCI identity/class, BAR lengths, GOP dimensions/stride/offset/length, TSC frequency, DMA address width |
| MAP_BAR (2) | BAR 0 or 2, offset, 1–32 pages, UC or WC | CPU window and byte length |
| HANDOFF (3) | Version 1, retired/coherent flags 3 | No payload |
| ALLOCATE (4) | 1–32 pages, device-write flag | Unique handle, CPU address, IOVA, bytes |
| RELEASE (5) / SYNC (6) | Exact allocation handle | No payload |
| IRQ (7) | Enable flag | Monotonic interrupt receipt count |
| REVOKE (8) | None | Completion flag; each call retires at most one allocation or BAR span |
| CONFIG (9) | Read-only PCI offset 0, 4, 8, 0x50 or 0x5c | Raw observation; family decoding stays in the package |
| RESERVATION (10) | None | Start/end of the retained firmware interval in device address space; no CPU mapping |

MMIO, aperture and DMA windows are disjoint. Their page-table parents are
charged to dedicated GPU process roles; they contain no user-accessible leaves
before a resource grant. No ordinary RAM physical address crosses the endpoint.
The native owner verifies immutable BAR assignment, memory type, reserved
regions and overlapping owners. It rejects BAR aliases even when they request
the same cache type. A renderer gets no MMIO parents or resource grant.

HANDOFF promotes the retained GOP domain without changing TE, IR, context roots
or firmware mappings. It accepts a retirement/coherence receipt only from the
trusted backend that already holds MMIO. This requires an actual family
implementation to perform that protocol; calling READY or identifying PCI is
insufficient. This native endpoint has host transaction/fault tests and a
freestanding kernel compile, but has not executed with an external family ELF.
Protocol offers are now present; package compatibility alone never grants
hardware authority. Platform admission and the signed resource budget are still
required. No physical execution is asserted by these offers.

RAM is zeroed and charged before publication. The native owner retains the
allocation before DMA registration or CPU/IOVA publication. Contiguous IOVA
spans skip fragmented holes; release requires IOTLB drain and exact CPU mapping
retirement. Handles never repeat, including when a drained CPU/IOVA slot is
reused. Lost publication/release receipts quarantine the owner and retain its
tokens and process root. Device Manager refresh preserves the connection epoch
when the observed PCI identities/topology have not changed.

The CML package library now contains a process adapter for firmware takeover
and RCS initialization. It receives raw PCI observations and the preserved DMA
interval through 411, validates CML stolen/GGTT layout itself, maps only its
BAR windows, parks firmware GT work and rewrites unprotected GGTT to a retained
platform allocation's IOVA. Protected GOP/display entries remain unchanged.
The adapter retains the sole forcewake/reset owner while installing private
HWSP storage. Two RCS contexts, real CSB/fence observation and bootstrap pattern
readback precede copying the GPU-produced golden LRC. This code is wired into
the independently built `cometlake-backend` ELF with per-renderer contexts,
buffers, execution, presentation and cooperative shutdown. It has not run on
physical hardware.

`memory::intel::TableSpace` accepts a table-page provider independent of CPU
physical addresses. The original native provider preserves existing physical
frame/DMA contracts; `memory::process::ProcessTables` uses platform allocation
tokens, CPU mappings and distinct DMA capabilities. Host tests build actual
table bytes through those public contracts, including stale generations and
lost cache/release receipts. They model platform DMA, not physical GPU access.

CPU-only shared objects support `driver-protocol::shared::op::SEAL` (7).
It closes all writable aliases and grants, completing CPU TLB invalidation
before publishing an immutable receipt. `QUERY` and `SEAL` return `token=1`
only after that transition. A lost unmap receipt leaves the object terminal,
charged and unsealed; neither a retry nor a read-only grant substitutes for
the proof. Native GPU sessions pin these sealed objects until the receiver
finishes or its exact process incarnation is retired.

## Bridge release and independent updates

The bridge release changes the kernel ABI offer and all delivered scene clients
once. Later compatible `.osadrv` updates use the same clients and kernel. Standard
kernel builds exclude CML; `intel-gpu-diagnostic` explicitly restores the legacy
hardware diagnostic implementation. Legacy statically linked Mesa clients do
not obtain new device authority through the old DRM syscalls. They need the new
SDK scene client for GPU use; an application that already has a CPU path can
continue using that path. There is no dynamic-link compatibility promise for
their old Mesa objects.

The distribution contains `cometlake-backend` and `cometlake-renderer` in one
authenticated manifest. Mesa and its DRM adapter are private to that renderer.
Do not add Mesa, family archives or `OSAIKU_SDK_CLOSURE` to client link steps.
The package provides its own build/authoring scripts, pinned dependency hashes,
budgets, hardware match and notices. Signing requires an explicit external key
or the explicit public test fixture mode. The fixture policy is isolated and
must not become a system publisher policy. Every changed payload requires a new
release number and ordinary review of the exact publisher.

The OS payload builder can copy an optional `.osadrv` and matching public
`.osadkey` into `Drivers/` beside the system release. These files are outside
OSAIBASE/RELEASE.SIG; staging neither imports them nor grants trust. Installation
uses the persistent driver catalog and retains the live package until safe
retirement. Updates, deactivation and removal take effect across restart; the
current process pair is not silently replaced.

The new device query `devices::gpu::query` returns cached versioned GPU facts
through operation READ_GPU (5). The existing device `Record` layout is unchanged.
Installed, selected and active releases are distinct; neither selection nor
heartbeat proves completed GPU work. Trial acceptance requires observed fences
and three presentations. Control STATUS/STOPPED coordinates S5 drain with the
exact backend identity; restoration, IRQ shutdown and resource retirement
precede completion. Missing proof retains resources instead of reporting a
successful stop.
