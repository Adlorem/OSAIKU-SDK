# Tooltip help

`ui-tooltip` is a help icon (`?`) with a literal UTF-8 explanation of up to
256 bytes. It occupies 24 × 24 px in the normal layout. It accepts `bind:text`,
hidden and disabled bindings. Use a separate help icon alongside a disabled
control when the explanation must remain available.

```html
<div id="temperature-row">
  <p id="temperature-label">Temperatura</p>
  <ui-tooltip id="temperature-help">Niższa temperatura ogranicza losowość. Nie gwarantuje prawdziwych odpowiedzi.</ui-tooltip>
  <ui-slider id="temperature" action="temperature" bind:value="temperature"></ui-slider>
</div>
```

In a hosted SDK application, enable the built-in controller:

```rust
impl WindowApplication for Application {
    fn tooltips(&self) -> bool { true }
    // The usual application methods follow.
}
```

No app timer or tooltip action handler is needed. The runtime waits 500 ms
after entering the icon; ordinary motion within it does not restart the wait.
Keyboard focus (Tab) and activation show help immediately. Escape, leaving the
icon, another input action, hiding the anchor or changing its geometry/text
close it. The native pointer host routes hover in the focused window only.
Help does not capture the pointer, make a modal window or block controls.

The explanation is wrapped using the SDK font metrics, drawn after document
content/popups and kept inside the content viewport. It flips above the icon
when there is insufficient space below. It is not clipped to its scrolling
parent. Very small viewports clip the explanation to available space; keep
help brief. CPU and GPU use the same scene paint commands and semantics.

The SDK owns the component's `selected` bit and `ui-tooltip` action. Do not bind
`selected` or assign an application action to this component. Its semantic
text is the full explanation rather than the visible question mark. Wire major
11 keeps its existing widths; the added kind is 15. Older readers reject that
kind, so deploy the updated SDK application and native host together.

Custom hosts can use `tooltip::Hover` for target transitions and
`tooltip::State::{event,tick,interval_us,apply}` for timing and rendering.
Authenticate input against the exact presented identity before passing it to
the controller. `input::HOVER` is 7 (value 1 enter, 0 leave/dismiss); it carries
no action or raw coordinates. Keyboard focus on help carries the reserved help
action. Consecutive hover events coalesce, without overwriting clicks or other
input. No periodic timer remains active once the tooltip is visible or closed.
