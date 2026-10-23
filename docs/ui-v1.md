# Experimental UI format (wire major 11)

Implemented compiler/runtime foundation; not a released native application
runtime. `osaiku-ui` 0.1.0 has no OS-source or third-party dependency. Runtime
is no_std without an allocator. Feature `compiler` enables the alloc-based
bounded parser for build scripts and future user-space report hosts.

## Authoring

Use `.ui.html` and `.ui.css`. Every element requires a unique stable `id`.
Names use 1–64 ASCII alphanumeric, dash, underscore or dot characters; selectors
use type, `.class`, or `#id` (dots inside IDs are not selectable in initial CSS).
`:focus`, `:disabled`, and SDK `:selected` suffixes may be combined, or used
without a base selector to match any eligible node. Duplicate/unknown state
suffixes, combinators and other pseudo-classes are rejected.
Symbol hashes are window-scoped identities, not authority; collisions are
rejected. Text and bindings are UTF-8. Supported entities: `&amp;`, `&lt;`,
`&gt;`, `&quot;`, `&apos;`. Tags must close. No browser error repair, mixed
text/child content, scripts, URL loading or expressions. HTML comments and CSS
comments between rules are supported.

| Elements | Behavior / role |
| --- | --- |
| main, section, header, footer, div, thead, tbody | Container/group |
| nav | Container/navigation |
| span, p; h1, h2 | Text; heading |
| button | Focusable action/button |
| ui-text-input | Focusable application-owned editable text / textbox |
| table, tr, th, td | Table, row, column header, cell |
| ui-virtual-list, template, ui-list-item | Scroll viewport, single row prototype, selectable row |
| ui-chart | Percent history, timestamped series with missing readings |
| ui-value-bar | Percent meter; unavailable is distinct from zero |
| ui-separator, ui-icon | Separator; decorative square mark, or a host image with `src` |
| ui-popup-menu | Menu above the document at an anchor point (context menu) |

Tables use explicit column widths shared by header/data-cell classes, not
browser table layout. Containers cannot also contain text. Icons are geometric
marks. `class` admits eight names. Boolean HTML `hidden` and `disabled` are
true by presence, including `disabled="false"`; bindings supply dynamic values.

| Binding | Rust value | Targets |
| --- | --- | --- |
| bind:text | Value::Text | Leaves |
| bind:value | Value::Percent(Option<u16>) | ui-value-bar |
| bind:series | Value::Series | ui-chart |
| bind:hidden / disabled / selected | Value::Bool | Any node |

Percent is 0–10000 basis points; None/10001 is unavailable. Series contain at
most 60 points with strictly increasing epoch-relative milliseconds. Gaps need
unavailable samples to break the line; spacing is proportional to elapsed time.
The shared chart paints a 0–100% scale and seconds relative to its latest sample
(`0 s`). Oldest and, when space permits, midpoint labels show negative offsets
with exact millisecond fractions, without assuming a 60-second window. Narrow
charts omit labels that would overlap. Empty series have no time labels; one
sample has only `0 s`, even if its value is unavailable. Lines and point markers
are clipped to the plot; labels respect the chart and scroll viewport clips.
Two series together fit 120 points. `Bindings` resolves locally; callbacks
never enter encoded content. Data text is literal. `action` names application
behavior, only on buttons/selectable rows/text inputs; it grants no system privileges.

`ui-text-input` renders bounded literal text and participates in focus, clipping
and scrolling. A pointer press sends `FOCUS` without executing the field's
action; Enter sends `ACTIVATE` for applications that submit the edited text.
The application owns its buffer, selection and authenticated
keyboard/command route. `editing::replace_selection`, append and backspace
validate UTF-8 boundaries and capacity before mutation and scrub removed bytes.
They do not grant filesystem access or disclose content to an agent. Document
keeps its existing 32-byte keyboard/save contract when using this control.
The text-only `text_model` binder formats bounded labels without a
Document-sized stack temporary. Its `matches` verifier checks a submitted
recipe against a trusted template/model while allowing focus and scroll state.

Wire major 5 adds TextInput/TextBox. Wire major 7 adds box decoration,
alignment and chart-area opacity (see below). Wire major 9 adds virtual lists.
Wire major 10 adds `outline: none`. Wire major 11 adds `background-image`
with the shared background presets.
Rebuild packaged views and
external SDK applications together with the runtime; earlier wire majors are
rejected.

