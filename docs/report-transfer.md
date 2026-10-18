# Report definition transfer, version 1

`osaiku_runtime::report::transfer` transports compiled SDK UI definitions over an
already authenticated lifecycle-host IPC channel. It carries no markup compiler,
source registration, provider data, executable code or permissions. A definition
can reference only the receiver's registered fields/actions. Literal UI text is
permitted. Source data is bound locally when the normal presentation runs.

## Protocol

Packets are exactly 152 bytes, below the 160-byte IPC limit and distinct from the
156-byte teardown and 160-byte editor envelopes. All scalars are little endian:

| Offset | Field |
| --- | --- |
| 0 | 8-byte `OSAIRPT1` magic |
| 8 | u16 version 1 |
| 10 | u16 kind: BEGIN, CHUNK, FINISH or CANCEL |
| 12 | u32 chunk length |
| 16 | u64 request ID |
| 24 | five u64 identity fields: epoch, owner endpoint, window, incarnation, revision |
| 64 | u32 offset |
| 68 | u32 total definition bytes |
| 72 | up to 80 data bytes, remaining bytes zero |

BEGIN and CANCEL have offset zero and no data. FINISH has offset equal to total
and no data. CHUNK must be nonempty and fit within total. The total is bounded by
`osaiku_ui::MAX_WIRE` (64 KiB). Every packet names the exact latest accepted
publication. Unknown versions, nonzero padding, malformed lengths and invalid
identities are refused before transfer mutation.

Replies are 80 bytes, with `OSAIRPA1` magic, the same version, status at offset 10,
zero bytes at 12..16, request/identity at 16..64, received count at 64, total at 68
and eight trailing zero bytes. Status distinguishes receiving, staged, cancelled,
invalid, stale, busy and rejected. These are acknowledgements, not frame receipts.

## Producer

`Upload::new(identity, request_id, wire)` borrows an immutable wire buffer.
`packet()` returns the next packet; encode and send it through the authorized
channel. Pass the actual IPC sender and reply bytes to `acknowledge()`. Only an
exact receiver endpoint, request, identity and expected byte count advance the
cursor. A malformed/foreign reply leaves the cursor unchanged. Request IDs must
increase for the receiving workspace's lifetime, including cancelled/refused
transfers. `cancellation()` builds an explicit cancellation for that transfer.

Use bounded receive deadlines and preserve unrelated host messages in the OS
adapter. This API does not perform IPC, retry indefinitely, choose an owner or
grant permission to reach another process. After `staged()` becomes true, request
ordinary presentation and verify its acceptance separately. On a lost response,
reconcile/cancel rather than treating unacknowledged bytes as accepted. A later
normal publication expires a partial transfer bound to the old identity.

## Receiver and publication

`ReportView::receive_definition` checks the trusted `ApplicationHost` policy
against the IPC sender, then checks its session and accepted identity. The caller
supplies host-owned registration. FINISH validates both document wire and contract
before staging. Providers are not invoked by transfer admission.

Two fixed wire slots retain the accepted definition and one candidate. Chunks
must be contiguous; interleaving, replay and stale targets are refused. Cancellation
scrubs the candidate. The candidate is used by the next `prepare`; it becomes the
accepted definition only when the corresponding `commit` succeeds. Encode/bind/
render failure or `discard` removes the candidate and retains the previous
definition and accepted report. A prepared publication blocks incoming mutations.
`close` scrubs both slots. The monotonically increasing request cursor is retained;
reopening a workspace does not make old transfer IDs reusable.

Static storage consumes 128 KiB for the wire slots in addition to the existing
report workspace. It is included in application image admission and reclaimed
with the process. No shared mappings, kernel scratch or new authority are added.

## Native integration

`WindowApplication::host_message` receives extension messages only after the
native runtime authenticates its lifecycle host. Existing applications default
to refusing unknown extensions. Legacy lifecycle messages keep their routing.

The OS report and assistant-window applications consume this protocol from
`initd` only. Initd selects a definition from its build-time catalog after human
activation, binds it to the first accepted publication, uploads it and requests
another normal presentation. The report region belongs to the assistant UI
process and cannot replace its surrounding view. Applications and `assistantd`
cannot submit definitions directly. A bounded transport failure rolls back that
activation and retires its exact endpoint, reclaiming incomplete transfer state.
The current catalog uses the receiver's own typed session provider; asynchronous
service queries and model-produced definitions are not implemented by this path.
