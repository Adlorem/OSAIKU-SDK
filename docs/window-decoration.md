# Native frame and CPU effects

`osaiku-ui::decoration` is a host-neutral library used by OSAIKU's actual frame
renderer. It depends only on the public UI crate. It neither opens windows nor
obtains display access. It is experimental source API; the view wire version
does not change with this extraction.

`frame::HTML` and `frame::CSS` expose the built-in template. Compile them on the
build host, replacing `$SURFACE`, `$TITLE`, `$TEXT`, `$ACCENT` with validated
colors. Pass the immutable Document to `frame::scene`, together with native
title/control metrics, the literal title, the name of the badge's icon (a host
image set: give the scene the images with `Scene::with_images`; OSAIKU passes
`shell_icons::ICONS` and names each application's shelf icon), measured title
width and the centering preference. `frame::slots` and `resize_regions` provide the compact
geometry used for hit routing; paint/semantics use the resulting Scene.
The template draws the title bar in `$TITLE` and the content in `$SURFACE`: a square application
badge (accent colour, rounded, centred in the bar and scaled with it) showing
the application's icon at the largest built size that fits, the title
beside it and a borderless `×` close control. OSAIKU's metrics make the close
target 8x5 logical units, one unit inside the bar's top and right edges.
Coordinates never grant permission to move, close or resize another window.

OSAIKU's current effect sequence is: retain private opaque foreground; apply the
frame perimeter; capture a cursor-free backdrop into compositor-owned RAM;
apply shadow, copy foreground rows and apply rounded coverage; publish a
coherent image. The optional `glass` library remains available for other hosts;
it is not used for OSAIKU's current window composition.
Its `glass::footprint` includes the blur's 8px sampling margin independently of
disabled/short native shadows. Capture that extent for glass; pass the relevant
subspan with the shadow's own footprint to a separate shadow/corner pass.
The `corners`, `shadow` and `glass` modules implement the same production code
for full and clipped passes. Keep backdrop storage separate from client-owned
surfaces. Obtaining or disclosing other windows' pixels requires host authority.

The native preset preserves 12px corners, 1px outline, 20px shadow and 3px glow.
The shape and falloffs are now defined by `WindowStyle`, alongside the frame's
HTML/CSS as `frame::STYLE`. Both the OS footprint/damage adapter and the
compositord renderer use this preset; no radius or halo size is duplicated in
kernel rendering code. Existing `corners::*` and `shadow::*` functions remain
convenience entry points to that same prepared preset.

## Configurable native decoration

```rust
use osaiku_ui::decoration::{Falloff, PreparedStyle, WindowStyle};

const STYLE: WindowStyle = WindowStyle {
    corner_radius: 8,
    shadow: Falloff { extent: 16, color: 0x000000, opacity: 80 },
    glow: Falloff::NONE,
    ..WindowStyle::NATIVE
};
static EFFECTS: PreparedStyle = PreparedStyle::new(STYLE);
```

`corner_radius` and both falloff extents accept 0–32 physical pixels, independent
of display scaling. Radius zero selects square corners. Smaller-than-diameter
windows also use square corners. Each falloff has RGB color and 0–255 opacity;
zero extent or opacity disables that layer. Set **both** `shadow` and `glow` to
`Falloff::NONE` to remove the outer halo entirely. Outline/focus RGB colors can
also be changed. The outline stays one pixel thick.

A compositor host uses `STYLE.footprint(window, screen)` for capture, damage and
restoration, and `STYLE.corner_boxes(window)` for coverage-sensitive regions.
It calls `EFFECTS.frame_surface` once when rasterizing a window, then
`apply_shadow_clipped`, copies the foreground, and calls `apply_corners_clipped`
for each repair. Full-image `apply_shadow` / `apply_corners` are also available.
Square corners are a successful no-op in the corner pass. All passes validate
geometry/storage before writes, including for disabled effects. Pixel buffers
remain owned by the host, never by the style.

Prepare lookup tables **once per style**, shared by windows: the bounded value
is about 59 KiB, not a per-window bitmap. Static preparation runs at compile
time, with no boot/drag-time sampling, square roots or shadow color conversion.
Each shadow/corner pass prepares its small placement geometry once; the pixel
loop reuses the selected mask and window bounds.
An invalid static style fails compilation. A host accepting dynamic settings
must call `valid()` first and prepare outside the render transaction in storage
appropriate for a large value; do not construct it on a small service stack.
When changing a live style, invalidate frame caches, repaint the union of old
and new footprints and recapture underlays before publishing the new style.
Do not mix geometry from one style with pixels/tables from another.

This is a native **host style API**, not an added `border-radius` or `box-shadow`
property in the client Document wire format. OSAIKU currently selects its preset
at build time; a live user preference and its transport remain separate work.
Applications cannot override trusted window chrome by supplying client markup.
No backdrop access or window-management permissions are implied by this API.

Passes accept RAM slices and, for long loops, `&mut impl FnMut()` as a cooperative
host scheduling boundary. The callback is never part of transported UI data.
There is no allocation, device access, unsafe code or global mutable state.
Images are limited to 64 MiB each; the host must enforce aggregate quotas and
ownership. Reject `None` results and do not publish a partially completed frame.
On cancellation/fault the host must rebuild from its last coherent publication.

Library reuse is not process isolation. OSAIKU now executes normal window
rasterization/effects in `compositord`; the kernel retains scene ownership,
transport validation and publication. A third-party app should not receive a
compositor backdrop merely because it links this library.
