#![cfg(feature = "compiler")]
use osaiku_ui::{compiler::compile, layout::Layout, *};

fn doc() -> Document {
    compile(r#"<main id="root"><button id="ok" action="accept" bind:disabled="off">Zażółć &amp; ćma</button><span id="value" bind:text="reading"></span></main>"#,
        "main { padding: 8px; gap: 4px; } button { height: 32px; }").unwrap()
}

#[test]
fn event_queue_checks_ownership_revisions_disabled_and_backpressure() {
    let mut view = doc();
    let identity = Identity {
        epoch: 1,
        owner: 8,
        window: 4,
        incarnation: 2,
        revision: 7,
    };
    let event = input::Event::activation(Action {
        target: identity,
        element: symbol("ok"),
        action: symbol("accept"),
    });
    let mut queue = input::Queue::EMPTY;
    for _ in 0..input::CAPACITY {
        queue.push_event(event, identity, &view).unwrap();
    }
    assert_eq!(queue.push_event(event, identity, &view), Err(Error::Limit));
    for _ in 0..input::CAPACITY {
        assert_eq!(queue.take_event(&[(identity, &view)]), Some(event));
    }
    assert!(queue.is_empty());
    queue.push_event(event, identity, &view).unwrap();
    assert!(queue
        .take_event(&[(
            Identity {
                revision: 8,
                ..identity
            },
            &view
        )])
        .is_none());
    assert_eq!(
        queue.push_event(
            event,
            Identity {
                epoch: 1,
                owner: 9,
                ..identity
            },
            &view
        ),
        Err(Error::Owner)
    );
    view.nodes[0].state = state::DISABLED;
    assert_eq!(
        queue.push_event(event, identity, &view),
        Err(Error::Disabled)
    );
    view.nodes[0].state = 0;
    queue.push_event(event, identity, &view).unwrap();
    view.nodes[0].state = state::HIDDEN;
    assert!(queue.take_event(&[(identity, &view)]).is_none());
}

#[test]
fn admitted_input_keeps_its_original_revision_through_a_refresh() {
    let before = doc();
    let mut after = doc();
    let old = Identity {
        epoch: 1,
        owner: 8,
        window: 4,
        incarnation: 2,
        revision: 7,
    };
    let new = Identity { revision: 8, ..old };
    let event = input::Event::activation(Action {
        target: old,
        element: symbol("ok"),
        action: symbol("accept"),
    });
    let mut queue = input::Queue::EMPTY;
    queue.push_event(event, old, &before).unwrap();
    // Replacement reuses the element ID with a different meaning. It cannot
    // rewrite an already admitted event or provide authority for its old ID.
    after.nodes[1].action = symbol("different");
    assert_eq!(
        queue.take_event(&[(new, &after), (old, &before)]),
        Some(event)
    );
    queue.push_event(event, old, &before).unwrap();
    assert_eq!(queue.take_event(&[(new, &after)]), None);
    assert_eq!(queue.push_event(event, new, &after), Err(Error::Stale));
    let latest = input::Event::activation(Action {
        target: new,
        element: symbol("ok"),
        action: symbol("different"),
    });
    queue.push_event(event, old, &before).unwrap();
    queue.push_event(latest, new, &after).unwrap();
    let sources = [(new, &after), (old, &before)];
    assert_eq!(queue.take_event(&sources), Some(event));
    assert_eq!(queue.take_event(&sources), Some(latest));
    assert!(queue.is_empty());
}

#[test]
fn packaged_and_dynamic_views_have_one_canonical_encoding() {
    let d = doc();
    let mut bytes = [0; MAX_WIRE];
    let len = d.encode(&mut bytes).unwrap();
    let decoded = Document::decode(&bytes[..len]).unwrap();
    assert_eq!(decoded, d);
    assert_eq!(decoded.text_of(1), "Zażółć & ćma");
    for end in 0..len {
        assert!(
            Document::decode(&bytes[..end]).is_err(),
            "truncation at {end}"
        );
    }
    bytes[len] = 0;
    assert_eq!(Document::decode(&bytes[..len + 1]), Err(Error::Buffer));
    bytes[4] = 99;
    assert_eq!(Document::decode(&bytes[..len]), Err(Error::Version));
}

