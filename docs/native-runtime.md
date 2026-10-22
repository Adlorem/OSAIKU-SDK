# Experimental native runtime foundation

`osaiku-runtime` 0.1.0 is a `no_std`, allocation-free crate with no external or
private OS dependencies. It is the canonical source for the x86_64 interrupt
calling convention, `ProcessId`/`ProcessEndpoint`, native error codes and the
initial syscall-number subset, session memory metadata, window broker status and UI event transport.
Its dependencies are the public sibling `osaiku-ui` and `osaiku-package` crates,
without UI compiler features. The OS ABI reexports these contracts; its
`user_rt` delegates traps to this implementation. The OS still owns admission,
pointer validation, permissions, scheduling and lifecycle policy.

The minimal single-window runtime is available for images already admitted by
OS deployment policy. General package registration, arbitrary privileges and
migration of every built-in application remain outside this subset. See
[window-hello](../examples/window-hello/README.md) for a complete SDK-only UI.
The smaller four-call native example remains a link/ABI example without a window.

## Hosted window application

Implement `hosted::WindowApplication` and call `hosted::run::<App>()` from
`_start` on the native target. The runtime reads `launch::current()` from the
kernel; applications do not declare a numeric `APP_ID`. The optional
`launched(&launch::Context)` constructor receives package, release, policy epoch,
permissions and exact process endpoint. These descriptive bytes do not transfer
authority. Window identities arrive separately via `submitted`. The trait provides initial state, event handling, rendering into a bounded output slice and an explicit
shutdown-readiness callback. One process owns one window session. State plus
host metadata must fit one 4 KiB session page; an oversized type is refused.

The runtime authenticates initd, validates OPEN/QUERY and presentation identity,
drains at most 16 events per query, and only delivers events naming the current
or preceding accepted submission with its original owner/window/revision.
Applications with dynamic rows must retain their original revision bindings;
this generic runtime does not reinterpret an old row against newer data.
The `submitted(identity)` callback supplies the kernel-issued identity after
successful acceptance, so the application can associate its prepared row/data
snapshot with that revision and retain the preceding accepted bindings. Never
infer this identity from a render counter. Failed renders and refused submissions
do not invoke the callback. The SDK-only counter example also uses it to match
input to its accepted views.
It validates query freshness before calling event handlers and advances metadata
only when they report a view change. Exhausted revisions refuse the query before
application mutation. A render failure or eight BUSY submission attempts produce
a decline, preserving the host's last valid view. Acceptance is not presentation;
the compositor remains responsible for the separate presented receipt.

The runtime owns its session through `SessionPage`, validates allocation and
release receipts, rejects replayed/mismatched teardown and drops/scrubs state
before explicit RELEASE. Process exit remains the terminal kernel reclaim path.
Protocol/transport corruption is fail-closed. No storage, monitoring, package
installation or update authority is granted by implementing this trait.
Shutdown readiness is supplied by the application, never inferred from its UI.

`WindowSession` also exposes the pure metadata adapter consumed by System Manager.
`compat` holds the versioned legacy OPEN/QUERY envelope used by the current OS
host (historical `DocumentEditor*` names). It contains no editor storage syscall
implementation. These experimental codecs are shared with the OS ABI, not a
second copy. New application authors normally use the hosted trait instead.

Declare `static VIEW: packaged::PackagedView = PackagedView::new()` and compile
HTML/CSS in `build.rs`. `encode` decodes that binary template, binds literal data,
sets valid focus state and emits bounded wire bytes. Its guarded scratch lives
in the process image, avoiding Document-sized temporaries on the 16 KiB native
stack. Do not submit its output after an error. This decodes a compiled binary,
not markup, during view updates; markup compilation runs only at build time.

## Available operations

On the freestanding x86_64 target:

- `log_write(&str) -> isize`: at most 160 UTF-8 bytes; native status, with zero
  meaning success. Longer messages return the invalid-buffer status.
- `process_self() -> Result<ProcessEndpoint, isize>`: the kernel-issued process
  ID and nonzero incarnation. Reused process IDs must retain that incarnation.
- `scheduler_yield() -> isize`: one scheduling yield. This low-level operation
  does not acknowledge shutdown readiness on an application's behalf.
- `yield_to(ProcessEndpoint) -> isize`: one direct transfer to the addressed
  instance (syscall 4), returning when resumed or activation is refused. The
  kernel checks activation permission separately from IPC permission and checks
  the target generation. This does not acknowledge shutdown readiness.
- `process_exit(u64) -> !`: the existing process exit/cleanup path.
- `shutdown_prepare_epoch() -> Result<Option<u64>, isize>`: query only the
  calling process's pending preparation epoch, without initiating shutdown.
- `shutdown_prepare_reply(epoch, ready) -> isize`: answer that epoch after
  checking application-owned state/work. The kernel validates epoch and caller;
  this grants no authority to initiate or approve the power action.
- `ipc_send(ProcessEndpoint, &[u8]) -> isize`: send exactly one message with the
  complete target incarnation. No automatic splitting, truncation or retry.
- `ipc_reply_and_yield(ProcessEndpoint, &[u8]) -> Result<(), ipc::ReplyError>`:
  send once, then directly hand off to the recipient only after successful send.
  `Send(status)` skips handoff; `Handoff(status)` means the reply was already
  sent. Never resend that reply to retry a failed handoff. This is not atomic.
- `ipc_receive(ReceiveMode, &mut [u8], &mut ProcessEndpoint) -> isize`: receive
  once, retaining the full sender address. Modes are `Blocking`, `Poll` and
  `Timed { timeout_us }`. Timed receive currently accepts 1..=200000 microseconds
  and requires the calibrated native clock; validation stays kernel-owned.
- `ui_action_take_for(window, &mut window::Event) -> isize`: take one event for
  a specific caller-owned window, so multiple windows cannot drain each other's
  focus/scroll events. Zero keeps the process-wide behavior below.
