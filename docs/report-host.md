# Transactional report host

`osaiku_ui::report::host::Host` implements one bounded report session in
caller-owned memory. `osaiku_runtime::report::ReportView` supplies guarded,
process-image storage for native apps; declare it as a static, like `PackagedView`.
It decodes a registered template, checks its contract before invoking binding
providers, and prepares an ordinary encoded `Document` for the existing window
runtime. The [native example](../examples/report-window/README.md) is its first
consumer. Neither API allocates an OS window or launches a process.

## Lifecycle and publication

`open(owner, report, Surface::Window)` creates a report incarnation. For an
embedded report, use `Surface::Region(anchor_id)` and supply the trusted
surrounding view to `prepare`. An assistant host uses its own authenticated
process endpoint and surrounding view; arbitrary report content cannot choose
another process's region. The owner number is a local binding, not proof of
authority. Authenticate external messages before invoking these APIs.

Each update names the current report revision. `prepare` creates one opaque
token for the next revision while preserving the accepted document. A second
pending update, a stale base, unknown data references or an incompatible schema
is refused. Encode/rasterize/submit the prepared view through the ordinary
surface protocol, then `commit(token, identity)` only after that submission is
accepted. This records acceptance, not a claim of completed display scanout.
Call `discard(token)` on a refused publication. Failure never replaces the
accepted picture or consumes its report revision. Tokens from discarded,
closed or previous report incarnations cannot be committed.

The first commit binds the session to its host window/incarnation. Later commits
cannot change that binding; advancing the compositor epoch revokes old input
identities while retaining application state. Input must come from the normal
authenticated input route and name the exact latest accepted identity. This
host conservatively refuses queued input from older accepted revisions.

`close` invalidates pending and accepted revisions and scrubs both view slots,
filter data and native decode/binding scratch. Static backing stays charged to
the process image until exit and can be reused; close does not claim to free
process-image pages. Close the OS window through its usual lifecycle separately.

## Compatible state

The host keeps user interaction state separate from its immutable accepted
document. Focus, scroll, selection and filter edits appear in the next prepared
view. Matching node IDs, kinds, parents, actions, binding types, source identities
and source schema versions preserve compatible edits across data/layout changes.
Removed or incompatible nodes lose those edits. Fresh hidden/disabled flags
always win; preserved focus cannot re-enable a disabled control.

`event` accepts ordinary focus, scroll and activation events. Registered select
intentions retain selection on stable, nonvirtualized elements. `set_filter`
accepts bounded literal UTF-8 only for a registered filter text-input control;
it does not parse markup or run queries. The core host's `filter` accessor lets
the host retrieve its current text. Actual filtering/sorting and asynchronous
queries are supplied by the [30F3 provider layer](report-queries.md).

Virtual-list scroll can survive compatible updates. Item-local focus, filters
and selection do not carry across updates by ordinal: an ordinal is not a
stable data identity after sorting or filtering. Providers must reconcile those
states using stable item keys before materializing a replacement. No silent
index-based selection transfer is performed by this host.

## Embedded composition

`report::region::mount` appends the report under an empty host-owned container.
The resulting tree remains a normal Document, with shared layout, semantic
projection and rasterization. Node IDs remain unchanged; collisions are refused.
Text/point ranges and style, decoration and virtual-list table indices are
remapped. Existing document node, depth, text and palette limits apply to the
whole composition. If the combined view does not fit, retain the prior view.

The anchor clips descendants in wide, narrow and state-specific styles without
changing styles shared by surrounding controls. Host hidden/disabled ancestors
continue to govern report input. Report event handling refuses controls outside
the report subtree. The surrounding view is an immutable empty-anchor template,
not a previously composed frame. Closing the region lets its parent render that
template again without the report.

## Integration status

The OS registers ordinary report-host and assistant-window applications. Initd
selects their definitions from a build-time catalog and drives authenticated
[definition transfer](report-transfer.md) followed by ordinary publication.
The assistant presentation process owns the surrounding view and clipped report
region; its closure/retirement revokes both. It is separate from `assistantd`,
which gains no route to application windows. The session report exposes
the receiving window's own session metadata. No model inference is required.
The [metadata provider](report-queries.md) now also supplies the disk-category
report, sorting/filtering and source inspection on both surfaces. Coherent
capture is supplied by [window context](window-context.md); saved reports remain
a later stage.