struct Data {
    fail: bool,
}
impl Bindings for Data {
    fn get(&self, name: u32) -> Option<Value<'_>> {
        if name == symbol("off") {
            Some(Value::Bool(true))
        } else if name == symbol("reading") && !self.fail {
            Some(Value::Text("<button action=\"delete\">Dane</button>"))
        } else {
            None
        }
    }
}
#[test]
fn failed_binding_is_atomic_and_data_never_becomes_markup() {
    let d = doc();
    let before = d.clone();
    let mut next = Document::EMPTY;
    assert_eq!(
        d.bind_into(&Data { fail: true }, &mut next),
        Err(Error::Binding)
    );
    assert_eq!(d, before);
    d.bind_into(&Data { fail: false }, &mut next).unwrap();
    assert_eq!(next.len, 3);
    assert_eq!(next.nodes[2].action, 0);
    assert!(next.text_of(2).starts_with("<button"));
    assert_eq!(next.nodes[1].state, state::DISABLED);
}

#[test]
fn cascade_inherits_text_and_applies_specificity_per_property() {
    let d = compile(r#"<main id="root"><span id="reading" class="metric">1</span></main>"#,
        ":root { --text: #abcdef; } main { color: var(--text); font-size: 20px; } #reading { padding: 4px; } .metric { padding: 8px; height: 32px; } span { height: 16px; } @media (max-width: 560px) { .metric { height: 48px; } }").unwrap();
    assert_eq!(d.nodes[1].style.padding, 4);
    assert_eq!(d.nodes[1].style.height, 32);
    assert_eq!(d.nodes[1].narrow.height, 48);
    assert_eq!(d.nodes[1].style.color, 0xabcdef);
    assert_eq!(d.nodes[1].style.font_size, 20);
    let small = Layout::compute(&d, 480, 360, &[0; MAX_NODES]).unwrap();
    let large = Layout::compute(&d, 800, 600, &[0; MAX_NODES]).unwrap();
    assert_eq!(small.nodes[1].bounds.height, 48);
    assert_eq!(large.nodes[1].bounds.height, 32);
}

#[test]
fn invalid_markup_and_css_report_source_locations() {
    let err = compile(
        "<main id=\"root\">\n  <script id=\"bad\"></script></main>",
        "",
    )
    .unwrap_err();
    assert_eq!((err.source, err.line), ("markup", 2));
    assert!(err.column > 2);
    let err = compile("<main id=\"root\"></main>", "main {\n  position: fixed;\n}").unwrap_err();
    assert_eq!((err.source, err.line, err.column), ("styles", 2, 3));
    for markup in [
        "<main id=\"a\"><span id=\"a\"></span></main>",
        "<main id=\"a\" id=\"b\"></main>",
        "<main id=\"a\"></section>",
        "<main id=\"a\"></main><main id=\"b\"></main>",
        "<main id=\"a\"><button id=\"b\"><span id=\"c\"></span></button></main>",
        "<main id=\"a\" bind:text=\"data\"></main>",
        "<main id=\"a\"><span id=\"b\" bind:value=\"data\"></span></main>",
        "<main id=\"a\"><span id=\"b\" action=\"run\"></span></main>",
    ] {
        assert!(compile(markup, "").is_err(), "{markup}");
    }
    for css in [
        "main { width: -2px; }",
        "main { height: 999999px; }",
        "main { padding: 257px; }",
        "main { color: var(--missing); }",
        ":root { --a: var(--b); --b: var(--a); } main { color: var(--a); }",
        ".unused { invalid: yes; }",
        "main:hover { color: #ffffff; }",
        "@media (max-width: 0px) { main { padding: 2px; } }",
    ] {
        assert!(compile("<main id=\"a\"></main>", css).is_err(), "{css}");
    }
}

#[test]
fn limits_and_untrusted_topology_fail_without_panics() {
    let mut markup = String::new();
    for i in 0..=MAX_DEPTH {
        markup.push_str(&format!("<main id=\"n{i}\">"));
    }
    assert!(compile(&markup, "").is_err());
    let mut d = doc();
    d.nodes[1].parent = 2;
    assert_eq!(d.validate(), Err(Error::Structure));
    let mut d = doc();
    d.nodes[1].text_start = u16::MAX;
    assert_eq!(d.validate(), Err(Error::Text));
    assert!(compile(&"a".repeat(32769), "").is_err());
    assert!(compile("<main id=\"a\"></main>", &" ".repeat(16385)).is_err());
}