- `ui_action_take(&mut window::Event) -> isize`: take one event from the calling
  application's presented UI queue, without waiting. Zero means one event was
  copied; `SYSCALL_ERR_MAILBOX_EMPTY` means none. After any error the previous
  output is not a new event. No event injection or privileged shell wake API is
  exposed by this operation.
- `ui_content_submit(&window::UiDocumentWindowContentFrame, &[u8],
  &mut window::UiDocumentWindowChannelStatusRecord) -> isize`: submit one recipe
  to the calling owner's armed window. The kernel copies the bounded recipe
  before accepting it; it retains none of these borrowed pointers after return.
  The public call makes one attempt, without implicit yield or retry.

IPC payload/capacity must be 1..=160 bytes. The kernel still owns validation and
error precedence. Oversized buffers/messages are passed intact and rejected,
never silently shortened. `ipc_send` returns zero on success; `ipc_receive`
returns the positive delivered length. Only then are that prefix and sender a
new message. Never parse prior buffer contents after an error. Compare the full
sender endpoint with the one authenticated by your application protocol; a
matching PID alone does not authenticate a reused process instance.

Polling an empty mailbox returns `SYSCALL_ERR_MAILBOX_EMPTY`, a receive deadline
returns `SYSCALL_ERR_TIMEOUT`, and lifecycle interruption returns
`ipc::RECEIVE_INTERRUPTED as isize` (-64). Mailbox contention, stale endpoint,
permission denial and unsupported clock retain their distinct native statuses.
The public runtime performs one attempt and does not invent an acknowledgment
or retry. In particular, retrying a timed receive would start a new relative
deadline. Lifecycle preparation, retry/backpressure budgets and message
authentication belong to the application's protocol/host.

Content recipe length must be 1..=65536 bytes (`osaiku_ui::MAX_WIRE`); invalid
lengths are passed intact for kernel rejection, never truncated. A negative
return (including `SYSCALL_ERR_BUSY`) provides no new status record. BUSY consumes
no submission, so the application's host chooses a bounded retry/yield policy.
Recognized nonnegative returns are broker channel statuses: only status OK means
accepted. Check the returned record and expected transaction; acceptance does
not mean that the compositor has already presented the view. An invalid or stale
submission cannot authorize a different window or replace an accepted view.

`UiDocumentWindowChannelStatusRecord::matches_request(&request, owner)` checks
canonical request/status metadata, request ID/hash, app-instance identity and
the full trusted owner endpoint plus its numeric PID. Zero/kernel and unbound
owners are rejected. It does not select a successful phase: a canonical refusal
can still match the transaction. `is_submitted_for(&request, owner)` additionally
requires the successful SUBMITTED phase, ordered arm/submit generations and no
take/complete generation. Neither method authenticates a sender or proves
presentation. Check the syscall result first; for IPC, authenticate the sender
independently. Host/renderer policy and comparison with a previously retained
arm generation remain the host's responsibility.

All four OS application adapters use `is_submitted_for` with the kernel-issued
self endpoint retained at startup. The OS host uses `matches_request` while
retaining its fixed-service identity and expected arm-generation checks.

The OS adapter keeps its existing shutdown checkpoints, receive-interruption
handling and busy-submission retry policy above the shared IPC/trap. Endpoint
blocking/polling receives retain their checkpoint/retry loop; timed receive
retains its one-attempt behavior. Those policies
are not duplicated or automatically assumed by the public foundation. A future
lifecycle adapter must preserve their meaning, including application cleanup.

`ipc::send_then_handoff(send, handoff)` is the shared sequencing adapter used
by the native operation and OS lifecycle-aware wrapper. Each closure runs at
most once; only status zero permits progression. In the OS, the handoff keeps
its shutdown checkpoint after successful send. All four built-in apps use this
path for teardown receipts, and Manager/Document also use it for their ordinary
host replies, retaining distinct send/handoff failure exit codes. File Manager
and System Update ordinary replies retain their mailbox-full retry policy.
Editor/file/save exchanges that require scheduler-only yielding keep it: a
direct host handoff would wait for host activation and could block the next
input. The host protocol must choose the appropriate scheduling operation.

## Session memory

`session` owns the existing ABI v1 `ProcessSessionMemoryRequest` (96 bytes) and
`ProcessSessionMemoryReceipt` (160 bytes), their flags, hashes and canonical
validators. Both have native C layout with 8-byte alignment; this transition
uses typed syscall buffers, not an IPC serialization format. OS ABI reexports
the exact definitions. The initd-only address-free observation contract and its
query syscall remain in the OS; this public module grants no observation rights.

`ProcessSessionMemoryRequest::map(request_id, session_key, byte_len)` builds
and hashes a canonical request for 1..=4096 bytes; the usize length is checked
before conversion and is never truncated. `::release(request_id, &mapping)`
builds an exact release request from a retained canonical MAP receipt, rejecting
corrupt, rejected or already-terminal receipt metadata. Both return `Option` and
perform no syscall, allocation, automatic ID increment or retry. They do not prove
that a mapping is still live or that a caller owns it; kernel checks still apply.

`receipt.matches_request(&request, owner_pid)` validates both records, request
ID/hash/session-key echoes and the trusted numeric owner PID. A mapped result
must answer MAP and cover the requested length; a released result must answer
RELEASE and match all three mapping identity fields. A matching rejection is
still a refusal, with no address. Callers must independently check the syscall
return and expected status/decision before dereferencing or changing state.
The four built-in apps use these constructors and matching checks. MAP responses
are checked against the PID learned at startup before accessing their address;
RELEASE matches the owner in the retained kernel-issued mapping receipt.

On the native target, `unsafe session_memory_transition(&request, &mut receipt)`
invokes syscall 334 once. MAP requests 1..=4096 bytes and, if admitted, receives
one zeroed writable non-executable page. Kernel policy chooses the process-local
virtual address, frame, quota, request order and mapping identity. RELEASE names
the exact session key, mapping ID/generation/hash and requests zero bytes.
Applications cannot select physical memory or another process's mappings.

