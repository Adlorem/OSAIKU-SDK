# Scoped read-only report queries

`osaiku_runtime::report::metadata` is the provider layer for report bindings.
`Scan` is a bounded state machine using the public `files::{FileRequest,FileReply}`
envelopes. `step(fetch)` performs at most one LIST_V2 or STAT_V2 exchange. Supply
only an authorized provider; the pure state machine grants no permissions.

Start with a nonzero `Scope { window, incarnation }`. The first successful reply
pins mount/catalog revision. Subsequent replies, cursor bodies, sequence numbers,
object versions, parents and lengths are validated. Stale/conflicting data clears
the snapshot. `cancel()` clears values and prevents additional exchanges. States
distinguish complete, partial, canceled, stale, unavailable and invalid results.
Store `Scan` in bounded long-lived memory, not a small native thread stack.

Limits are 256 files, 64 directories, 2,048 service steps and 100 consecutive BUSY
replies. A capacity limit or unavailable remainder yields a partial snapshot.
Logical sizes are `Option<u64>`; checked category totals preserve unknown and
overflow. Categories are extension rules, not MIME detection or device usage.
Even a complete scan covers only the mounted named-file catalog, excluding raw
device regions, filesystem overhead and free space.

`Order::rebuild` supports category and ASCII case-insensitive substring filters,
name/size sorting and direction. Its indices refer to a particular scan. Retain
mount/object/version keys with each accepted view identity; do not interpret old
row actions using new indices. `index_of` locates a stable object in an order.
Sorting is in place, without allocation, with O(n log n) worst-case comparisons;
object IDs break equal-name/equal-size ties deterministically.

On the native target, `Query::start(scope)` leases one worker per process. It uses
FileOperation and existing ThreadCreate/Join/Exit/Sleep operations, with permanent
32 KiB stack, TLS and result storage. `poll(&mut result)` returns true only after
join and copies the finished result in place. `cancel()` signals the worker;
continue polling to join before reuse. Dropping cancels without blocking and
retains the worker's ownership. The next start attempts a nonblocking join of
that orphan, clears its workspace, and admits new work only after join succeeds.
While the orphan runs, start refuses and leaves its memory untouched. Terminal
process retirement also reclaims it. A failed start leaves the caller responsible
for displaying unavailability.

Use `WindowApplication::poll()` to collect results and return whether the view
changed; use `poll_interval_us()` while work is active. The hosted loop requests
UiViewInvalidate (375) for its last accepted identity and retains dirty state
until successful publication. The kernel only admits the caller's exact current
presented identity; retries cover a submission that has not yet been presented.
The ordinary host still queries and publishes the view. No model, injected input
or cross-window access is involved.

The current OS admits metadata reads for its Report and Assistant applications
only on their own live windows. Importing this SDK API does not admit an external
application or grant any storage capability. Public report definitions select
registered bindings/actions; they cannot request syscalls or widen query scope.