#[test]
fn clipping_scroll_and_disabled_ancestors_control_hit_and_focus() {
    let mut d = compile(r#"<main id="root"><section id="scroll"><button id="one" action="one">One</button><button id="two" action="two">Two</button></section></main>"#,
        "section { height: 32px; overflow: auto; } button { height: 32px; }").unwrap();
    let l = Layout::compute(&d, 480, 360, &[0; MAX_NODES]).unwrap();
    assert_eq!(l.hit(&d, 10, 10), Some(2));
    assert_eq!(l.hit(&d, 10, 40), None);
    assert_eq!(l.focus_next(&d, Some(2), false), Some(2));
    let mut scroll = [0; MAX_NODES];
    scroll[1] = u32::MAX;
    let l = Layout::compute(&d, 480, 360, &scroll).unwrap();
    assert_eq!(l.nodes[1].scroll_y, 32);
    assert_eq!(l.hit(&d, 10, 10), Some(3));
    d.nodes[1].state = state::DISABLED;
    let l = Layout::compute(&d, 480, 360, &scroll).unwrap();
    assert_eq!(l.hit(&d, 10, 10), None);
    assert_eq!(l.focus_next(&d, None, false), None);
}

#[test]
fn layout_preserves_noncontiguous_children_hidden_flow_and_nested_height() {
    let mut view = Document::EMPTY;
    view.len = 6;
    for i in 0..6 {
        view.nodes[i].id = (i + 1) as u32;
        view.nodes[i].parent = if i == 0 { NONE } else { 0 };
    }
    // Root children are A, B; A's children are C, D; C contains E. The wire
    // format requires parent-before-child, not contiguous/preorder subtrees.
    view.nodes[1].style.padding = 2;
    view.nodes[1].style.gap = 3;
    view.nodes[2].style.height = 10;
    view.nodes[3].parent = 1;
    view.nodes[3].style.padding = 1;
    view.nodes[4].parent = 1;
    view.nodes[4].style.height = 50;
    view.nodes[4].state = state::HIDDEN;
    view.nodes[5].parent = 3;
    view.nodes[5].kind = Kind::Button;
    view.nodes[5].role = Role::Button;
    view.nodes[5].action = 1;
    view.nodes[5].style.height = 20;
    let layout = Layout::compute(&view, 200, 100, &[0; MAX_NODES]).unwrap();
    assert_eq!(layout.nodes[1].bounds.height, 26);
    assert_eq!(layout.nodes[2].bounds.y, 26);
    assert_eq!(layout.nodes[3].bounds.height, 22);
    assert_eq!(
        layout.nodes[5].bounds,
        layout::Rect {
            x: 3,
            y: 3,
            width: 194,
            height: 20
        }
    );
    assert!(layout.nodes[4].hidden);
    assert_eq!(layout.hit(&view, 4, 4), Some(5));
    view.nodes[4].state = 0;
    let layout = Layout::compute(&view, 200, 100, &[0; MAX_NODES]).unwrap();
    assert_eq!(layout.nodes[1].bounds.height, 79);
    assert_eq!(layout.nodes[2].bounds.y, 79);
    assert_eq!(layout.nodes[4].bounds.y, 27);
}

#[test]
fn actions_bind_owner_incarnation_revision_and_disabled_state() {
    let mut d = doc();
    let id = Identity {
        epoch: 1,
        owner: 1,
        window: 2,
        incarnation: 3,
        revision: 4,
    };
    let a = Action {
        target: id,
        element: symbol("ok"),
        action: symbol("accept"),
    };
    assert_eq!(a.validate(id, &d), Ok(()));
    assert_eq!(
        a.validate(Identity { revision: 5, ..id }, &d),
        Err(Error::Stale)
    );
    assert_eq!(
        a.validate(
            Identity {
                incarnation: 5,
                ..id
            },
            &d
        ),
        Err(Error::Owner)
    );
    d.nodes[0].state = state::DISABLED;
    assert_eq!(a.validate(id, &d), Err(Error::Disabled));
}

#[test]
fn series_preserve_missing_values_and_reject_nonmonotonic_time() {
    let mut d = compile(
        r#"<main id="r"><ui-chart id="c" bind:series="series"></ui-chart></main>"#,
        "",
    )
    .unwrap();
    struct Series([Point; 3]);
    impl Bindings for Series {
        fn get(&self, _: u32) -> Option<Value<'_>> {
            Some(Value::Series(&self.0))
        }
    }
    let mut series = Series([
        Point {
            time_ms: 100,
            value: 4500,
        },
        Point {
            time_ms: 900,
            value: 10001,
        },
        Point {
            time_ms: 5100,
            value: 0,
        },
    ]);
    let mut bound = Document::EMPTY;
    d.bind_into(&series, &mut bound).unwrap();
    assert_eq!(bound.points[1].value, 10001);
    let previous = bound.clone();
    series.0[2].time_ms = 900;
    assert_eq!(bound.bind_into(&series, &mut d), Err(Error::Value));
    assert_eq!(bound, previous);
}