The raw transition is an unsafe memory-lifetime boundary. Before RELEASE,
detach the session, end all references and outstanding
accesses, and destroy any live values in the page. No continuation may touch it
after success. Place both control records in the registered process image or
stack, outside session mappings; the kernel checks this before unmapping.
The OS user-runtime adapter now marks this operation unsafe as well, and its
four existing application consumers document the allocation/teardown preconditions.

Negative native errors do not provide a new receipt. Nonnegative results still
require validating receipt status/decision, request ID/hash, session, trusted
owner/address-space identity and the exact mapping. Only a successful matching
MAP gives a usable address; check alignment and page length before constructing
references. RELEASED and REJECTED records contain no live address. A metadata
hash is not proof that a mapping belongs to this caller or remains live. The
runtime does not retry or automatically advance request IDs. Lifecycle hosts
still own cleanup, response authentication and failure handling.

### Typed page ownership

`SessionPage<T>` owns one initialized value in a mapped page. Its unsafe
`adopt(&request, &mapping, syscall_status, owner_pid, value)` checks the MAP
outcome, canonical response/request matching, layout and requested capacity,
then verifies all 4096 bytes are zero before initializing the value. Only a
nonempty type fitting the page and its alignment can be adopted. Invalid
status/metadata is rejected before accessing memory. A failed adoption drops
the supplied value normally and leaves the allocation untouched.

The caller must supply a real live allocation with exclusive ownership, no
aliases or existing values, and keep it mapped until the handle is destroyed.
Hashes cannot establish this, so adoption remains unsafe. Keep the handle and
control buffers outside the owned page. The handle is not Copy/Clone and gives
only borrowed `value()`/`value_mut()` access plus read-only mapping metadata.

`retire(self)` consumes the owner, runs the value's destructor and zeroes the
whole page with volatile writes before returning copied mapping metadata. All
borrows must end before retirement. Use that metadata for explicit RELEASE,
then validate its response. Dropping the handle also destroys/scrubs the value
on normal return, but performs no syscall: it leaves the allocation for explicit
release or process cleanup. Forgetting the handle skips destruction and cleanup.
A panicking destructor follows the target's panic policy; cleanup completion is
not guaranteed through a panic. There is no hidden retry or shutdown exchange.

System Manager is the first consumer: its single admitted session is an
`Option<SessionPage<ManagerState>>`, replacing the raw linked-list node and
separate count. It detaches/retire-scrubs before RELEASE and creates a released
teardown receipt only after the successful kernel reply. Other app adapters
retain their existing mapping ownership for incremental migration.

## Window metadata and event identity

`window::UiDocumentWindowChannelStatusRecord` is the existing ABI v10 record,
136 bytes with 8-byte alignment. Its legacy document-window naming is retained
for compatibility; the broker also serves generic SDK content. The public module
owns its constants, little-endian codec, FNV digests and canonical validator.
The OS reexports that exact type and implementation, rather than maintaining a
second copy.

`UiDocumentWindowPresentRequest` (160 bytes) and `UiDocumentWindowContentFrame`
(168 bytes) also have one public definition, codec and hash implementation.
Their legacy field names, hash domains and ABI version remain unchanged. A frame
exceeds the 160-byte IPC limit and is passed through `ui_content_submit`, with the
recipe in a separate byte slice. Its 32-byte inline body remains the legacy
Document slot; generic UI uses content kind 4 and the recipe hash. Codecs check
exact length only, including unsupported/malformed records; hash computation
does not grant validity or permission.

`UiDocumentWindowContentFrame::for_ui(&request, encoded_recipe)` builds a kind-4
frame for declarative UI with an empty legacy editor body, clean/never-saved
compatibility state and no assistant report. It copies the request's identities,
revisions and state hashes, zeroes the inline body/reserved fields, and hashes
the exact recipe bytes. It returns `None` for malformed request metadata, a
nonempty expected legacy body, or a recipe outside 1..=MAX_WIRE bytes. This is
the path consumed by System Manager after authenticating its current session.

The builder does not parse the recipe or allocate a Document on the native
stack. Use the UI compiler/encoder to prepare bytes; the kernel still validates
the full copied recipe before admission. A returned frame is not an admission
receipt. Editing applications that carry legacy body/save state retain their
OS adapter until their own migration; this helper must not replace that state
with a fabricated clean state.

`ui_document_window_present_request_well_formed` checks v10 metadata, required
IDs/generations, derived document identity, semantic generation and request hash.
It does not decide which application IDs are admitted or authenticate the
sender/current armed transaction. OS canonical validation adds its existing app
policy; private editor/binding matching remains separate. Public
`compute_window_document_id` and `compute_window_body_hash` preserve the legacy
hash domains and are also the OS editor's canonical implementations.

Editor-dependent constructors and canonical admission checks remain in the OS.
The OS constructors are now free functions named
`ui_document_window_present_request_from_sources`,
`ui_document_window_present_request_from_window_sources`,
`ui_document_window_content_frame_from_sources` and
`ui_document_window_content_frame_from_request_and_editor`. This separates
private editor/surface types from the public wire representation. Current OS
consumers still check those exact sources before submission. Do not manufacture
identities or bypass lifecycle authentication because the fields are public.
Lifecycle exchanges and receipt delivery/authentication still require extraction
before an external app can own a complete window.

`ui_content_declined(&request)` encodes the existing 24-byte `OSUINO01` response,
bound to request ID and hash. It only creates bytes: the lifecycle host must
authenticate the sender and match the current request before cancelling an
update. This response is not a presentation receipt.

`UiDocumentWindowPresentationReceipt` has a public 160-byte, 8-byte-aligned
definition, codec, provenance/record hashes and
`ui_document_window_presentation_receipt_canonical` validator. It preserves the
existing renderer-sequence envelope, with prepared, committed and rejected
states. Canonical prepared metadata is not committed presentation; rejection
cannot claim visual generations. Committed records require a supported sequence
length and exact, non-overflowing first-to-final generation range. FNV does not
authenticate who produced a record.

