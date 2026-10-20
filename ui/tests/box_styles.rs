#![cfg(feature = "compiler")]
//! Box decoration, alignment and chart-area styles (wire major 7).
use core::fmt::Write;
use osaiku_ui::{
    appearance::BACKGROUNDS,
    compiler::compile_unstyled as compile,
    font::ui_font_text_width,
    layout::{Layout, Rect},
    paint::{vertical_gradient, Painter, Scene},
    raster, state, symbol, text_model, Bindings, Decoration, Document, Error, Point, Value,
    MAX_DECORATIONS, MAX_WIRE, NO_DECORATION, VERSION,
};

fn index(view: &Document, id: &str) -> usize {
    view.nodes[..view.len as usize]
        .iter()
        .position(|n| n.id == symbol(id))
        .unwrap()
}

/// Production band rasterizer, `band_rows` rows per call.
fn pixels(view: &Document, width: u32, height: u32, band_rows: u32) -> Vec<u32> {
    let mut layout = Layout::EMPTY;
    let scene = Scene::prepare(view, width, height, &mut layout).unwrap();
    let stride = (width * 4) as usize;
    let mut bytes = vec![0; stride * height as usize];
    for (i, band) in bytes.chunks_mut(stride * band_rows as usize).enumerate() {
        raster::rows(&scene, i as u32 * band_rows, band).unwrap();
    }
    bytes
        .chunks_exact(4)
        .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
        .collect()
}

#[derive(Default)]
struct Calls(Vec<String>);
impl Painter for Calls {
    fn fill_alpha(&mut self, r: Rect, color: u32, alpha: u8, clip: Rect) {
        self.0
            .push(format!("fill {r:?} {color:x} {alpha} {clip:?}"));
    }
    fn text(&mut self, r: Rect, text: &str, size: u16, color: u32, clip: Rect) {
        self.0
            .push(format!("text {r:?} {text} {size} {color:x} {clip:?}"));
    }
    fn line(&mut self, from: (i32, i32), to: (i32, i32), color: u32, clip: Rect) {
        self.0
            .push(format!("line {from:?} {to:?} {color:x} {clip:?}"));
    }
}

#[test]
fn box_properties_intern_one_canonical_decoration_per_appearance() {
    let view = compile(
        r#"<main id="r"><div id="a" class="card"></div><div id="b" class="card"></div><button id="tab" action="go">Tab</button><span id="plain">x</span></main>"#,
        ".card { border-radius: 12px; border-width: 1px; border-color: #ffffff1a; }
         button { border-width: 0 0 2px 0; vertical-align: middle; }
         button:focus { border-color: currentColor; }
         #plain { border-radius: 0px; border-width: 0; fill-opacity: 0; }",
    )
    .unwrap();
    let card = view.nodes[index(&view, "a")].style;
    assert_eq!(
        view.decoration(card),
        Decoration {
            radius: 12,
            border_width: 1,
            border_sides: 15,
            vertical_align: 0,
            align_items: 0,
            fill_opacity: 0,
            border_current_color: false,
            border_color: 0x1aff_ffff,
            border_token: 0,
            outline_none: false,
            background_image: 0,
        }
    );
    assert_eq!(
        card.decoration,
        view.nodes[index(&view, "b")].style.decoration
    );
    // An undeclared border color is the text color, as in CSS; declaring it
    // in the focus state is therefore no new appearance.
    let tab = view.nodes[index(&view, "tab")];
    let decoration = view.decoration(tab.style);
    assert_eq!(
        (
            decoration.border_width,
            decoration.border_sides,
            decoration.vertical_align,
            decoration.border_current_color
        ),
        (2, 4, 1, true)
    );
    assert!(tab.state_styles.iter().all(|s| *s == u8::MAX));
    // Zero values are the default appearance, not table entries.
    assert_eq!(
        view.nodes[index(&view, "plain")].style.decoration,
        NO_DECORATION
    );
    assert_eq!(view.decoration_len, 2);

    let mut bytes = [0; MAX_WIRE];
    let len = view.encode(&mut bytes).unwrap();
    assert_eq!(Document::decode(&bytes[..len]).unwrap(), view);
    bytes[4..6].copy_from_slice(&(VERSION - 1).to_le_bytes());
    assert_eq!(Document::decode(&bytes[..len]), Err(Error::Version));
}