#[test]
fn painting_input_and_semantics_share_clipped_bounds() {
    use osaiku_ui::{
        layout::Rect,
        paint::{Painter, Scene},
    };
    #[derive(Default)]
    struct Capture {
        clips: Vec<Rect>,
    }
    impl Painter for Capture {
        fn fill_alpha(&mut self, _: Rect, _: u32, _: u8, clip: Rect) {
            self.clips.push(clip);
        }
        fn text(&mut self, _: Rect, _: &str, _: u16, _: u32, clip: Rect) {
            self.clips.push(clip);
        }
        fn line(&mut self, _: (i32, i32), _: (i32, i32), _: u32, clip: Rect) {
            self.clips.push(clip);
        }
    }
    let view = compiler::compile_unstyled(
        r#"<main id="r"><button id="b" action="go">Go</button></main>"#,
        "main { padding: 8px; } button { height: 48px; background-color: #123456; }",
    )
    .unwrap();
    let scene = Scene::new(&view, 100, 40, &[0; MAX_NODES]).unwrap();
    let semantic = scene.semantics().find(|s| s.id == symbol("b")).unwrap();
    assert!(semantic.clipped);
    assert_eq!(semantic.visible_bounds.height, 32);
    assert_eq!(scene.hit(9, 39), Some(semantic.id));
    assert_eq!(scene.hit(9, 40), None);
    let mut capture = Capture::default();
    scene.paint(&mut capture);
    assert_eq!(capture.clips, vec![semantic.visible_bounds; 2]);
}

#[test]
fn zero_lengths_are_not_auto_and_flexible_content_preserves_navigation() {
    let view = compile(r#"<main id="r"><nav id="n"><button id="b" action="go">Go</button></nav><section id="s"><span id="zero">Zero</span><p id="long">Text</p></section><footer id="f"><span id="end">End</span></footer></main>"#,
        "main { display: flex; flex-direction: column; } button { height: 32px; } section { flex-grow: 1; overflow: auto; } #zero { width: 0px; height: 0px; } #long { height: 400px; }").unwrap();
    let l = Layout::compute(&view, 200, 100, &[0; MAX_NODES]).unwrap();
    assert_eq!(l.nodes[4].bounds.width, 0);
    assert_eq!(l.nodes[4].bounds.height, 0);
    assert_eq!(l.nodes[3].bounds.y, 32);
    assert_eq!(l.nodes[6].bounds.bottom(), 100);
    assert_eq!(l.nodes[3].content_height, 400);
}

#[test]
fn mutation_corpus_never_panics_or_admits_a_noncanonical_view() {
    let styled = compile(
        r#"<main id="r"><button id="b" action="go">Go</button></main>"#,
        "button:focus { color: #123456; } button:selected { padding: 8px; }",
    )
    .unwrap();
    for d in [doc(), styled] {
        let mut bytes = [0; MAX_WIRE];
        let len = d.encode(&mut bytes).unwrap();
        for offset in 0..len {
            let old = bytes[offset];
            for byte in [0, 0xff, 0x80] {
                bytes[offset] = byte;
                if let Ok(decoded) = Document::decode(&bytes[..len]) {
                    decoded.validate().unwrap();
                }
            }
            bytes[offset] = old;
        }
    }
}