The historical v10 sequence counts (26 tiles / 52 cells) are now canonical SDK
constants. OS compile-time assertions check its renderer geometry against them.
These are compatibility constraints, not a drawing API for applications or the
number of nodes in a declarative view. The OS renderer constructor is now
`ui_document_window_presentation_receipt_committed`; it still derives and checks
the frame/manifest/sequence using private renderer state. OS frame matching and
kernel verification against the actual completed sequence remain required.
The public codec adds neither a receipt query syscall nor permission for an app
to complete a renderer transaction. Do not treat a decoded/rehashed receipt as
evidence that a view reached the display.

For status records, `decode` checks exact byte length only. Call
`ui_document_window_channel_status_record_canonical` to verify supported version,
flags, status/reason, nonzero participant identities, transition ordering and
hashes. A canonical record is not proof of caller authority, current transaction
or successful presentation. Match the full authenticated application endpoint,
window instance, request ID/hash and issued `next_public_generation` against the
expected transaction. FNV is corruption/provenance binding, not authentication.
Submission acceptance remains distinct from a compositor presentation receipt.

`window::Event` and `window::Identity` reexport the UI crate's exact types. Events
are 48 bytes and contain the source owner endpoint, window, incarnation and
revision, followed by element, kind, value and action. The kernel routes events
to their owner; applications must apply an event using the retained bindings
for that source revision. A queued row click must not be reinterpreted using a
later refresh's row data. Receiving an event does not grant shell operations or
permission to submit another owner's content.

Host builds expose the ABI and process types for tooling/tests; the safe native
operations are absent. `raw::syscall` and `raw::exit` are unsafe x86_64 adapter
interfaces. Never call them on Windows/Linux or from kernel mode. Callers must
be OSAIKU user processes and uphold each syscall's pointer/length/lifetime and
aliasing contract through any blocking/rescheduling. Numbers and endpoints grant
no authority. The trap uses `int 0x80`, RAX for number/result, and RDI, RSI, RDX,
R8, R9 for arguments. Unused arguments are zero; memory, flags and C caller
clobbers are preserved in the assembly contract.

## Application control-message routing

`application` publishes the existing report/exit commands and running/invalid
replies. Their historical `document_app|...` bytes are unchanged and shared by
all built-in apps; OS legacy names reexport these constants. OS-only process
launch requests, editor commands and application-specific operations are not
exported by this module.

Configure `ApplicationHost` from trusted local boot/session state, never message
contents. `FixedService(ProcessId)` is only for a service whose PID cannot be
reused: the System Manager and System Updates adapters use it for initd, retaining
their existing PID-only sender policy. `Instance(ProcessEndpoint)` requires an
exact PID/generation match and refuses generation zero. Both refuse kernel PID
zero. Selecting FixedService does not query the registry or prove a PID is fixed.

`host.classify(sender, bytes)` takes the sender returned by kernel IPC. It first
rejects empty or over-160-byte messages, then checks the sender policy before
returning a borrowed `ApplicationMessage`: Report, Exit, Present, Teardown or
Other. Report/exit require exact bytes. Present requires the current envelope's
size, magic and version; teardown preserves the existing 156-byte routing rule.
Unknown presentation versions and private commands remain Other for the app's
existing handling. No payload buffer is copied and no allocation is needed.

A routed envelope is not canonical window/session metadata or permission to
release memory. The adapter must still decode, validate current ownership and
state, then act or refuse. Routing performs no receive, response send, retry,
yield, mutation or cleanup. Existing power-checkpoint and one-slot IPC scheduling
remain in the OS receive/send wrappers. All four built-in applications use the
shared classification for lifecycle commands. File Manager retains navigation
before presentation/teardown and its timed summary polling. Document processes
its separately authorized status and inputd paths before initd lifecycle
classification; physical editing still requires a redeemed kernel input permit.
Its file/save/editor handlers retain their original order, canonical checks,
reply scheduling and buffer scrubbing. These private operations remain OS
adapters and are not exported as generic SDK lifecycle authority.

## Session teardown messages

`lifecycle` owns the v2 `WindowSessionTeardownNotice` (156 wire bytes) and
`WindowSessionTeardownReceipt` (120 wire bytes), their constants, hashes and
canonical validators. The OS ABI reexports the same definitions; shell close
requests and compositor/window-registry decisions remain OS-owned. This does
not yet provide a complete public application lifecycle or receipt dispatcher.

Use `encode` and `decode`, never a native struct byte cast: the notice occupies
160 bytes in memory including padding. Decode validates exact length only.
Canonical validation checks shape and unkeyed FNV provenance, not authorization.
The notice's `owner_process_id` is a numeric PID bounded by `u32::MAX`, not an
encoded endpoint. IPC endpoints and their generations are checked separately.

Before acting, authenticate the lifecycle host's IPC endpoint, reject answered
notice IDs, compare the local application/owner/window/session identity, and
check `session_key` against `window::compute_window_document_id` using the six
instance/surface/binding fields. The teardown identity hash merely binds the
supplied fields; it does not derive that key or establish freshness. Preserve
exact kernel-issued mapping identity through teardown. Finish all page access,
references and value destruction before unsafe `session_memory_transition`
RELEASE, with request/receipt buffers outside the released mapping.

`validate_window_session_teardown_notice(notice, owner_app_id, owner_pid,
last_answered_notice_id)` implements the common preflight used by all four
built-in applications. It borrows the decoded notice (or accepts `None` for a
failed decode), checks canonical metadata, then rejects replayed IDs, then
compares the trusted owner and derives the session key. It does not allocate,
mutate the notice/cursor, look up sessions or call the kernel. The owner arguments
and answered-notice cursor must come from trusted local state.

`TeardownNoticeError` supplies the existing wire status/reason and
`consumes_notice_id()`. Invalid shape and replay do not change the cursor;
a canonical, fresh identity mismatch consumes its notice ID when answered.
After successful preflight, the application must still locate the exact stored
session and mapping. Unknown sessions consume the ID when refused too. A
successful validation neither authenticates the sender nor proves a live session
exists. Callers retain sender authentication, cursor updates, exact lookup,
detach/scrub/release and receipt delivery. There is no implicit retry.