## Virtual lists and lazy data

```html
<section id="panel" class="panel">
  <h2 id="heading">Processes</h2>
  <ui-virtual-list id="processes" source="process-data" item-height="36" overscan="2">
    <template>
      <ui-list-item id="process" class="virtual-row" action="select" bind:selected="selected">
        <span id="name" bind:text="name"></span>
      </ui-list-item>
    </template>
  </ui-virtual-list>
  <footer id="status">Ready</footer>
</section>
```

The list inherits `width:100%; flex-grow:1; overflow:auto`. Give its parent the
available client-area height (for example `#panel { flex-grow:1; }`). Its left
and right edges follow the parent's inner edges; its top follows the header
and its bottom precedes the footer. Padding, gaps and borders remain effective.
Resizing recomputes the viewport and requested rows; no screen coordinates or
manual repositioning of each item are required. Rows have a fixed height.
When scrolling is needed, rows reserve the scrollbar's eight-pixel gutter;
the last column's text and actions cannot fall underneath its track.

Compile once, bind ordinary application state with `Document::bind_into`, and
set `prepared.lists[prepared.nodes[list_index].value as usize].total` and the
list node's `scroll_y`. Compute `Layout` at the current content size, then call
`virtual_list::requested`. Fetch only that ordinal range from the application's
data provider. Implement `virtual_list::Items::get(source, ordinal, binding)`
over the fetched range and call `virtual_list::realize` into a separate
caller-owned document. Paint and publish that document normally. This is lazy
data retrieval controlled by the application, not an implicit kernel callback.
An asynchronous provider should retain the previous frame until data arrives.

The compiler stores one row prototype; realization expands only visible rows
and overscan, and hidden lists make no data requests. Scrollbar size reflects
the entire logical data set. Shared input supports track clicks, thumb dragging
and PageUp/PageDown. Native wheel routing remains a separate input feature.
The prototype has one root; `<template>` receives a generated ID. Its other
elements need explicit IDs. Nested virtual lists and per-item series bindings
are refused. Text, percentages and boolean state can be bound per item.

At most 16 lists share a document's existing 128-node/8-KiB text budget. The
configuration table adds 256 bytes rather than enlarging every node. Row height
is 1–4096 px, overscan 0–8 rows, and total scroll extent at most 1,048,576 px.
Set every list's total and scroll before querying ranges. Visible rows across
all lists are reserved first, then the spare nodes fund overscan in list order.
Planning is identical for range queries and realization. If visible rows exceed the budget,
realization returns `Error::Limit`. Choose a row template whose visible rows fit
the application's supported maximum viewport. Multiple visible lists share
the same budget. Missing data or invalid dimensions refuse publication; never
publish partial scratch after an error. These limits do not grow with item count.

`item_id(list_id, prototype_node_id, absolute_ordinal)` gives stable generated
element IDs. It is not a process or object identity. Resolve events against the
exact presented revision and the original item's stable key (for processes,
PID plus generation), because data may reorder between refreshes. The source
symbol identifies a local provider and conveys no permission. No provider
function pointer enters the wire. Shared `.virtual-row` styling uses the SDK
palette and follows live system themes; application CSS supplies column layout.

## Styles and layout

Scope is one view. Cascade follows CSS specificity: IDs, then classes and
pseudo-classes, then types; equal specificity uses source order per property.
A combined state selector requires every named state. `:disabled` reflects the
control's effective state, including a disabled ancestor. Focus and selection
are local to the node. Color, font size and text-align inherit dynamically from
the resolved parent style; an explicit child property wins over inherited values. `:root` defines 32 `--name`
tokens; `var(--name)` follows at most eight references and rejects missing
values/cycles. Aliases resolve within the view before computing styles. A `theme(name)` value retains its SDK palette reference for runtime painting.