#[test]
fn scroll_track_is_painted_last_and_clicks_round_trip_in_the_recipe() {
    use osaiku_ui::{
        input,
        layout::Rect,
        paint::{Painter, Scene},
    };
    struct Capture(Vec<(Rect, u32)>);
    impl Painter for Capture {
        fn fill_alpha(&mut self, rect: Rect, color: u32, _: u8, _: Rect) {
            self.0.push((rect, color));
        }
        fn text(&mut self, _: Rect, _: &str, _: u16, _: u32, _: Rect) {}
        fn line(&mut self, _: (i32, i32), _: (i32, i32), _: u32, _: Rect) {}
    }
    fn hull(fills: &[(Rect, u32)]) -> Rect {
        let left = fills.iter().map(|(r, _)| r.x).min().unwrap();
        let top = fills.iter().map(|(r, _)| r.y).min().unwrap();
        let right = fills.iter().map(|(r, _)| r.right()).max().unwrap();
        let bottom = fills.iter().map(|(r, _)| r.bottom()).max().unwrap();
        Rect {
            x: left,
            y: top,
            width: (right - left) as u32,
            height: (bottom - top) as u32,
        }
    }
    let mut view = compile(r#"<main id="root"><div id="scroll"><button id="one" action="go">Go</button><span id="long">Long</span></div></main>"#,
        "#scroll { height: 100px; padding: 8px; overflow: auto; } #long { height: 400px; background-color: #123456; }").unwrap();
    let identity = Identity {
        epoch: 1,
        owner: 1,
        window: 2,
        incarnation: 3,
        revision: 4,
    };
    let scene = Scene::from_document(&view, 200, 120).unwrap();
    let bar = scene.scrollbar(1).unwrap();
    let mut capture = Capture(Vec::new());
    scene.paint(&mut capture);
    // The rounded track, then the rounded thumb, are the final fills.
    let fills = &capture.0;
    let color_run = |end: usize| {
        let color = fills[end - 1].1;
        fills[..end].iter().rposition(|(_, c)| *c != color).unwrap() + 1
    };
    let thumb = color_run(fills.len());
    let track = color_run(thumb);
    assert_eq!(hull(&fills[thumb..]), bar.thumb);
    assert_eq!(hull(&fills[track..thumb]), bar.track);
    let event = input::pointer_event(
        &view,
        &scene,
        identity,
        bar.track.x + 2,
        bar.track.bottom() - 1,
    )
    .unwrap();
    assert_eq!(event.kind, input::SCROLL);
    assert_eq!(event.value as u32, bar.maximum);
    let mut queue = input::Queue::EMPTY;
    queue.push_event(event, identity, &view).unwrap();
    assert_eq!(queue.take_event(&[(identity, &view)]), Some(event));
    view.nodes[1].scroll_y = event.value as u32;
    let mut bytes = [0; MAX_WIRE];
    let len = view.encode(&mut bytes).unwrap();
    let decoded = Document::decode(&bytes[..len]).unwrap();
    let scene = Scene::from_document(&decoded, 200, 120).unwrap();
    assert_eq!(scene.layout().nodes[1].scroll_y, event.value as u32);
    let up = input::key_event(&decoded, &scene, identity, input::Key::PageUp).unwrap();
    assert!(up.value < event.value);
    bytes[4..6].copy_from_slice(&1u16.to_le_bytes());
    assert_eq!(Document::decode(&bytes[..len]), Err(Error::Version));
}

#[test]
fn focus_and_scroll_events_reject_disabled_ancestors_and_stale_owners() {
    use osaiku_ui::input;
    let mut view = compile(
        r#"<main id="r"><div id="s"><button id="b" action="go">Go</button></div></main>"#,
        "#s { overflow: auto; }",
    )
    .unwrap();
    let identity = Identity {
        epoch: 1,
        owner: 1,
        window: 2,
        incarnation: 3,
        revision: 4,
    };
    for (kind, element, value) in [
        (input::FOCUS, symbol("b"), 0),
        (input::SCROLL, symbol("s"), 20),
    ] {
        let event = input::Event {
            target: identity,
            element,
            kind,
            value,
            action: 0,
        };
        event.validate(identity, &view).unwrap();
        let mut stale = identity;
        stale.revision += 1;
        assert_eq!(event.validate(stale, &view), Err(Error::Stale));
        stale = identity;
        stale.owner += 1;
        assert_eq!(event.validate(stale, &view), Err(Error::Owner));
        view.nodes[0].state = state::DISABLED;
        assert_eq!(event.validate(identity, &view), Err(Error::Disabled));
        view.nodes[0].state = 0;
    }
}