`WindowSessionTeardownReceipt::released(notice, mapping, remaining_count)`
constructs and seals the diagnostic success response. Pass the original MAP
receipt copied out of the session page before unmapping, after successful
RELEASE. The builder requires canonical notice/MAP metadata, matching numeric
owner PID and session key, and a nonzero mapping identity. A terminal RELEASE
receipt is not that retained MAP receipt and is rejected. The builder neither
unmaps memory nor verifies that RELEASE actually happened.

`WindowSessionTeardownReceipt::rejected(notice, status, reason, remaining_count)`
constructs a sealed refusal with no mapping claim. It accepts `None` for failed
decode and malformed notices for diagnostic echoes. Zero echo fields retain the
v2 placeholder 1; nonzero values are copied unchanged. Unrepresentable identity
(such as a PID above `u32::MAX`) or invalid result metadata returns `None`, with
no PID truncation. Both builders return `Option`, allocate nothing and perform
no IPC or cursor update. All four built-in apps now use these shared builders;
encoding failure retains their existing fail-closed process-exit path.

A released receipt names a nonzero mapping ID/generation/hash. A refusal names
no mapping. Both are diagnostic application reports; the lifecycle host still
checks the kernel registry to establish terminal state. Neither hashes nor the
`WINDOW_DESTROYED` flag prove that a compositor operation actually occurred.
Current built-in applications retain their existing sender/replay/identity and
mapping checks; this extraction changes no admission or release policy.

## Native build from this repository alone

The pinned toolchain is `nightly-2026-04-19`; its `rust-src` component is required
for `build-std`. Use `targets/x86_64-osaiku-user.json`. The shared
`runtime/linker.ld` places `_start` and static application sections at
`0x4e000000`, with 4 KiB alignment. It does not provide allocation, startup
arguments, a host C runtime or permission to launch a package.

Use `ld.lld` on PATH or set `CARGO_TARGET_X86_64_OSAIKU_USER_LINKER` to the
toolchain's `rust-lld` (`rust-lld.exe` on Windows), located under the directory
printed by `rustc --print sysroot`, in `lib/rustlib/<host-triple>/bin`.
`rustc -vV` reports the host triple. From the SDK root:

```text
cargo build --offline --locked --release -p osaiku-native-hello --features native --target targets/x86_64-osaiku-user.json -Z build-std=core,compiler_builtins -Z build-std-features=compiler-builtins-mem -Z json-target-spec
```

The binary is `target/x86_64-osaiku-user/release/osaiku-native-hello` when no
target-directory override is active. It obtains its endpoint, writes a log
message, yields and exits. It has no window and does not speak the current
built-in application launch protocol. **Do not replace an installed application
with it or execute it as a host program.** Loading/running it through the public
application lifecycle is a later integration step.

In the OS checkout, `tools/ci/test-sdk-native.py --output <evidence-directory>`
copies only public SDK inputs into an isolated temporary directory, removes
OS-specific environment settings, verifies every Cargo dependency stays inside
that export, builds offline, and inspects the ELF's load bounds, entry and W^X.
Pass `--example window-hello` to build the hosted UI example instead.
It records source hashes and the binary hash. It never executes the ELF and is
not guest/physical qualification. Portable ABI tests run in the normal host
gate; final native guest acceptance follows the OS test-execution policy.

The OS's old `targets/x86_64-osaiku-user.json` remains a compatibility copy for
existing commands; an ABI host test requires it to match the public target.
Production QEMU builds and `OSAIKU_USER_TARGET` use the SDK file. System Manager
uses the public linker script directly. Other built-in app/service linker
scripts retain their existing roles until their own migration.

## License and scope

The experimental `surface` module introduces syscall 368 for bounded immutable
pixel publication. A handle binds the authenticated process, existing window,
incarnation, compositor epoch and revision. Uploads are consecutive and at most
64 KiB in surface ABI 6; seal publishes validated pixel, semantic and explicit material data
together. Applications may query their own metadata; pixel reads require the
supervisor-admitted compositor. Binding/revoking that compositor is initd-only.

Surface ABI 6 supports incremental armed frames. `SurfacePublisher::from_delta`
accepts a `DeltaOffer` containing the current `FrameOffer`, an exact immutable
base `Handle` and `DirtyRows`. BEGIN_DELTA (18) rechecks base geometry and identity.
UPLOAD sends dirty rows; COPY_BASE (19) copies clean rows inside the kernel, with
an empty payload and byte offset/length in `Request.offset`/`server_endpoint`.
BEGIN_STREAM (20) takes the same offer with an empty bitmap; the kernel records
which rows were actually uploaded as the candidate advances. Both paths require
consecutive full rows, at most 64 KiB per step. SEAL checks complete coverage and
damage covering uploaded rows before atomically replacing pixels and metadata.
`PublishStep::Copied` reports bounded copy progress.

`HostedRaster` renders each band once and computes a noncryptographic 128-bit
fingerprint, then sends UPLOAD or COPY_BASE immediately. Bands are at most 32
rows and 64 KiB; retained fingerprints use 16 KiB. Busy retries reuse the prepared
band. Fingerprints are change hints, never authentication or permission checks.
Geometry/theme/tenancy changes or failed publication invalidate the base. A
stale delta BEGIN allows one full-frame fallback; admitted failures cancel the
exact candidate. Unchanged images upload zero pixel bytes and use empty damage,
but still copy a complete immutable base inside the kernel and seal new semantics.
Up to 16 disjoint dirty runs are published; overflow conservatively extends the
last rectangle. Native compositord patches only these rows when its raster is
based on the exact previous handle, otherwise it rebuilds the complete raster.
Reduced work is structural; hardware latency/CPU measurements remain outstanding.

The OS's bounded spare pool reuses only fully released storage, never a visible
or pinned image. The 184-byte `WorkSnapshot` adds global-only `spare_bytes`,
`backing_reuses` and `backing_allocations`; copied base bytes contribute to
`payload_copy_bytes`. Rebuild old producers/consumers for ABI 6.

