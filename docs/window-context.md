# Window image and semantic context, version 1

`osaiku_runtime::context` gives an agent client a bounded image and semantic
snapshot from **one committed window publication**. It does not start a model,
execute UI actions or grant general desktop access. Syscall `UiContext` (376)
implements OPEN, SEMANTICS, PIXELS and RELEASE. Its records are padding-free,
integer-only versioned wire values, shared through `osaiku_abi::ui_context`.

## Scope and disclosure

The image is a crop of the immutable **client content** of one window, in BGRA8
premultiplied format, with packed rows. It is not a framebuffer/desktop screenshot.
Other windows, cursor, system decorations, desktop backgrounds and host-owned
dialogs/menus are excluded. Overlapping windows do not become part of this image.
Menus rendered inside the application's client publication are included, together
with their visible semantic elements. Bounds in the semantic result are relative
to the crop; adding `info.crop.x/y` maps them back to client coordinates.

Only the owner of the exact live window can OPEN a capture. The request names a
full recipient process endpoint including generation, crop and disclosure mode.
The returned token permits only that recipient and the owner to read/release that
specific snapshot. It is not transferable by forwarding its numeric value. The
owner can send the token through its existing authenticated IPC workflow, after
its own application/user disclosure decision. There is no privileged default
agent reader and no agent request that silently obtains another window's data.

- `STRUCTURE`: element identities, roles, state and clipped geometry, with empty
  labels/values/series; pixel reads return opaque black.
- `CONTENT`: pixels inside the authorized crop and eligible semantic values.
  Hidden/offscreen elements are omitted. Partially clipped labels, scalar values
  and series are redacted as a whole. Any later overlapping visible element
  conservatively redacts underlying semantic content, even for a transparent
  overlay. `CLIPPED` and `REDACTED` distinguish those cases from missing elements.

This API cannot reveal file contents, offscreen rows or hidden text through the
semantic record. It does reveal everything the application drew inside a CONTENT
crop; applications must choose the crop/mode accordingly. Semantic labels/actions
are client assertions, not trusted instructions or newly granted action authority.

## Identity and lifetime

`Request::open(target, revision, recipient, crop, disclosure)` takes a nonzero
window target (owner/window/incarnation/compositor epoch). `revision == 0` selects
the currently committed presentation; a nonzero revision requires an exact match.
Neither path substitutes the latest submitted or sealed-but-undisplayed frame.

The compositor atomically retains a presented source for each window. Capture
pins that source's recipe, semantic layout and pixel backing together. `Info`
contains the exact surface handle/revision, compositor frame generation, source
recipe hash, application ID, crop, disclosure and scope flags. This provenance
identifies the producing application/publication; it does not authenticate
application-supplied text or external service facts.

New publications, focus changes or resizing do not change an existing capture.
It remains a snapshot of its recorded publication, never a claim to be the newest
screen. Close/reopen, owner/recipient retirement and compositor epoch changes
revoke access. RELEASE by either party cancels the shared capture. Kernel resource
retirement drops pins outside pool/process-table locks. A failed OPEN leaves no
capture behind. Legacy windows without a sealed SDK image are explicitly refused.

Limits: 16 MiB cropped image, 64 KiB per pixel read, 128 semantic elements, 8 KiB
labels and 120 shared chart samples. Eight captures may be live globally, with at
most two per owner and two per recipient. The 64 MiB global retention budget counts
each complete pinned source image, not merely its crop. Existing image accounting
continues to charge that backing until its final owner releases it. Quota pressure
is a refusal, never implicit eviction or a switch to another revision.

## SDK workflow

1. Owner calls `open_with(&request, exchange)` (native exchange is
   `osaiku_runtime::context::exchange`) and passes the returned token to the named
   recipient through its existing IPC protocol.
2. Recipient allocates bounded caller-owned scratch and calls
   `semantics_with(token, &mut snapshot, exchange)`. Failed/corrupt responses are
   scrubbed. Validate provenance and disclosure before processing source text.
3. Allocate exactly `snapshot.info.extent.bytes()` bytes under the application's
   own memory policy. `Receiver::new(&snapshot, &mut pixels)` exclusively borrows
   both for the transfer. Each `next_with(exchange)` reads at most 64 KiB.
4. `finish()` yields a read-only `Context` only after all image bytes arrive.
   Incomplete/corrupt/stale reads never produce a complete context. Read errors
   clear partial pixels; dropping an incomplete receiver also clears them.
5. Call `release_with(token, exchange)` after consumption. For cancellation,
   `receiver.cancel_with(exchange)` clears local pixels and releases the capture;
   failed release may be retried. A canceled receiver cannot resume reading.

Large snapshots and image buffers belong in accounted long-lived application
storage, not on a small native stack. The API allocates no caller memory and makes
no network/model calls. Model integration is a Stage 31 consumer of this boundary.