#[test]
fn thumb_drag_survives_repaint_clamps_outside_and_cancels_on_retarget() {
    use input::PointerPhase::*;
    let mut view = compile(
        r#"<main id="root"><div id="scroll"><span id="long">Long</span></div></main>"#,
        "#scroll { height: 100px; overflow: auto; } #long { height: 400px; }",
    )
    .unwrap();
    let mut target = Identity {
        epoch: 1,
        owner: 1,
        window: 2,
        incarnation: 3,
        revision: 4,
    };
    let mut pointer = input::Pointer::EMPTY;
    let scene = paint::Scene::from_document(&view, 200, 120).unwrap();
    let bar = scene.scrollbar(1).unwrap();
    let x = bar.thumb.x + 2;
    let y = bar.thumb.y + 2;
    assert_eq!(
        pointer
            .event(&view, &scene, target, Press, x, y)
            .unwrap()
            .value,
        0
    );
    assert!(pointer.captured());
    let moved = pointer
        .event(&view, &scene, target, Move, x, y + 25)
        .unwrap();
    assert_eq!(moved.value, 100);
    view.nodes[1].scroll_y = moved.value as u32;
    target.revision += 1;
    let scene = paint::Scene::from_document(&view, 200, 120).unwrap();
    let end = pointer
        .event(&view, &scene, target, Move, -1000, i32::MAX)
        .unwrap();
    assert_eq!(end.target, target);
    assert_eq!(end.value as u32, bar.maximum);
    let mut queue = input::Queue::EMPTY;
    for offset in 0..100 {
        queue
            .push_event(
                input::Event {
                    value: offset,
                    ..end
                },
                target,
                &view,
            )
            .unwrap();
    }
    assert_eq!(queue.take_event(&[(target, &view)]).unwrap().value, 99);
    assert!(queue.is_empty());
    assert_eq!(
        pointer
            .event(&view, &scene, target, Release, x, -1000)
            .unwrap()
            .value,
        0
    );
    assert!(!pointer.captured());
    assert!(pointer.event(&view, &scene, target, Move, x, y).is_none());
    pointer.event(&view, &scene, target, Press, x, y).unwrap();
    let resized = paint::Scene::from_document(&view, 240, 120).unwrap();
    assert!(pointer.event(&view, &resized, target, Move, x, y).is_none());
    assert!(!pointer.captured());
    pointer.event(&view, &scene, target, Press, x, y).unwrap();
    target.incarnation += 1;
    assert!(pointer.event(&view, &scene, target, Move, x, y).is_none());
    assert!(!pointer.captured());
    pointer.event(&view, &scene, target, Press, x, y).unwrap();
    assert!(pointer.event(&view, &scene, target, Cancel, x, y).is_none());
    assert!(!pointer.captured());
}

