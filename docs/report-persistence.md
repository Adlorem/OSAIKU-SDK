# Saved report definitions and results (experimental v1)

`osaiku_runtime::report::artifact` and `ReportView` provide bounded saved data
for both `Surface::Window` and `Surface::Region`. No model inference, source
credentials, executable code, process pointers or authority tokens are persisted.

## Definitions and snapshots

- `DEFINITION` contains the accepted compiled report template, the host's
  versioned recipe/configuration and a contract fingerprint. Reopening admits
  the template again and resolves its bindings using the current, authorized
  providers. Old process/window identities are provenance only.
- `SNAPSHOT` contains a literal copy of the accepted report subtree: text,
  values, chart points, unknown/partial markers, source descriptions and view
  state. Source bindings are removed; source actions and text inputs are
  disabled. Only caller-allowlisted persistence controls may stay live. Region
  extraction excludes the surrounding host's nodes and text. This is a snapshot
  of the materialized report view, including its current table page, not an
  export of all provider rows or a screenshot. V1 explicitly refuses unresolved
  virtual-list providers. Snapshot comparison remains separate work.

`ReportView::save_artifact` exports only a committed report and refuses a pending
publication or definition upload. A refused candidate cannot become the saved
result. `open_artifact` checks the envelope, recipe, contract and document before
staging a local definition; the usual prepare/submit/commit transaction still
decides publication. Failed admission leaves the previous active definition.
Opening uses fresh interaction state, so an old filter/selection cannot replace
recorded content just because element IDs coincide. Later ordinary publications
retain the normal compatible interaction behavior.

The caller owns query configuration and its interpretation. It must validate
`Header::settings` before staging, rerun providers only for definitions and avoid
starting queries for snapshots. The OS recipe 1 stores disk/session selection,
category filter, name/size ordering, direction, source visibility and page.

## Wire format and compatibility

All integers are little endian. The 128-byte envelope starts with `OSAREP01`,
then u64 format version 1, kind, recipe, settings, contract key and payload
length; five u64 values identify the recorded owner/window/incarnation/epoch/
revision, followed by mount generation and catalog revision. Eight reserved
bytes must be zero. The last eight bytes hold FNV-1a over header bytes 0–119 and
the payload. This checksum detects accidental mixing/corruption; it is not an
authorization or cryptographic signature. OSAIFS supplies authenticated storage
and the transactional writer independently checks SHA-256 for the complete blob.

The payload uses the ordinary SDK document wire; maximum size is `MAX_WIRE`.
The complete artifact is at most 65,664 bytes. Extra bytes, truncation, unknown
envelope versions/kinds, incompatible recipe/contract keys or unsupported UI
wire versions are refused. The key covers catalog/wire versions, registered
sources and their schema/shape, fields/types and actions. V1 has no implicit
migration. A future migration must be explicit and preserve the original file.

## Storage and native worker

The production broker admits `REPORT_INFO/READ/BEGIN/CHUNK/COMMIT/ABORT` only for
Report/Assistant processes acting on their own live window. Existing Document
file permissions and initd's broker allowlist are unchanged. These operations
address 16 fixed root slots, not arbitrary names/paths: eight pairs
`report-N.osa-def1` and `report-N.osa-res1`, shared by both report surfaces.
The files are ordinary named OSAIFS files so the file manager can enumerate,
rename or remove them; reopening through this API uses their reserved slot name.
Storage formats without the installed V3 transaction engine explicitly refuse
report persistence.

`artifact::storage::run` is the transport-neutral client. Save observes a file
version, starts a bounded upload, sends exact contiguous 256-byte fragments,
then commits. BEGIN and COMMIT both check the expected object/version. Only
COMMIT changes the named file, using the V3 transaction engine. An incomplete,
canceled, corrupt or conflicting upload cannot replace the previous file.
Unknown commit results are reported without automatic retries; reopen to inspect
the durable state. Lost/malformed commit replies are also treated as unknown.

Reads bind the slot to a mount/object/version and reject mixed fragments. The
client validates the entire envelope before returning a complete result and
scrubs failed/canceled downloads. The kernel stages at most two uploads in the
existing accounted storage workspace, bound to endpoint generation, window
incarnation, mount and monotonic token. A subsequent report upload request
scrubs dead, replaced-mount or expired stages (five minutes of inactivity when
the clock frequency is available); explicit abort and commit scrub immediately.
Staging is volatile and never conveys an execution grant.

Native `metadata::Query::start_artifact`/`poll_artifact` reuse the metadata
worker's stack and a union of query/artifact scratch. Cancellation is checked
between requests, and shutdown waits for the worker to join. A confirmed commit
stays successful even if cancellation arrives after its durable point. The
callback used to build/import an artifact runs on the application thread; no
session references cross into the worker. The scratch is wiped before release.

Generic SDK tests cover envelopes, compatibility, mixed reads, cancellation,
lost replies and committed state extraction. The OS's synthetic-media tests
exercise the actual broker adapter and V3 engine across remount, replacement,
conflict, expiry and incomplete upload. These are not desktop-widget tests or
claims of interactive/performance acceptance.