`surface::bind_server_with(endpoint, exchange)` is the supervisor's bounded
startup adapter. It requires a nonzero PID and instance generation, queries the
current epoch, then binds that exact instance in that epoch. Both receipt echoes
are checked. It performs at most two exchanges, never retries and never revokes
an existing binding on failure. A receipt error after BIND does not imply that
the binding was rolled back; lifecycle policy belongs to the supervisor. The
kernel still checks caller authority, consoled identity, generation and live
process state. An application cannot gain authority by using this helper.
Production initd now uses it before starting the console; consoled starts at
generation one, while other fixed services retain their legacy initial generation.
This startup binding does not implement compositor restart or move rendering.
Current surface operations are restricted to the main application thread.

`osaiku_ui::paint::Scene::prepare` computes into an application-owned static
`Layout::EMPTY` workspace and borrows it with the immutable document. This avoids
returning a multi-kilobyte layout on the 16 KiB native thread stack. `raster::rows`
paints complete BGRA row bands, each at most 64 KiB, using the existing Polish
glyph repertoire. `surface::Publication::prepare` derives clipped actions and
labels from that same scene; material regions are explicit arguments.

`osaiku_runtime::surface::publish_with` accepts the scene, authenticated target,
revision, material regions, private `Publication::EMPTY` workspace and band,
plus a synchronous exchange callback. On the native target the callback calls
the unsafe `transition` syscall wrapper. The producer validates metadata before
BEGIN, uploads consecutive bands and seals the matching metadata. It does no
hidden waiting/retry. Failed uploads/seals attempt cancellation; `Cleanup`
returns the outstanding handle when cleanup itself was refused. The caller
must retry cleanup or retire the window before starting another candidate.
Large documents, layouts, metadata and bands belong in guarded static/process
storage, not local stack variables. Metadata preserves placements and scroll
extents. Chart series and ancestry remain in the authenticated frozen recipe;
a complete self-contained semantic record is still migration work.

For cooperative event loops use `SurfacePublisher::new` with the same inputs.
It borrows the immutable scene, metadata and band for the entire operation;
`step_with` performs at most one syscall: BEGIN, one raster/upload band, or SEAL.
Scratch must fit at least one complete row and be at most 64 KiB. A nonmultiple
of the stride leaves an unused tail. Rasterization initializes each band;
successful uploads retain private scratch until the next step. Refusal, seal
and cancellation clear it. The returned `PublishStep` identifies completed work.
The publisher itself is a small reference/cursor object with no heap allocation.

Schedule event/other work between steps, explicitly retry transient refusals,
or use `cancel_with` to abandon an obsolete frame. A failed step keeps its upload
cursor; a lease-blocked SEAL can be retried without rasterizing/uploading again.
A failed cancellation retains its handle. Epoch revocation makes that handle
stale and cleanup succeeds without retaining a buffer. A malformed receipt stops
further publication; cleanup uses only a previously authenticated handle. If
BEGIN returned an untrustworthy handle, retire/reconstruct the window tenancy.
Dropping the Rust value does not send CANCEL. A successful SEAL is not proof of
scanout and cancellation cannot undo a publication already accepted by SEAL.
`publish_with` remains a synchronous convenience wrapper over this state machine,
with one cleanup attempt on failure; it does not schedule application work.

The kernel binds BEGIN to the admitted window's resize generation and dimensions.
If those change before SEAL, sealing returns `SYSCALL_ERR_STALE_ENDPOINT` while
retaining the candidate for explicit cancellation. Cancel that candidate and
reconstruct the frame for current geometry; do not retry its SEAL. Returning to
the same dimensions does not revive a candidate from an earlier generation.
Ordinary moves do not invalidate content. This guard covers BEGIN-to-SEAL;
the geometry offer below additionally covers query-to-BEGIN. Binding the frame
to its document request through the frame offer below is available; the presentation/input barrier is still required before desktop
migration. A stale SEAL is not itself a cleanup
acknowledgement; only CANCEL or lifecycle retirement releases that candidate.

Call `geometry_with(target, exchange)` before layout to fetch a `GeometryOffer`
for your exact window/incarnation/current epoch (obtain the epoch using QUERY).
The 80-byte integer-only record describes the admitted window, resize generation,
frame layout hash and exact content rectangle beneath the title bar. Use
`offer.content_extent()` as the viewport for the scene, then use
`SurfacePublisher::from_offer` instead of `new`. Smaller or larger surfaces are
refused. Pixel, damage and semantic coordinates start at `(0, 0)` within content;
only the compositor applies the window-local `offer.content.x/y` placement.
Its first step sends BEGIN_OFFER, which compares the offer with current authenticated
geometry before admission/allocation. A changed epoch/generation/size/content slot requires a
fresh query and scene; a refused BEGIN_OFFER has no candidate to cancel.

Holding an offer reserves no surface and grants no new authority. It may be reused
for subsequent revisions while geometry is unchanged; every BEGIN_OFFER is still
validated. A geometry-only offer uses the caller's monotonic content revision;
use the frame offer below for the kernel-issued request revision. The old BEGIN remains a low-level transport
path; it does not protect layout performed before BEGIN from a geometry change.
Ordinary applications can query only their own window, including before the first
pixel publication; a geometry query uses bounded syscall scratch but no surface
allocation. Querying or beginning a frame does not acknowledge its presentation.

Frame metrics are captured at window admission and copied into the immutable
scene. Transport uses the same bounded `decoration::frame::content_bounds`
function as SDK frame slots; it does not derive the responsive render graph or
run scene layout in a syscall. A scene whose frame-layout hash no longer matches
the admitted window is refused until its window metrics are reconstructed.
This experimental record replaces the earlier 56-byte envelope: old payload
sizes fail closed. It is not a released/stable surface ABI.

`frame_offer_with(target, request_id, request_hash, exchange)` obtains a 112-byte
`FrameOffer` for the exact currently ARMED render request. It combines the content
geometry with a 32-byte `RenderRequest`: request ID/hash, arm generation and the
kernel's `next_public_generation`. Use `SurfacePublisher::from_frame`, which takes
the issued revision from that offer rather than a caller-selected revision.
BEGIN_FRAME and SEAL both recheck the armed request; re-arming, changing or closing
it invalidates old work. A failed SEAL still requires cancellation of its candidate.