#[test]
fn background_alpha_is_explicit_and_old_wire_versions_are_rejected() {
    let view = compile(
        r#"<main id="root"><div id="panel"></div></main>"#,
        "main { background-color: #123456; } #panel { background-color: #20406080; }",
    )
    .unwrap();
    assert_eq!(view.nodes[0].style.background, 0xff123456);
    assert_eq!(view.nodes[1].style.background, 0x80204060);
    let mut bytes = [0; MAX_WIRE];
    let len = view.encode(&mut bytes).unwrap();
    assert_eq!(Document::decode(&bytes[..len]).unwrap(), view);
    bytes[4..6].copy_from_slice(&3u16.to_le_bytes());
    assert_eq!(Document::decode(&bytes[..len]), Err(Error::Version));
    assert!(compile(r#"<main id="r"></main>"#, "main { color: #12345680; }").is_err());
}

#[test]
fn state_cascade_uses_css_specificity_combinations_and_responsive_rules() {
    let mut view = compile(
        r#"<main id="r"><button id="b" class="control" action="go">Go</button></main>"#,
        "#b { color: #112233; } .control:focus { color: #ffffff; height: 40px; }
         button:selected { background-color: #223344; } button:focus { background-color: #334455; }
         button:selected:focus { background-color: #445566; }
         @media (max-width: 300px) { button:focus { height: 56px; } }",
    )
    .unwrap();
    view.nodes[1].state = state::FOCUSED | state::SELECTED;
    for width in [300, 301] {
        let scene = paint::Scene::from_document(&view, width, 100).unwrap();
        let style = scene.layout().style(&view, 1);
        assert_eq!(
            style.color, 0x112233,
            "an ID wins over a class plus pseudo-class"
        );
        assert_eq!(
            style.background, 0xff445566,
            "combined selector outranks a single state"
        );
        // .control:focus outranks button:focus even inside a media rule.
        assert_eq!(style.height, 40);
        assert_eq!(scene.layout().nodes[1].bounds.height, 40);
        assert_eq!(scene.hit(1, 39), Some(symbol("b")));
        assert_eq!(scene.hit(1, 40), None);
    }
    let mut responsive = compile(r#"<main id="r"><button id="b" action="go">Go</button></main>"#,
        "button:focus { height: 40px; } @media (max-width: 300px) { button:focus { height: 56px; } }").unwrap();
    responsive.nodes[1].state = state::FOCUSED;
    assert_eq!(
        paint::Scene::from_document(&responsive, 300, 100)
            .unwrap()
            .layout()
            .nodes[1]
            .bounds
            .height,
        56
    );
    assert_eq!(
        paint::Scene::from_document(&responsive, 301, 100)
            .unwrap()
            .layout()
            .nodes[1]
            .bounds
            .height,
        40
    );
}

#[test]
fn dynamic_state_inheritance_drives_text_layout_paint_and_input_together() {
    use layout::Rect;
    use paint::Painter;
    struct Capture {
        text: Vec<(String, u16, u32)>,
        fills: Vec<u32>,
    }
    impl Painter for Capture {
        fn fill_alpha(&mut self, _: Rect, color: u32, _: u8, _: Rect) {
            self.fills.push(color);
        }
        fn text(&mut self, _: Rect, text: &str, size: u16, color: u32, _: Rect) {
            self.text.push((text.into(), size, color));
        }
        fn line(&mut self, _: (i32, i32), _: (i32, i32), _: u32, _: Rect) {}
    }
    let mut view = compiler::compile_unstyled(r#"<main id="r"><div id="panel"><button id="b" action="go">Go</button><span id="explicit">Own</span></div></main>"#,
        "#panel { color: #112233; } #panel:selected { color: #abcdef; font-size: 28px; }
         button:disabled { background-color: #445566; } #explicit { color: #123456; font-size: 16px; }").unwrap();
    view.nodes[1].state = state::SELECTED | state::DISABLED;
    let scene = paint::Scene::from_document(&view, 400, 200).unwrap();
    assert_eq!(scene.layout().style(&view, 2).color, 0xabcdef);
    assert_eq!(scene.layout().style(&view, 2).font_size, 28);
    assert_eq!(scene.layout().style(&view, 3).color, 0x123456);
    assert_eq!(scene.layout().style(&view, 3).font_size, 16);
    assert!(scene.hit(1, 1).is_none());
    assert!(scene.layout().focus_next(&view, None, false).is_none());
    let mut capture = Capture {
        text: Vec::new(),
        fills: Vec::new(),
    };
    scene.paint(&mut capture);
    assert!(capture.text.contains(&(String::from("Go"), 28, 0xabcdef)));
    assert!(capture.fills.contains(&0x445566));
    assert_eq!(
        scene
            .semantics()
            .find(|s| s.id == symbol("b"))
            .unwrap()
            .state
            & state::DISABLED,
        state::DISABLED
    );
    // Explicit child properties override inherited state styles regardless of
    // the parent's selector specificity; removing the state restores defaults.
    view.nodes[1].state = 0;
    let scene = paint::Scene::from_document(&view, 400, 200).unwrap();
    assert_eq!(scene.layout().style(&view, 2).color, 0x112233);
    assert_eq!(scene.layout().style(&view, 2).font_size, 16);
    assert!(scene.hit(1, 1).is_some());
}

#[test]
fn state_palettes_survive_binding_and_wire_and_reject_bad_references() {
    struct Selected;
    impl Bindings for Selected {
        fn get(&self, _: u32) -> Option<Value<'_>> {
            Some(Value::Bool(true))
        }
    }
    let mut view = compile(
        r#"<main id="r"><button id="b" bind:selected="chosen">Selected</button></main>"#,
        "button:selected { background-color: #abcdef; }",
    )
    .unwrap();
    let template = view;
    view = Document::EMPTY;
    template.bind_into(&Selected, &mut view).unwrap();
    let mut bytes = [0; MAX_WIRE];
    let len = view.encode(&mut bytes).unwrap();
    let decoded = Document::decode(&bytes[..len]).unwrap();
    assert_eq!(view, decoded);
    assert_eq!(
        paint::Scene::from_document(&decoded, 100, 100)
            .unwrap()
            .layout()
            .style(&decoded, 1)
            .background,
        0xffabcdef
    );
    // Wire 11 already includes native styles, decorations and virtual lists.
    // Pin its existing retained size; full Documents belong in process storage,
    // not the old 32 KiB stack temporary. OS image budgets are checked at build.
    assert_eq!(core::mem::size_of::<Document>(), 50320);
    for version in [1u16, 2] {
        bytes[4..6].copy_from_slice(&version.to_le_bytes());
        assert_eq!(Document::decode(&bytes[..len]), Err(Error::Version));
    }
    view.nodes[1].state_styles[0] = view.state_style_len as u8;
    assert_eq!(view.validate(), Err(Error::Structure));
    view.nodes[1].state_styles[0] = NO_STATE_STYLE;
    view.nodes[1].inherited[0] = 8;
    assert_eq!(view.validate(), Err(Error::Structure));
}

#[test]
fn unsupported_states_and_palette_exhaustion_report_style_diagnostics() {
    for css in [
        "button:hover { color: #123456; }",
        "button:focus:focus { color: #123456; }",
        "button:focus { transform: none; }",
        "button:not(:disabled) { color: #123456; }",
    ] {
        let error = compile(r#"<main id="r"><button id="b">Go</button></main>"#, css).unwrap_err();
        assert_eq!(error.source, "styles");
        assert!(error.line > 0 && error.column > 0);
    }
    let mut html = String::from("<main id=\"r\">");
    let mut css = String::new();
    for i in 0..65 {
        html.push_str(&format!("<button id=\"b{i}\">B</button>"));
        css.push_str(&format!("#b{i} {{ width: {}px; }}", i + 1));
    }
    html.push_str("</main>");
    css.push_str(":focus { color: #112233; } :selected { background-color: #445566; }");
    let error = compile(&html, &css).unwrap_err();
    assert_eq!(error.source, "styles");
    assert!(error.message.contains("palette"));
}

#[test]
fn state_dependent_scroll_is_validated_from_resolved_styles() {
    let mut view = compile(
        r#"<main id="r"><div id="scroll"><span id="long">Long</span></div></main>"#,
        "#long { height: 400px; } #scroll:selected { height: 80px; overflow: auto; }",
    )
    .unwrap();
    let target = Identity {
        epoch: 1,
        owner: 1,
        window: 2,
        incarnation: 3,
        revision: 4,
    };
    view.nodes[1].state = state::SELECTED;
    let scene = paint::Scene::from_document(&view, 200, 200).unwrap();
    let event = input::key_event(&view, &scene, target, input::Key::PageDown).unwrap();
    assert_eq!(event.kind, input::SCROLL);
    event.validate(target, &view).unwrap();
    view.nodes[1].state = 0;
    assert_eq!(event.validate(target, &view), Err(Error::Value));
}

#[test]
fn focus_assignment_clears_old_state_and_bounds_invalid_parent_paths() {
    let mut view = compile(r#"<main id="root"><section id="group"><button id="go" action="go">Go</button></section><button id="other" action="other">Other</button></main>"#, "").unwrap();
    assert!(view.set_focus(symbol("other")));
    assert!(view.set_focus(symbol("go")));
    assert_eq!(view.nodes[3].state & state::FOCUSED, 0);
    for blocked in [state::HIDDEN, state::DISABLED] {
        view.nodes[1].state = blocked;
        assert!(!view.set_focus(symbol("go")));
        assert_eq!(view.nodes[2].state & state::FOCUSED, 0);
        view.nodes[1].state = 0;
        assert!(view.set_focus(symbol("go")));
    }
    assert!(!view.set_focus(symbol("root"))); // Noninteractive.
    assert!(!view.set_focus(0));
    for invalid in [2, 127, NONE] {
        view.nodes[2].parent = invalid;
        assert!(!view.set_focus(symbol("go")));
        assert_eq!(view.nodes[2].state & state::FOCUSED, 0);
    }
    view.len = u16::MAX;
    assert!(!view.set_focus(symbol("go")));
}

#[test]
fn compositor_epoch_rejects_old_actions_at_the_same_content_revision() {
    let old = Identity {
        epoch: 1,
        owner: 1,
        window: 2,
        incarnation: 3,
        revision: 4,
    };
    let current = Identity { epoch: 2, ..old };
    let view = Document::EMPTY;
    let event = osaiku_ui::input::Event {
        target: old,
        kind: osaiku_ui::input::FOCUS,
        element: 1,
        value: 0,
        action: 0,
    };
    // Use a valid one-node document so rejection is specifically freshness.
    let mut view = view;
    view.len = 1;
    view.nodes[0].id = 1;
    view.nodes[0].parent = osaiku_ui::NONE;
    assert_eq!(event.validate(current, &view), Err(Error::Stale));
    assert_eq!(
        Action {
            target: old,
            element: 1,
            action: 1
        }
        .validate(current, &view),
        Err(Error::Stale)
    );
}