Implemented properties: `display: block|flex`, `flex-direction: row|column`,
`flex-grow: 0|1`, `width: auto|Npx|N%`, `height: auto|Npx`, `padding`, `gap`,
`font-size` (pixels), `color` (#RRGGBB), `background-color` (#RRGGBB or #RRGGBBAA),
`background-image` (`none` or `theme(preset)`, below),
`overflow: visible|hidden|auto`, and the box/alignment properties below.
Dimensions: 0–4096; padding/gap: 0–256; font size: 12, 14, 16, 18, 20, 22, 24,
28 or 32 px (see Text below). Dimensions include padding and border (SDK
border-box default). Zero is not auto. Text is one line, clipped;
shaping/wrapping is pending.

### Text

UI text uses Inter 4.1 Regular (SIL OFL 1.1, `ui/licenses/Inter-OFL.txt`), a
proportional typeface. `ui/src/text_atlas.rs` holds one pre-rasterized,
grid-fitted table per admitted size, generated by the OS repository's
`tools/font/generate-text-atlas.py`; text is never scaled at runtime, so every
size is drawn as sharply as the 16 px default. Only each glyph's inked box is
stored, at 16 coverage levels (two pixels per byte): 56 KiB for all sizes,
which bounds the font's contribution to the application's admitted image. Any other `font-size` is refused
by the compiler with the admitted list, and by document validation on the wire.
A size is the line box: the font's ascender-to-descender span fills exactly
that many rows, so layout heights are unchanged from earlier fonts. A line box
taller than its content box overflows both edges when `vertical-align: middle`
(and the top when `bottom`), as in CSS; the node clip bounds what is drawn.

`font::ui_text_run(text, size)` places a line: pens advance in 1/64 px and each
glyph lands on the nearest whole pixel, so spacing does not drift along a line.
`font::ui_font_text_width` returns the same rounded total. Every adapter (client
band raster, compositor canvas and recipe planners, kernel recipe planner) draws
from this run, so alignment and pixels agree. Kerning is not applied. A
character outside the atlas (Basic Latin, Polish letters, middle dot, en dash,
ellipsis, ×, ←, →, ↻, ✓ and ▼) is drawn as `?`. The fixed-cell DejaVu Sans Mono atlas
(`font::ui_font_glyph`) remains only for the native cell-grid presentation
path and its pinned hash.

The root fills the content viewport. Column flow stacks natural heights;
flexible children share remaining height, preserving navigation/footer around
scrolling content. Row flow shares remaining width among auto-width children
after fixed/percentage widths and gaps; `align-items` places a row's children
on its vertical axis. `@media (max-width: Npx)` uses content
width, including equality. One threshold per view, multiple matching rules;
other conditions/nested media are rejected.

Layout is bounded to 256 nodes/depth 24. Scroll is clamped to content extent.
`Scene` borrows an immutable validated Document and owns the layout: paint,
pointer lookup and semantic enumeration share geometry and inherited states.
`Document::set_focus(id)` replaces the focus flag after binding data. It clears
focus for missing/noninteractive targets and targets hidden or disabled by
an ancestor. It bounds malformed parent paths and does not replace full document
validation or viewport clipping. System Manager and `PackagedView` use this same
rule, so an updated paging button or disappearing row cannot publish stale focus.

Focus traversal skips clipped/hidden/disabled actions. Child enablement cannot
override a disabled ancestor. `Scene::from_document` uses the recipe's encoded
per-node scroll offsets. `input::key_event` implements forward/backward focus,
activation and page scrolling. `input::pointer_event` handles actions and
scroll-track presses using the same layout. `input::Pointer` additionally retains
thumb capture through press/move/release/cancel. It preserves the grab offset,
clamps motion outside the track, and follows repaint revisions of the same
window. Changed track geometry or owner/window/incarnation cancels capture.
Native wheel reports are not implemented. The track is drawn like a styled
`ui-value-bar`: eight pixels wide and fully rounded, the `track` token under
an `accent` thumb, in the active theme.

Backgrounds carry straight ARGB in `Style`; six-digit CSS colors get alpha 255.
`Painter::fill_alpha` receives RGB and independent coverage. Native adapters
compose premultiplied pixels into a transparent content surface above the window
material. Background alpha does not reduce text or child-control opacity.

Selection and disabled decoration are authored with state selectors rather than
forced painter colors. The focus indicator remains a one-pixel border using
the resolved text color; it follows the element's corner radius. `outline:
none` omits it (`auto` restores it); an element without the ring should show
focus through its own `:focus` rule, for example a background. All supported
properties may appear in state rules; state-dependent size/font/overflow/border
changes also affect layout, hit testing and semantic bounds. Arbitrary template
replacement, per-corner radii, configurable outline geometry and hover/pressed
tracking are **not implemented**.

### Box decoration and alignment (wire major 7)

| Property | Values | Applies |
| --- | --- | --- |
| `border-radius` | `Npx`, 0–255 | Background, border, focus ring, `ui-icon` mark, `ui-value-bar` track and value |
| `border-width` | 1–4 CSS values, 0–16 px | Drawn sides share one width (`0 0 2px 0`, `1px`, `2px 0`); mixed non-zero widths are refused |
| `border-color` | `#RRGGBB`, `#RRGGBBAA`, `currentColor` | Undeclared means currentColor, as in CSS |
| `text-align` | `left`, `center`, `right` | Text leaves; inherited |
| `vertical-align` | `top`, `middle`, `bottom` | Text placement inside the leaf's content box |
| `align-items` | `flex-start`/`start`, `center`, `flex-end`/`end`, `stretch` | Children of a flex row; `stretch` fills auto-height children |
| `fill-opacity` | `0`–`1` (three decimals) or `N%` | Area under each `ui-chart` series, in the series color |
| `outline` | `none`, `auto` | Focus indicator; `none` omits it (wire major 10) |
| `background-image` | `none`, `theme(navy)`, `theme(mountains)`, `theme(dusk)`, `theme(origami)` | A shared background preset over the background color, in the box shape (wire major 11) |

The radius is clamped to half the shorter side, so a large radius makes a pill
or, on a square box, a circle (status dots, count badges). Borders are part of
the box and inset its content like padding. Inner border corners keep the outer
centres (inner radius = radius − adjacent border width, per axis, as in CSS).
Painting order is background, border, focus ring, content. A translucent
border is blended once per pixel. Text alignment uses
`font::ui_font_text_width`, the same placement every adapter draws with; text
wider than its box keeps its start edge. A column cannot
centre or end-align its children (that needs intrinsic widths); the compiler
names the declaration. Per-corner radii, percentage radii, border shorthands
(`border`, `border-bottom`), border styles and other `align-items` values are
refused with a diagnostic.

A background image is one of `appearance::BACKGROUNDS`, the presets the desktop
background uses: an opaque vertical gradient (solid when its stops match),
painted after the background color as CSS paints an image over it, one row at
a time in the box shape. A photograph preset (`Background::photo`) paints its
approximating gradient here; the photograph itself (`appearance::photo`, a
built-in JPEG) is decoded only by the kernel for the desktop, and a view that
shows it uses pre-scaled host images. `paint::vertical_gradient` interpolates each channel
with rounding; hosts painting a preset elsewhere use it too. Like a palette
token, a decoration stores only the preset's number, which keeps a retained
recipe within its 32 KiB; there are no literal gradients.

A styled `ui-value-bar` background is its track; without one the built-in
track remains. The chart area spans consecutive available samples down to the
0 % line; an unavailable sample breaks the area as it breaks the line, and an
isolated sample has none. The area is painted over the grid and under the
series line and point markers.

Decorations are interned like state styles: `Style`
carries `text_align` and a `decoration` index into `Document::decorations`
(at most 32 distinct entries per view; exhausting the table is a source
diagnostic). Unstyled zero decorations add no entry. Native controls obtain their defaults
from the SDK stylesheet even when the application supplies no CSS.

`Painter::fill_rounded` and `Painter::stroke_rounded` draw these shapes. Their
default methods emit bounded `fill_alpha` rectangles with 8×8-sampled
anti-aliased arcs, in integer arithmetic, so every existing adapter (client
band raster, compositor canvas and recipe planners) shows identical pixels
without changes. An adapter may override them only with identical coverage.
They cut the box at every edge where coverage may change: a cell no arc
crosses becomes one rectangle, and a cell an arc crosses goes to
`Painter::fill_shape(area, shape, color, clip)`. Its default emits one
rectangle per run of equal alpha. A recipe planner with a fixed primitive
budget can override it to keep one primitive per cell and evaluate
`RoundedShape::alpha(x, y)` while composing (`RoundedShape::translated` moves
the shape to the planner's origin); the pixels are the same. The OSAIKU kernel
and compositor planners do this, so a rounded box costs at most a few
primitives for each corner instead of one per anti-aliased run.

The compiler resolves eight state combinations for wide/narrow layouts, interns
changed styles into at most 128 shared palette entries, and retains per-node
inheritance flags. Runtime state changes require no parsing or heap allocation.
Exhausting the palette is an explicit source diagnostic. Unmatched declarations
are validated too, including rules for states which are currently inactive.

### Images

`<ui-icon id="logo" src="osaiku"></ui-icon>` names an image; the name (1–64
ASCII letters, digits, `-`, `_`, `.`) becomes the icon's text, so the wire
format is unchanged and a host without the image draws the square mark. An
icon with `src` takes no other text.

Images are never decoded or scaled at runtime. A build tool rasterizes each
source, preferably SVG (each size drawn from the vector by resvg), else PNG
(Lanczos on premultiplied alpha), at a fixed set of sizes into
`image::ImageSet { name, images }`, each `image::Image` straight ARGB with alpha
in the top byte. The host passes its sets with `Scene::with_images`; painting
picks the largest size that fits the icon's content box, centred. An unknown
name, or a box smaller than every size, keeps the square mark.
`Painter::image(at, image, clip)` receives exactly the image's size; its
default emits one `fill_alpha` per row run of equal pixels, skipping
transparent ones, and a recipe planner may keep the image as one primitive and
compose its pixels later with the same result. OSAIKU's shell icons are
generated by `tools/icons/generate-icons.py` from `abi/ui/icons`.

A text binding can also provide `mask8:` followed by exactly 16 hexadecimal
digits for a bounded monochrome thumbnail. Bit `y * 8 + x` selects the cell at
column x, row y; the 8×8 grid is centred in the icon box and uses its foreground
colour. This path emits at most 64 fills, has no image decoder or allocation,
and carries no resource pointer. Invalid masks keep the ordinary square mark.
This is a text-binding representation, not a `src` image name.

Under the light palette (`Theme::Light`) a set named `NAME.light`, when the
host has one, is drawn in place of `NAME`. A one-colour glyph (an arrow, a
chevron) keeps its contrast on paper without the application knowing the
theme: the document and the kernel's text verification name only `NAME`.
An application that rasterizes its own surface passes its own images to the
runtime (`HostedRaster::publish_with_images`).

## Popup menus

```html
<ui-popup-menu id="context-menu" action="context-close" bind:hidden="closed">
  <ui-list-item id="copy" action="copy"><span id="copy-label">Kopiuj</span></ui-list-item>
</ui-popup-menu>
```

A popup is a container drawn above the rest of the document, the way a
desktop context menu is drawn over the item it was opened on. It takes no
space in its parent's flow and is placed at its anchor
(`Node::set_popup_anchor(x, y)`, document pixels; the wire carries it in the
node's `value` and `scroll_y`): to the right of and below the point, or to the
left of / above it when that side has no room, always inside the viewport and
never cut by an ancestor's overflow. Its width is the CSS width (240 px when
auto), its height its content's. It cannot scroll or live in a list row.
Paint draws the document, then each open popup with a soft shadow.

While a popup with an action is open it holds input like a menu: a press on
it reaches only its items (its padding activates nothing), a press anywhere
else activates the popup itself, whose action closes it, and nothing beneath
starts a scroll or activates. Escape (`input::Key::Cancel`) sends the same
action, and focus cycles among its items. `input::context_event` hit-tests
beneath open popups, so a second secondary click on another item can move the
menu there; `input::popup_dismiss` builds the closing activation. A popup
without an action is a non-modal overlay. Hosts verify the anchor like any
other recipe field.

Text models (`text_model::Model`) may also bind `ui-value-bar` values through
`Model::value` (0..10000 basis points, `Some(None)` for unavailable); a model
without values still refuses `bind:value`.

## Encoding and ownership

Explicit little-endian stream: OSUI magic, wire major, node/text/point counts,
component records, UTF-8 bytes, points and a bounded state-style palette.
Wire major 4 retains the u16 palette count in the header, 14 palette indices and 16
inheritance flags after each node's scroll offset, and serialized palette styles
after the points. A 255 index means the base style; other indices must refer to
an admitted palette entry. Wire major 7 adds a u16 decoration count after the
palette count, two bytes after each style (`text_align`, `decoration`; 255 is
none) and, after the palette styles, one 11-byte record per decoration: radius,
border width, border sides (bit 0 top, 1 right, 2 bottom, 3 left), vertical
align, align-items, fill opacity, currentColor flag, then the ARGB border color.
Entries must be canonical (no color without a border) and never empty. Background colors now encode explicit ARGB coverage;
earlier majors are refused rather than reinterpreted. Major 8 adds two u8
token IDs after each style background (`color_token`, `background_token`) and
one u8 after each decoration border color (`border_token`, a 12-byte record).
Major 10 appends one u8 flag to each decoration (`outline_none`, a 13-byte
record); values other than 0 and 1 are refused. Major 11 appends one u8
background image (`background_image`, a 14-byte record): zero is none, `n` is
`appearance::BACKGROUNDS[n - 1]`; larger values are refused.
Zero selects the literal color; IDs 1 through 14 select the bounded SDK palette.
Unknown IDs are rejected before layout/paint.
No native struct layout, Rust enum ABI,
padding, process pointer or callback. Unknown versions/kinds/roles, trailing
bytes, invalid parents, duplicate IDs, bad ranges/styles are refused.
Limits: wire 64 KiB, nodes 128, text 8 KiB, leaf text 256 bytes, points 120;
source markup 32 KiB, styles 16 KiB, rules 96, declarations/rule 16.

`decode`/`bind` are convenience APIs with Document-sized stack temporaries.
Freestanding callers use `decode_into`/`bind_into` with separately admitted
scratch storage and publish only after success. Scratch is unspecified after
failure; the previous view stays intact. The OS adapter must account retained
and preparation allocations. Runtime methods hide no heap allocation.

`Action::validate` checks owner/window/incarnation/revision and element/action
against the **presented** recipe, including ancestor states. The caller must
authenticate its sender and supply the actual presented identity. This crate
provides `input::Queue` with sixteen entries, coalescing of consecutive absolute
scroll offsets for the same target/element, explicit full refusal otherwise, and
revalidation on delivery against an exact retained `(Identity, Document)` source.
The OS retains the current and one preceding presented recipe so a periodic
refresh cannot erase input already admitted against the preceding picture.
The event keeps its original revision; its IDs are never rebound to the new
document. The application must retain the matching data bindings. Missing
source revisions are refused. Window/geometry replacement and close revoke
queued events and release the preceding recipe.
`Identity` and `Action` have integer-only C layouts; an identity is not an
authority token. The host assigns the revision to an exact published recipe,
not merely to the application's data counter. `wire_hash` hashes the exact
encoded bytes; validate a private copy before admission.

This crate does not yet implement native syscalls, window lifecycle, snapshot permission,
or accepted/presented receipts. Semantics expose full text and clipped bounds;
the future observation adapter must enforce privacy/occlusion before disclosure.

## Build and compatibility

From an SDK-only checkout:

```text
cargo test --features osaiku-ui/compiler
cargo run -p osaiku-ui-hello
cargo run -p osaiku-ui --features compiler --bin osaiku-ui-compile -- view.ui.html view.ui.css view.ui.bin
```

In OSAIKU add `--manifest-path sdk/Cargo.toml`. The hello host program requires
no OS source. It is an early dependency check, **not a bootable external app**;
public window lifecycle is still pending. The separate
[native runtime foundation](native-runtime.md) supplies the initial syscall
subset, target/linker and an independently linked non-windowed ELF example.

OSAIKU pins this repository as a Git submodule. Initialize with
`git submodule update --init sdk` and use that same checkout for development.
Experimental 0.1.0 is not a qualified release. Reject unknown wire majors;
before stabilizing format 1, complete state-style, transport/input and native
application integration and record paired OS/SDK revisions.


The native event record is `input::Event` (56 bytes): presented identity,
element, kind, signed value, action. Kinds are ACTIVATE, FOCUS and SCROLL;
SCROLL carries an absolute clamped vertical offset (0..1048576), not wheel
units. Queue capacity is 16 with explicit overflow refusal. Delivery validates
the presented identity and ancestor states again. Host adapters authenticate
the caller and take only events for that window owner. The 48-byte
`Action` remains an activation helper; it is not the native receive buffer.


## Host-owned composition slots

The reusable [native frame and CPU effects](window-decoration.md) build on these
slots. The same SDK source now owns the frame template, resize geometry, corners,
shadow and glass; the OS retains ownership/publication and device integration.

`Layout::in_slots` / `Scene::in_slots` accept a complete slice of `HostSlot`
records for the document root's direct children. Each stable node ID occurs
exactly once; unknown, duplicate, missing and out-of-viewport slots are rejected
before layout is returned. Nested elements use normal CSS layout and inherit
hidden/disabled state. The host slot clips descendants even with visible
CSS overflow. Painting, hit testing and semantics use that same layout.
This is a trusted host placement adapter, not markup geometry or a new wire
format, and grants no lifecycle or device authority.

`Scene::with_text_binding` borrows up to eight literal text replacements for
existing text bindings (each at most `MAX_LEAF_TEXT` bytes). It rejects missing
or duplicate bindings. Paint and semantic labels see the same borrowed text;
the compiled Document remains unchanged. This is suitable for fixed single-line
host slots whose metrics do not depend on text. Use ordinary `Document::bind`
for data-driven application layouts. Text never becomes markup. Semantic
records now expose the stable parent node ID as well as clipped geometry.


Compositor epochs (UI wire version 6): `Identity` is 40 bytes, ordered as
`epoch, owner, window, incarnation, revision` (five u64 words). Pixels carry the
same epoch in `surface::Target`; frozen layout and semantic records accompany
that sealed frame. Actions, scrolling captures and preceding-revision queues
must reject a different epoch even if all other identity words match. Rebuild
OS and SDK clients together. A restart retains the application's document and
session, requests a fresh render, and accepts a new seal before remapping the
window. A retained pre-restart bitmap is not a recovery publication.

## Shared native controls and live system themes

`compiler::compile` always applies `src/controls.css` first. The System Manager
is the reference: 32px buttons with 14px centered text, 8px radius and 1px border;
12px card/panel corners, 16px padding; 36px table rows; 8px value bars.
The common classes include `tab`, `card`, `panel`, `caption`, `table-head`,
`toolbar`, `list-row`, `muted`, `danger`, `warning` and `secondary`.

Application CSS normally describes layout. It remains a higher cascade origin
than SDK defaults, independently of selector specificity. Overrides are per
property, including state and responsive rules. A smaller native button needs
only `button { height:24px; font-size:12px; }`; its remaining native skin survives.
A literal color intentionally stays unchanged across themes. To customize a
semantic color while following system preferences, use `color:theme(secondary)`
or `:root { --series:theme(secondary); }` with `color:var(--series)`.
The 96-rule/16KiB source limit applies to application CSS; SDK defaults are
separate. Rules never leak from one view into another.

Palette names: `text`, `muted`, `surface`, `card`, `line`, `track`, `selected`,
`accent`, `secondary`, `danger`, `warning`, `input`, `input-focus`, `grid`.
`Theme::Dark` preserves the current System Manager palette. `Theme::Contrast`
is the system contrast profile. Tokens are resolved once per painted node,
not per pixel. There is no runtime CSS parser, global mutable stylesheet or
second themed copy of the document. Layout, bindings and input identities are
independent of palette selection.

`appearance` holds the desktop presets as data: Dark and Light chrome colors,
four accent sets (shell fill, `accent` and `selected` per base), four
background gradients and the taskbar transparency steps. The window
decoration constants, the OSAIKU shell recipes and the Dark palette's
surface, accent and selected tokens read their values from it.

The native host puts the selected palette in `GeometryOffer.theme` (surface ABI
4, same record size, replacing the old reserved u32). `HostedRaster` uses it for
the complete publication. BEGIN and SEAL reject stale geometry/theme offers.
The OSAIKU shell currently has one palette and always offers `Theme::Dark`;
its panel theme control and live re-theming of running applications were
removed on 2026-10-11. `Theme::Contrast` remains available to SDK hosts.

Host-owned frame/panel/power recipes use `compile_unstyled` because their
explicit placement and decoration policy differ from application controls.
Custom canvas authors can also use this API; it does not bypass host permissions
or permit changing other windows. Native applications should use `compile`.


### Host-owned virtual grids

`virtual_grid::Config` plans fixed cells in a bounded row cache using the same
range planner as `virtual_list`. Supply column count, row stride, gap and total
items; `range(scroll, viewport_height, capacity)` clamps the offset and returns
the first item, item count and total extent. Bind only those items into recycled
cells and use `cell` for their full positions. Clip both painting and input to
the viewport. Keep stable item identity and the presented revision alongside
each cell; a recycled slot alone must never authorize an operation. Host-owned
grids can expose the existing SDK scrollbar by setting the container's content
height and scroll offset. I/O belongs to the provider's continuation; pointer
movement only updates/coalesces the requested range.