For an application's own event loop, use
`WindowSession::frame_offer_with(request, app_id, owner, exchange)` after
authenticating the IPC sender. It checks the request hash, admitted application
ID and retained session state before any transport call, then queries the epoch
and the exact armed offer. Supply the image's application ID and the endpoint
from `process_self`, not identities taken from untrusted message fields. The
helper performs at most two calls, with no retry, allocation or state mutation.
An epoch-query refusal or corrupt receipt stops before the offer query; a race
with re-arm/revocation is rejected by the following transport transition.
Decode the frozen recipe and prepare a `Scene` at the offered content extent,
then use `SurfacePublisher::from_frame` and service scheduling/cancellation
between its bounded steps. Keep large document/layout/metadata/band storage
outside the application's 16 KiB stack. Sealing still requires a future broker
completion/presentation integration; it must not call the hosted `submitted`
callback or enable input on its own.

The request binding is retained atomically with the sealed pixels and metadata.
`frame_request_with(handle, exchange)` returns it to the owner or admitted server,
including after the live channel changes. Unbound legacy/geometry-only surfaces,
foreign windows and stale handles are refused. A retained binding is evidence of
which request produced the snapshot, not a grant to deliver input or perform an
action. FRAME_OFFER uses the request handle's revision field for the expected
request ID and offset for its hash; BEGIN_FRAME uses revision for the issued frame.

This protocol does not consume or complete the broker transaction, acknowledge
scanout, or route input. The hosted application loop still uses its recipe path;
integration must retire/complete that request coherently with presentation. A
sealed request revision cannot be republished as a different arm of the same
revision. Full compositor migration and transaction recovery remain pending.

This limits work per call, not elapsed time. Document decoding/layout and initial
semantic preparation still happen before stepping. A host application must
integrate scheduling/cancellation into its event loop; the existing desktop does
not yet use this transport. No latency improvement is claimed from these bounds.

Server-side `SurfaceReader` reads incrementally: `start_with` acquires a per-window
lease and fetches validated metadata, `next_with` copies at most 64 KiB, and
`finish_with` releases only after all pixels were read. `cancel_with` releases
an incomplete read; errors retain the reader for retry. Scratch must be at most
64 KiB and hold at least one pixel. The exact handle remains stable while leased:
producer SEAL returns Busy but other windows can continue. Only the admitted
server can acquire/release leases. Keep output private until successful finish;
completion of a copy does not declare it visible or authorize input. Dropping
the reader does not issue a syscall. Teardown/epoch revocation reclaim leases;
the independent watchdog remains part of the unfinished server migration.

For request-bound content, construct
`SurfaceReader::for_frame(handle, expected_request)` using the transaction from
authenticated broker state. `start_with` then performs ACQUIRE, FRAME_REQUEST
and METADATA, in that order, within one lease. It compares all four retained
request fields (ID/hash, arm generation, public revision) before exposing
metadata or allowing pixels to be read. An unbound surface is refused. The live
channel may already have completed; verification uses the immutable binding
retained with the sealed image. The lease prevents a producer from replacing that
image during verification or incremental reads. A preparation refusal clears the
metadata in place and retains the acquired lease for explicit retry/cancel;
retry does not acquire a second lease. The bound start costs one additional
syscall and 64 accounted copy bytes for the 32-byte binding. It does not certify
current geometry, visibility, scanout or input authority, and a supervisor epoch
change invalidates the lease as before. Out-of-sequence start calls preserve the
caller's existing metadata and return an invalid-state error.

`frame_snapshot_with(target, exchange)` discovers the latest sealed bound image
of an exact owner/window/incarnation/epoch without knowing its kernel-issued
serial. Only that owner or the admitted server may query it. FRAME_SNAPSHOT (17)
returns a 160-byte `FrameSnapshot` with the handle, retained render request and
geometry saved with the image. Candidates, unbound publications and stale targets
are refused. The query allocates no pixel storage and takes no read lease; it
does not enumerate windows or grant read authority to an application.

Compare the returned request with the transaction the server intends to consume,
then use `SurfaceReader::from_snapshot(snapshot)`. It performs the existing bound
lease/request checks and additionally checks metadata extent against the saved
content geometry. Replacement between lookup and ACQUIRE is refused through the
exact handle; once acquired, the image remains immutable until release. A resize
after publication does not rewrite the descriptor: presentation must separately
validate current geometry and epoch. Completion of lookup/read is not broker
completion, presentation acknowledgement or input permission. Successful lookup
adds one dispatched operation and 320 accounted payload-copy bytes.

`SurfaceReceiver::new(snapshot, metadata, pixels)` combines the bound reader with
server-private cache storage. The caller supplies an exact-size pixel buffer and
separate metadata; neither may alias the currently displayed cache. It allocates
nothing, and borrows both buffers until the receiver is dropped. `step_with`
prepares the lease/request/metadata (at most three calls), then performs one READ
of at most 64 KiB per step directly into its destination, and finally one RELEASE.
The caller can schedule input, another window or cancellation between steps.
Refusals preserve the phase and cursor, including RELEASE backpressure after all
pixels have arrived; retrying release does not copy pixels again.

Only `received()` after `ReceiveStep::Complete` returns an immutable
`ReceivedFrame` view of the descriptor, metadata and pixels. Before completion or
after cancellation it returns None. Completion is idempotent without further
syscalls. `cancel_with` explicitly releases any acquired lease and discards the
result; failure retains cleanup state. Dropping never performs a syscall. Keep
the supplied buffers private after cancellation/drop: their partial contents are
not a received frame. Cancellation does not synchronously scrub an entire image
of up to 64 MiB. The caller's storage owner must handle reclamation and prevent
cross-owner exposure. The receiver occupies less than 512 bytes, excluding the
caller-owned buffers, and adds no transfer/queue/storage-allocation ABI.