#[test]
fn box_values_follow_css_forms_and_unsupported_forms_are_diagnosed() {
    let opacity = |value: &str| {
        compile(
            r#"<main id="r"><ui-chart id="c"></ui-chart></main>"#,
            &format!("ui-chart {{ fill-opacity: {value}; }}"),
        )
        .map(|v| v.decoration(v.nodes[1].style).fill_opacity)
    };
    for (value, expected) in [
        ("0.25", 64),
        (".5", 128),
        ("25%", 64),
        ("1", 255),
        ("1.0", 255),
        ("100%", 255),
        ("0", 0),
    ] {
        assert_eq!(opacity(value).unwrap(), expected, "{value}");
    }
    for value in ["1.5", "0.2500", "101%", "-0.1", "50 %", ".", "%"] {
        assert!(opacity(value).is_err(), "{value}");
    }
    let sides = |value: &str| {
        compile(
            r#"<main id="r"><div id="d"></div></main>"#,
            &format!("div {{ border-width: {value}; }}"),
        )
        .map(|v| {
            let d = v.decoration(v.nodes[1].style);
            (d.border_width, d.border_sides)
        })
    };
    for (value, expected) in [
        ("1px", (1, 15)),
        ("2px 0", (2, 5)),
        ("0 3px", (3, 10)),
        ("0 0 2px", (2, 4)),
        ("0 0 2px 0", (2, 4)),
        ("0 0 0 4px", (4, 8)),
        ("0", (0, 0)),
    ] {
        assert_eq!(sides(value).unwrap(), expected, "{value}");
    }
    for value in ["1px 2px", "17px", "1px 1px 1px 1px 1px", "thin"] {
        assert!(sides(value).is_err(), "{value}");
    }
    for css in [
        "div { border-radius: 256px; }",
        "div { border-radius: 4px 8px; }",
        "div { border-radius: 50%; }",
        "div { border-color: red; }",
        "div { text-align: justify; }",
        "div { vertical-align: baseline; }",
        "div { align-items: baseline; }",
        "div { outline: 1px solid; }",
        "div { outline: 0; }",
    ] {
        assert!(
            compile(r#"<main id="r"><div id="d"></div></main>"#, css).is_err(),
            "{css}"
        );
    }
    // Centring column children would need intrinsic widths; the diagnostic
    // names the declaration that requested it.
    let error = compile(
        r#"<main id="r"><div id="d"></div></main>"#,
        "div {\n  align-items: center;\n}",
    )
    .unwrap_err();
    assert_eq!((error.line, error.column), (2, 3), "{error}");
    assert!(error.message.contains("flex row"), "{error}");
    assert!(compile(
        r#"<main id="r"><div id="d"></div></main>"#,
        "div { display: flex; flex-direction: row; align-items: center; }"
    )
    .is_ok());
    assert!(compile(
        r#"<main id="r"><div id="d"></div></main>"#,
        "div { align-items: stretch; }"
    )
    .is_ok());
    let mut markup = String::from(r#"<main id="r">"#);
    let mut css = String::new();
    for i in 0..=MAX_DECORATIONS {
        write!(markup, r#"<div id="d{i}"></div>"#).unwrap();
        writeln!(css, "#d{i} {{ border-radius: {}px; }}", i + 1).unwrap();
    }
    markup.push_str("</main>");
    let error = compile(&markup, &css).unwrap_err();
    assert!(error.message.contains("decoration table"), "{error}");
}

#[test]
fn text_align_inherits_and_explicit_or_state_values_win() {
    let mut view = compile(
        r#"<main id="r"><section id="panel"><span id="child">a</span><span id="own">b</span></section></main>"#,
        "main { text-align: right; } #own { text-align: left; } section:selected { text-align: center; }",
    )
    .unwrap();
    let aligned = |view: &Document| {
        let scene = Scene::from_document(view, 200, 100).unwrap();
        [1, 2, 3].map(|i| scene.layout().style(view, i).text_align)
    };
    assert_eq!(aligned(&view), [2, 2, 0]);
    view.nodes[1].state |= state::SELECTED;
    assert_eq!(aligned(&view), [1, 1, 0]);
}

#[test]
fn borders_belong_to_the_box_and_inset_its_content() {
    let view = compile(
        r#"<main id="r"><section id="s"><span id="t">x</span></section><div id="u"><span id="v">y</span></div></main>"#,
        "section { padding: 4px; border-width: 2px; } div { border-width: 0 0 3px 0; }",
    )
    .unwrap();
    let scene = Scene::from_document(&view, 200, 200).unwrap();
    let placed = |id| scene.layout().nodes[index(&view, id)].bounds;
    let rect = |x, y, width, height| Rect {
        x,
        y,
        width,
        height,
    };
    assert_eq!(placed("s"), rect(0, 0, 200, 28));
    assert_eq!(placed("t"), rect(6, 6, 188, 16));
    assert_eq!(placed("u"), rect(0, 28, 200, 19));
    assert_eq!(placed("v"), rect(0, 28, 200, 16));
}

#[test]
fn align_items_places_row_children_on_the_vertical_axis() {
    for (value, icon, text) in [
        ("flex-start", (0, 10), (0, 16)),
        ("center", (15, 10), (12, 16)),
        ("flex-end", (30, 10), (24, 16)),
        ("stretch", (0, 10), (0, 40)),
    ] {
        let view = compile(
            r#"<main id="r"><div id="row"><ui-icon id="i"></ui-icon><span id="s">x</span></div></main>"#,
            &format!(
                "#row {{ display: flex; flex-direction: row; height: 40px; align-items: {value}; }}
                 ui-icon {{ width: 10px; height: 10px; }}"
            ),
        )
        .unwrap();
        let scene = Scene::from_document(&view, 100, 100).unwrap();
        let at = |id| {
            let bounds = scene.layout().nodes[index(&view, id)].bounds;
            (bounds.y, bounds.height)
        };
        assert_eq!(at("i"), icon, "{value}");
        assert_eq!(at("s"), text, "{value}");
    }
}

#[test]
fn rounded_background_is_symmetric_antialiased_and_solid_inside() {
    let view = compile(
        r#"<main id="r"><div id="d"></div></main>"#,
        "div { height: 24px; border-radius: 8px; background-color: #ffffff; }",
    )
    .unwrap();
    let (width, height) = (40, 24);
    let image = pixels(&view, width, height, 1);
    let alpha = |x: u32, y: u32| image[(y * width + x) as usize] >> 24;
    assert_eq!(alpha(0, 0), 0);
    for (x, y) in [(20, 12), (20, 0), (0, 12), (8, 8)] {
        assert_eq!(alpha(x, y), 255, "({x}, {y})");
    }
    let partial = (0..8)
        .flat_map(|y| (0..8).map(move |x| (x, y)))
        .filter(|(x, y)| (1..255).contains(&alpha(*x, *y)))
        .count();
    assert!(partial >= 8, "{partial} antialiased corner pixels");
    for y in 0..height {
        for x in 0..width {
            assert_eq!(alpha(x, y), alpha(width - 1 - x, y));
            assert_eq!(alpha(x, y), alpha(x, height - 1 - y));
        }
    }
    // A rounded box loses (4 - pi) r^2 of its area.
    let covered = image.iter().map(|p| f64::from(p >> 24)).sum::<f64>() / 255.0;
    let expected = f64::from(width * height) - (4.0 - core::f64::consts::PI) * 64.0;
    assert!((covered - expected).abs() < 1.0, "{covered} vs {expected}");
}

#[test]
fn border_ring_paints_only_the_edge_and_band_height_never_changes_pixels() {
    let view = compile(
        r#"<main id="r"><div id="d"></div></main>"#,
        "div { height: 30px; border-radius: 10px; border-width: 2px; border-color: #ff8000; }",
    )
    .unwrap();
    let (width, height) = (50, 30);
    let image = pixels(&view, width, height, height);
    let alpha = |x: u32, y: u32| image[(y * width + x) as usize] >> 24;
    assert_eq!((alpha(25, 0), alpha(25, 1), alpha(25, 2)), (255, 255, 0));
    assert_eq!((alpha(0, 15), alpha(1, 15), alpha(2, 15)), (255, 255, 0));
    assert_eq!((alpha(25, 15), alpha(0, 0)), (0, 0));
    assert_eq!(image[25], 0xffff_8000);
    for band in [1, 4, 7] {
        assert_eq!(pixels(&view, width, height, band), image, "band {band}");
    }
    // A translucent border is blended once per pixel: no span overlaps.
    let view = compile(
        r#"<main id="r"><div id="d"></div></main>"#,
        "div { height: 30px; border-radius: 10px; border-width: 3px 0; border-color: #ff800080; }",
    )
    .unwrap();
    let image = pixels(&view, width, height, 3);
    assert!(image.iter().all(|p| p >> 24 <= 0x80));
    assert_eq!(image[25] >> 24, 0x80);
}

#[test]
fn default_and_zero_box_values_keep_the_original_paint_calls() {
    let markup = r#"<main id="r"><button id="b" action="go">Go</button><ui-value-bar id="v"></ui-value-bar><ui-icon id="i"></ui-icon><span id="s">Text</span></main>"#;
    let base = "main { padding: 3px; background-color: #101820; } button { height: 30px; background-color: #304050; }";
    let zero = format!(
        "{base} button {{ border-radius: 0px; border-width: 0; vertical-align: top; }}
         ui-value-bar {{ border-radius: 0px; fill-opacity: 0; }} span {{ border-width: 0 0 0 0; }}"
    );
    let calls = |css: &str| {
        let mut view = compile(markup, css).unwrap();
        assert_eq!(view.decoration_len, 0);
        view.set_focus(symbol("b"));
        let mut calls = Calls::default();
        Scene::from_document(&view, 120, 90)
            .unwrap()
            .paint(&mut calls);
        calls.0
    };
    let plain = calls(base);
    assert_eq!(plain, calls(&zero));
    // One rectangle per background, a four-edge focus outline, the built-in
    // value-bar track and a square icon.
    assert_eq!(plain.iter().filter(|c| c.contains(" 101820 ")).count(), 1);
    assert_eq!(plain.iter().filter(|c| c.contains(" 304050 ")).count(), 1);
    assert_eq!(
        plain.iter().filter(|c| c.contains(" e9edf6 255")).count(),
        5
    );
    assert_eq!(plain.iter().filter(|c| c.contains(" 4b5266 ")).count(), 1);
}

#[test]
fn outline_none_omits_only_the_focus_ring() {
    let markup = r#"<main id="r"><button id="b" action="go">Go</button></main>"#;
    let base =
        "button { height: 30px; border-radius: 6px; color: #e9edf6; background-color: #304050; }";
    let painted = |css: &str| {
        let mut view = compile(markup, css).unwrap();
        view.set_focus(symbol("b"));
        let mut calls = Calls::default();
        Scene::from_document(&view, 120, 90)
            .unwrap()
            .paint(&mut calls);
        (view, calls.0)
    };
    // Ring pixels are fills in the text color; the label is a text call.
    let is_ring = |c: &&String| c.starts_with("fill") && c.contains(" e9edf6 ");
    let ring = |calls: &[String]| calls.iter().filter(is_ring).count();
    let (_, ringed) = painted(base);
    assert!(ring(&ringed) > 0);
    let (view, plain) = painted(&format!("{base} button:focus {{ outline: none; }}"));
    assert_eq!(ring(&plain), 0);
    // Background and label are unchanged; only the ring is gone.
    assert_eq!(
        ringed.iter().filter(|c| !is_ring(c)).collect::<Vec<_>>(),
        plain.iter().collect::<Vec<_>>()
    );
    // The flag lives in the decoration, so it survives the wire.
    let mut bytes = [0; MAX_WIRE];
    let len = view.encode(&mut bytes).unwrap();
    assert_eq!(Document::decode(&bytes[..len]).unwrap(), view);
    // A later `outline: auto` restores the ring.
    let (_, restored) = painted(&format!(
        "{base} button:focus {{ outline: none; }} #b:focus {{ outline: auto; }}"
    ));
    assert_eq!(restored, ringed);
}

#[test]
fn scrollbar_is_drawn_like_a_value_bar() {
    let mut markup = String::from(r#"<main id="r"><div id="list">"#);
    for i in 0..20 {
        write!(markup, r#"<span id="s{i}">Row</span>"#).unwrap();
    }
    markup.push_str("</div></main>");
    let view = compile(
        &markup,
        "#list { height: 100px; overflow: auto; } span { height: 20px; }",
    )
    .unwrap();
    let (width, height) = (120, 100);
    let scene = Scene::from_document(&view, width, height).unwrap();
    let bar = scene.scrollbar(index(&view, "list")).unwrap();
    let mut calls = Calls::default();
    scene.paint(&mut calls);
    // The dark theme's translucent `track` under its opaque `accent` thumb.
    assert!(calls.0.iter().any(|c| c.contains(" ffffff 31 ")));
    assert!(calls.0.iter().any(|c| c.contains(" a78bfa 255 ")));
    let image = pixels(&view, width, height, height);
    let at = |x: i32, y: i32| image[(y as u32 * width + x as u32) as usize];
    let thumb = bar.thumb;
    // Fully rounded: the thumb's corner is only partly covered, its middle is
    // solid accent.
    assert_eq!(at(thumb.x, thumb.y + thumb.height as i32 / 2), 0xffa7_8bfa);
    assert!(at(thumb.x, thumb.y) >> 24 < 0xff);
}

#[test]
fn rounded_focus_follows_the_corners() {
    let image = |radius: u32| {
        let mut view = compile(
            r#"<main id="r"><button id="b" action="go"></button></main>"#,
            &format!("button {{ height: 20px; border-radius: {radius}px; }}"),
        )
        .unwrap();
        view.set_focus(symbol("b"));
        pixels(&view, 40, 20, 20)
    };
    let (square, rounded) = (image(0), image(6));
    assert_eq!(square[0] >> 24, 255);
    assert_eq!(rounded[0] >> 24, 0);
    assert_eq!((square[20] >> 24, rounded[20] >> 24), (255, 255));
}

#[test]
fn text_align_and_vertical_align_position_labels_with_the_shared_advance() {
    // Proportional text: narrow and wide letters differ, sizes are not scaled.
    assert!(ui_font_text_width("iiii", 16) < ui_font_text_width("MMMM", 16));
    assert!(ui_font_text_width("abcd", 20) > ui_font_text_width("abcd", 16));
    assert_eq!(ui_font_text_width("", 16), 0);
    let abcd = ui_font_text_width("abcd", 16) as i32;
    let long = "abcdefghijklmnopqrstuvwxyz0123";
    assert!(ui_font_text_width(long, 16) > 192);
    for (text, css, x, y) in [
        ("abcd", "", 4, 4),
        ("abcd", "text-align: center;", 4 + (192 - abcd) / 2, 4),
        (
            "abcd",
            "text-align: right; vertical-align: bottom;",
            4 + 192 - abcd,
            4 + 32 - 16,
        ),
        ("abcd", "vertical-align: middle;", 4, 4 + 8),
        (long, "text-align: right;", 4, 4),
    ] {
        let view = compile(
            &format!(r#"<main id="r"><span id="s">{text}</span></main>"#),
            &format!("span {{ width: 200px; height: 40px; padding: 4px; {css} }}"),
        )
        .unwrap();
        let mut calls = Calls::default();
        Scene::from_document(&view, 300, 100)
            .unwrap()
            .paint(&mut calls);
        let expected = format!("text Rect {{ x: {x}, y: {y},");
        assert!(calls.0[0].starts_with(&expected), "{css}: {}", calls.0[0]);
    }
}

#[test]
fn chart_area_follows_available_segments_down_to_the_baseline() {
    struct Series<'a>(&'a [Point]);
    impl Bindings for Series<'_> {
        fn get(&self, _: u32) -> Option<Value<'_>> {
            Some(Value::Series(self.0))
        }
    }
    let point = |time_ms, value| Point { time_ms, value };
    let points = [
        point(0, 0),
        point(1000, 10000),
        point(2000, 10001),
        point(3000, 5000),
        point(4000, 5000),
        point(5000, 10001),
        point(6000, 7000),
    ];
    let area = |opacity: &str| {
        let template = compile(
            r#"<main id="root"><ui-chart id="chart" bind:series="history"></ui-chart></main>"#,
            &format!("ui-chart {{ height: 120px; color: #123456; {opacity} }}"),
        )
        .unwrap();
        let mut view = Document::EMPTY;
        template.bind_into(&Series(&points), &mut view).unwrap();
        let mut calls = Calls::default();
        Scene::from_document(&view, 700, 120)
            .unwrap()
            .paint(&mut calls);
        calls
            .0
            .into_iter()
            .filter(|c| c.starts_with("fill") && c.contains(" 123456 64 "))
            .collect::<Vec<_>>()
    };
    assert!(area("").is_empty());
    let fills = area("fill-opacity: 25%;");
    let plot = "Rect { x: 36, y: 6, width: 664, height: 98 }";
    let mut covered = vec![[0u8; 700]; 120];
    for call in &fills {
        assert!(call.ends_with(plot), "{call}");
        let numbers: Vec<i64> = call
            .split(|c: char| !c.is_ascii_digit())
            .filter(|s| !s.is_empty())
            .map(|s| s.parse().unwrap())
            .collect();
        let (x, y, width, height) = (numbers[0], numbers[1], numbers[2], numbers[3]);
        for row in &mut covered[y as usize..(y + height) as usize] {
            for cell in &mut row[x as usize..(x + width) as usize] {
                *cell += 1;
            }
        }
    }
    // Samples sit at x = 36 + t * 663 / 6000; 0 % is row 103, 100 % row 6.
    let column = |x: usize| {
        let rows: Vec<_> = (0..120).filter(|y| covered[*y][x] != 0).collect();
        rows.first().map(|first| (*first, rows.len()))
    };
    assert!(covered.iter().flatten().all(|c| *c <= 1), "no double blend");
    assert_eq!(column(36), Some((103, 1)));
    assert_eq!(column(146), Some((6, 98)));
    assert_eq!(column(400), Some((55, 49)));
    assert_eq!(column(478), Some((55, 49)));
    // The unavailable samples break the area; the isolated last sample has
    // no segment and therefore no area.
    assert_eq!(column(147), None);
    assert_eq!(column(400 - 70), None);
    assert_eq!(column(479), None);
    assert_eq!(column(699), None);
}

#[test]
fn styled_value_bar_background_replaces_the_builtin_track() {
    struct Half;
    impl Bindings for Half {
        fn get(&self, _: u32) -> Option<Value<'_>> {
            Some(Value::Percent(Some(5000)))
        }
    }
    let fills = |css: &str| {
        let template = compile(
            r#"<main id="r"><ui-value-bar id="v" bind:value="v"></ui-value-bar></main>"#,
            css,
        )
        .unwrap();
        let mut view = Document::EMPTY;
        template.bind_into(&Half, &mut view).unwrap();
        let mut calls = Calls::default();
        Scene::from_document(&view, 100, 40)
            .unwrap()
            .paint(&mut calls);
        calls.0
    };
    let builtin = fills("ui-value-bar { color: #8899aa; }");
    assert!(builtin.iter().any(|c| c.contains(" 4b5266 ")));
    let styled = fills("ui-value-bar { color: #8899aa; background-color: #223344; }");
    assert!(!styled.iter().any(|c| c.contains(" 4b5266 ")));
    assert!(styled[0].contains("width: 100, height: 16 } 223344 255"));
    assert!(styled[1].contains("width: 50, height: 16 } 8899aa 255"));
}

#[test]
fn text_model_verification_covers_the_decoration_table() {
    struct Model;
    impl text_model::Model for Model {
        fn text(&self, _: u32, out: &mut text_model::Text) -> core::fmt::Result {
            out.write_str("label")
        }
        fn flag(&self, _: u32) -> Option<bool> {
            Some(false)
        }
    }
    let template = compile(
        r#"<main id="r"><span id="s" bind:text="t">x</span></main>"#,
        "span { border-radius: 4px; border-width: 1px; }",
    )
    .unwrap();
    let mut document = Box::new(Document::EMPTY);
    text_model::bind_into(&template, &Model, &mut document).unwrap();
    assert!(text_model::matches(&template, &document, &Model));
    document.decorations[0].border_width = 2;
    assert!(!text_model::matches(&template, &document, &Model));
}

#[test]
fn decoration_references_and_entries_are_validated() {
    let view = compile(
        r#"<main id="r"><div id="d"></div></main>"#,
        "div { border-radius: 6px; }",
    )
    .unwrap();
    assert_eq!(view.decoration_len, 1);
    view.validate().unwrap();
    let refused = |change: &dyn Fn(&mut Document)| {
        let mut bad = view.clone();
        change(&mut bad);
        bad.validate()
    };
    assert_eq!(
        refused(&|d| d.nodes[1].style.decoration = 1),
        Err(Error::Structure)
    );
    assert_eq!(
        refused(&|d| d.decorations[0] = Decoration::NONE),
        Err(Error::Structure)
    );
    // A border color without a border is not the canonical entry.
    assert_eq!(
        refused(&|d| d.decorations[0].border_color = 0xff00_0000),
        Err(Error::Structure)
    );
    assert_eq!(
        refused(&|d| d.decorations[0].align_items = 4),
        Err(Error::Structure)
    );
    assert_eq!(
        refused(&|d| d.decoration_len = MAX_DECORATIONS as u16 + 1),
        Err(Error::Limit)
    );
    assert_eq!(
        refused(&|d| d.nodes[1].style.text_align = 3),
        Err(Error::Structure)
    );
    let mut bytes = [0; MAX_WIRE];
    let len = view.encode(&mut bytes).unwrap();
    // The last record is the decoration: seven bytes, its color, its color
    // token, the outline flag and the background preset.
    bytes[len - 7] = 2;
    assert_eq!(Document::decode(&bytes[..len]), Err(Error::Structure));
    let len = view.encode(&mut bytes).unwrap();
    bytes[len - 2] = 2;
    assert_eq!(Document::decode(&bytes[..len]), Err(Error::Structure));
    let len = view.encode(&mut bytes).unwrap();
    bytes[len - 1] = BACKGROUNDS.len() as u8 + 1;
    assert_eq!(Document::decode(&bytes[..len]), Err(Error::Structure));
}

#[test]
fn background_presets_paint_their_gradient_over_the_background_in_the_box_shape() {
    let view = compile(
        r#"<main id="r"><div id="d"></div><div id="flat"></div></main>"#,
        "div { height: 11px; border-radius: 4px; background-color: #ff0000;
               background-image: theme(dusk); }
         #flat { background-image: none; }",
    )
    .unwrap();
    let dusk = BACKGROUNDS[2];
    assert_eq!(dusk.name, "dusk");
    let decoration = |id| view.decoration(view.nodes[index(&view, id)].style);
    assert_eq!(
        (
            decoration("d").background_image,
            decoration("flat").background_image
        ),
        (3, 0)
    );
    let mut bytes = [0; MAX_WIRE];
    let len = view.encode(&mut bytes).unwrap();
    assert_eq!(Document::decode(&bytes[..len]).unwrap(), view);

    let width = 20;
    let image = pixels(&view, width, 22, 3);
    let pixel = |x: u32, y: u32| image[(y * width + x) as usize];
    // The opaque preset hides the background color, one row at a time.
    for y in 0..11 {
        let row = vertical_gradient(dusk.top, dusk.bottom, y, 11);
        assert_eq!(pixel(10, y), 0xff00_0000 | row, "row {y}");
    }
    assert_eq!(pixel(0, 0) >> 24, 0, "the corner stays rounded");
    assert_eq!(pixel(10, 11), 0xffff_0000, "no image: the background color");

    for value in [
        "theme(sunset)",
        "theme(text)",
        "linear-gradient(#000000, #ffffff)",
        "url(a.png)",
    ] {
        let css = format!("div {{ background-image: {value}; }}");
        assert!(
            compile(r#"<main id="r"><div id="d"></div></main>"#, &css).is_err(),
            "{value}"
        );
    }
}

#[test]
fn vertical_gradient_rows_round_each_channel_between_the_stops() {
    assert_eq!(
        vertical_gradient(0x8010_2030, 0x4050_6070, 0, 5),
        0x8010_2030
    );
    assert_eq!(
        vertical_gradient(0x8010_2030, 0x4050_6070, 4, 5),
        0x4050_6070
    );
    assert_eq!(
        vertical_gradient(0x8010_2030, 0x4050_6070, 2, 5),
        0x6030_4050
    );
    assert_eq!(
        vertical_gradient(0xff00_0000, 0xff00_00ff, 1, 3),
        0xff00_0080
    );
    // One row, or a row past the end, is a stop rather than a division by zero.
    assert_eq!(
        vertical_gradient(0xff11_2233, 0xff44_5566, 0, 1),
        0xff11_2233
    );
    assert_eq!(
        vertical_gradient(0xff11_2233, 0xff44_5566, 9, 3),
        0xff44_5566
    );
}

#[test]
fn retained_shapes_compose_like_the_default_runs() {
    use osaiku_ui::paint::RoundedShape;
    const W: usize = 96;
    /// Alpha per pixel; every shape writes each pixel at most once.
    struct Alpha {
        pixels: Vec<Option<u8>>,
        retain: bool,
        shapes: usize,
    }
    impl Alpha {
        fn put(&mut self, x: i32, y: i32, alpha: u8) {
            let pixel = &mut self.pixels[y as usize * W + x as usize];
            assert!(pixel.is_none(), "({x}, {y}) painted twice");
            *pixel = Some(alpha);
        }
    }
    impl Painter for Alpha {
        fn fill_alpha(&mut self, r: Rect, _: u32, alpha: u8, clip: Rect) {
            let r = r.intersect(clip);
            for y in r.y..r.bottom() {
                for x in r.x..r.right() {
                    self.put(x, y, alpha);
                }
            }
        }
        fn text(&mut self, _: Rect, _: &str, _: u16, _: u32, _: Rect) {}
        fn line(&mut self, _: (i32, i32), _: (i32, i32), _: u32, _: Rect) {}
        fn fill_shape(&mut self, area: Rect, shape: RoundedShape, _: u32, clip: Rect) {
            if !self.retain {
                return self.fill_shape_default(area, shape, clip);
            }
            self.shapes += 1;
            // A planner composes later, at another origin.
            let moved = shape.translated(7, -3);
            let r = area.intersect(clip);
            for y in r.y..r.bottom() {
                for x in r.x..r.right() {
                    let alpha = moved.alpha(x + 7, y - 3);
                    assert_eq!(alpha, shape.alpha(x, y));
                    if alpha != 0 {
                        self.put(x, y, alpha);
                    }
                }
            }
        }
    }
    impl Alpha {
        fn fill_shape_default(&mut self, area: Rect, shape: RoundedShape, clip: Rect) {
            struct Default<'a>(&'a mut Alpha);
            impl Painter for Default<'_> {
                fn fill_alpha(&mut self, r: Rect, c: u32, a: u8, clip: Rect) {
                    self.0.fill_alpha(r, c, a, clip)
                }
                fn text(&mut self, _: Rect, _: &str, _: u16, _: u32, _: Rect) {}
                fn line(&mut self, _: (i32, i32), _: (i32, i32), _: u32, _: Rect) {}
            }
            Default(self).fill_shape(area, shape, 0, clip);
        }
    }
    let clip_all = Rect {
        x: 0,
        y: 0,
        width: W as u32,
        height: W as u32,
    };
    let clip_part = Rect {
        x: 9,
        y: 5,
        width: 40,
        height: 33,
    };
    for (rect, radius, widths) in [
        (
            Rect {
                x: 3,
                y: 4,
                width: 80,
                height: 30,
            },
            12,
            None,
        ),
        (
            Rect {
                x: 3,
                y: 4,
                width: 80,
                height: 30,
            },
            255,
            None,
        ),
        (
            Rect {
                x: 10,
                y: 6,
                width: 42,
                height: 42,
            },
            12,
            Some([1; 4]),
        ),
        (
            Rect {
                x: 2,
                y: 2,
                width: 27,
                height: 20,
            },
            255,
            Some([6; 4]),
        ),
        (
            Rect {
                x: 5,
                y: 5,
                width: 60,
                height: 24,
            },
            8,
            Some([0, 0, 2, 0]),
        ),
        (
            Rect {
                x: 5,
                y: 5,
                width: 12,
                height: 12,
            },
            6,
            Some([8; 4]),
        ),
    ] {
        for clip in [clip_all, clip_part] {
            let mut painted = [false, true].map(|retain| {
                let mut p = Alpha {
                    pixels: vec![None; W * W],
                    retain,
                    shapes: 0,
                };
                match widths {
                    None => p.fill_rounded(rect, radius, 0xffffff, 200, clip),
                    Some(widths) => p.stroke_rounded(rect, radius, widths, 0xffffff, 200, clip),
                }
                p
            });
            // A corner cell for a fill; a ring's corner also splits at the
            // inner shape's edges.
            let cells = if widths.is_none() { 4 } else { 16 };
            assert!(
                painted[1].shapes <= cells,
                "{rect:?}: {}",
                painted[1].shapes
            );
            let [default, retained] = &mut painted;
            assert!(
                default.pixels == retained.pixels,
                "{rect:?} {radius} {widths:?} {clip:?}"
            );
        }
    }
}