This supplies a complete cache candidate, not a scene commit. Preserve the old
displayed cache while receiving into separate storage. Before promoting the new
copy, the compositor still needs the pending scene/geometry/epoch and input
barriers; the receiver does not acknowledge scanout or authorize actions. Its
bounded operations do not establish a wall-clock latency guarantee.

`surface::work_with(target, exchange)` queries `QUERY_WORK` diagnostics for an
owned window. A zero target requests the supervisor/server-only global aggregate.
Counters report dispatched calls, attempted uploads/reads, successful pixel
bytes, payload copies through kernel scratch, publications, cancellations and
lease-blocked seals. Queries exclude themselves; early pointer/endpoint/scratch
refusals are outside dispatch accounting. Reject measurements with nonzero
`saturated` or `incomplete`; global history survives teardown but window history
ends with its tenancy. Current/peak pixel charges exclude metadata, scratch and
allocator overhead. They are not process memory or latency measurements.
`FullFrameWork::calculate` estimates full-frame row-band upload/read calls and
pixel copies; damage metadata alone does not reduce those uploads.

`hosted::run::<A>()` publishes application-rendered surfaces in
the actual SDK event loop; `run_with_surfaces` is a compatibility alias.
It authenticates the host/session, renders a frozen
recipe, gets the armed offer, then decodes, lays out and rasterizes in the
application address space through `HostedRaster`. It yields after BEGIN,
each 256 KiB of successful uploads and every Busy retry. A publication gets
at most eight Busy retries in total; it does not process more application input
mid-frame. Other processes can run at these checkpoints and timer preemption.
This byte budget is not a calibrated 1 ms budget or a latency qualification.

`HostedRaster` owns guarded static scratch (Document, Layout, metadata and a
64 KiB band). Never put it on the native 16 KiB stack. `publish_with` exposes
the same orchestration with explicit transport/checkpoint callbacks. Invalid
views/offers allocate no candidate; failure after BEGIN attempts one CANCEL.
Recursive/concurrent scratch use returns Busy without waiting. A cleanup refusal
retains the exact handle in `RasterError::Publish(PublishError::Cleanup)`;
receipt corruption or checkpoint failure requires caller recovery.

After SEAL the native loop submits the same recipe to the existing host and
requires the accepted owner/window/incarnation/revision to equal the sealed
handle before calling `submitted`. A surface failure declines the request;
uncertain receipts or refused cancellation terminate the process for reclamation.
Host admission refusal declines the transaction. An authenticated, still-armed
BEGIN_FRAME may retry the same public revision with a fresh serial; old pinned
images remain immutable. Plain BEGIN still requires a strictly newer revision.
This is not transparent recovery or compositor supervision. `submitted` still
means recipe acceptance, not display acknowledgement.

Surface ABI version 3 atomically includes the frozen source hash, element labels
and integer layout placements/scroll extents. Metadata transfers allow at most
32 KiB, while pixel bands are at most 64 KiB (up from 16 KiB in version 2).
Rebuild producers and consumers together; mismatched versions are refused.
Ordinary shipped OS applications
use this publication path. The compositor adds window chrome to their pixels;
the host retains an immutable pin for presentation, observation and queued input.
It reconstructs the supplied layout with bounded validation instead of reflowing
the content. A resize clips the old image and hit regions until a new client
publication arrives.

UI wire version 6 binds the sealed image, semantic identities and events to the
same public compositor epoch. `Identity` is 40 bytes, `Action` 48 bytes and
`Event` 56 bytes. Rebuild applications and the OS together. After a server
restart the host discards old image/layout pins and requests fresh publications
from the still-running clients; application-owned document models survive.
The hosted loop clears previous input identities when its sealed epoch changes.
Ordinary window policy, including resize preview, now runs in `compositord`.

This remains an in-progress integration, not full UI isolation or a stable ABI.
The kernel retains resource admission and a validated scene/query checkpoint;
privileged dialogs and remaining metadata integration still require work.
Functional recovery checks do not qualify latency or hardware responsiveness.

Runtime code and the extracted process-address, session-memory, teardown messages,
legacy OPEN/QUERY command/state/reply codecs and validation, window metadata and
constant definitions use the SDK's
MIT license (OSAIKU contributors). The extraction selects the MIT option of the
OS workspace's MIT OR Apache-2.0 declaration. The new trap implementation follows
the existing calling convention; no third-party runtime, allocator, cryptography
or firmware source is imported. The UI crate now owns the unchanged pre-rasterized
DejaVu Sans Mono 2.37 coverage previously stored in the OS ABI. Its attribution,
source digest and repertoire remain in `ui/src/font_atlas.rs`, with the complete
upstream notice in `ui/licenses/DejaVu.txt`. The OS glyph API is a compatibility
facade, not a second atlas. Target and linker inputs are project-owned
build configuration. This does not constitute an audit or release of unrelated
private OS modules. Unknown UI wire majors remain independently rejected.


## Launch policy and public services

`launch::Context` version 1 is a 256-byte, integer/byte-only record returned by
syscall 394. `launch::current()` correlates its endpoint with `process_self()`.
The source explicitly distinguishes a bootstrap image (no signed package digest)
from a package (publisher, package identifier, release, SHA-256 and policy epoch).
Application numbers are kernel handles; numbers starting at 65536 are dynamic.
Neither this record, a copied number, nor a manifest request conveys authority.

The base application profile has private memory, its own surfaces/input and
lifecycle/input broker communication. Public permission bits are independent of
reserved system roles. Selected-file read/write apply only to the object already
bound by the broker to the application's exact live window and version; they do
not allow names, directory enumeration or arbitrary object IDs. Private package
data and creation of new picker grants depend on the OS package owner/picker.

`appearance::exchange` checks request shape, bounded rows and correlated replies;
DesktopAppearanceWrite authorizes the settings service, not the legacy USB image
gallery. `metrics::read` checks the canonical bounded view and requested sample/page;
SystemMetricsRead authorizes observation, never process control. The OS ABI reexports
these exact definitions. Hosts must advertise implemented protocol versions before
launch; a library version is not a substitute for compatibility negotiation.
