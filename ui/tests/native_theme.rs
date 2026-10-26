#![cfg(feature = "compiler")]
use osaiku_ui::{compiler::compile, layout::Layout, paint::Scene, theme::Theme, *};

fn image(doc: &Document, theme: Theme) -> Vec<u8> {
    let mut layout = Layout::EMPTY;
    let scene = Scene::prepare(doc, 100, 80, &mut layout)
        .unwrap()
        .with_theme(theme);
    let mut pixels = vec![0; 100 * 80 * 4];
    raster::rows(&scene, 0, &mut pixels).unwrap();
    pixels
}

#[test]
fn native_defaults_match_manager_and_local_overrides_are_property_wise() {
    let doc = compile(
        r#"<main id="r"><button id="b" action="go">Go</button></main>"#,
        "",
    )
    .unwrap();
    let button = doc.nodes[1].style;
    assert_eq!(
        (button.height, button.font_size, button.text_align),
        (32, 14, 1)
    );
    let decoration = doc.decoration(button);
    assert_eq!(
        (
            decoration.radius,
            decoration.border_width,
            decoration.vertical_align
        ),
        (8, 1, 1)
    );
    let custom = compile(
        r#"<main id="r"><button id="b" class="tab" action="go">Go</button></main>"#,
        "button { height:20px; border-radius:3px; background-color:#123456; }",
    )
    .unwrap();
    for state in [0, state::FOCUSED, state::SELECTED, state::DISABLED] {
        let mut view = custom.clone();
        view.nodes[1].state = state;
        let layout = Layout::compute(&view, 100, 80, &[0; MAX_NODES]).unwrap();
        let style = layout.style(&view, 1);
        assert_eq!(
            (
                style.height,
                view.decoration(style).radius,
                style.background
            ),
            (20, 3, 0xff123456)
        );
        assert_eq!(style.background_token, 0);
    }
}

#[test]
fn switching_palette_changes_pixels_without_touching_data_layout_or_input() {
    let mut doc = compile(r#"<main id="r"><button id="b" action="go">Keep me</button><ui-text-input id="input" action="edit">Unsaved text</ui-text-input></main>"#, "").unwrap();
    doc.set_focus(symbol("input"));
    doc.nodes[1].state |= state::SELECTED;
    let original = doc.clone();
    let dark = image(&doc, Theme::Dark);
    let contrast = image(&doc, Theme::Contrast);
    assert_ne!(dark, contrast);
    assert_eq!(image(&doc, Theme::Dark), dark);
    assert_eq!(doc, original);
    let mut layout = Layout::EMPTY;
    let a = Scene::prepare(&doc, 100, 80, &mut layout).unwrap();
    let before: Vec<_> = a
        .semantics()
        .map(|s| (s.id, s.bounds, s.text.to_owned(), s.state))
        .collect();
    let after: Vec<_> = a
        .with_theme(Theme::Contrast)
        .semantics()
        .map(|s| (s.id, s.bounds, s.text.to_owned(), s.state))
        .collect();
    assert_eq!(before, after);
}

#[test]
fn literal_colors_stay_custom_and_theme_aliases_survive_wire_roundtrip() {
    let doc = compile(r#"<main id="r"><button id="b" action="go">Go</button><section id="p"><span id="label">Label</span></section></main>"#,
        ":root { --highlight: theme(accent); } button { color:#010203; background-color:#456789; border-color:#abcdef; } #p { color:var(--highlight); }").unwrap();
    let mut wire = vec![0; MAX_WIRE];
    let len = doc.encode(&mut wire).unwrap();
    let decoded = Document::decode(&wire[..len]).unwrap();
    assert_eq!(doc, decoded);
    let layout = Layout::compute(&decoded, 100, 80, &[0; MAX_NODES]).unwrap();
    let b = layout.style(&decoded, 1);
    assert_eq!(
        (
            b.color_token,
            b.background_token,
            decoded.decoration(b).border_token
        ),
        (0, 0, 0)
    );
    let inherited = layout.style(&decoded, 3);
    assert_ne!(inherited.color_token, 0);
    assert_ne!(
        Theme::Dark.color(inherited.color_token, inherited.color),
        Theme::Contrast.color(inherited.color_token, inherited.color)
    );
    let dark = image(&decoded, Theme::Dark);
    let contrast = image(&decoded, Theme::Contrast);
    // Interior of the explicitly colored button, away from its label/border.
    let at = (10 * 100 + 10) * 4;
    assert_eq!(&dark[at..at + 4], &[0x89, 0x67, 0x45, 0xff]);
    assert_eq!(&dark[at..at + 4], &contrast[at..at + 4]);
}

#[test]
fn unknown_tokens_and_noncanonical_wire_references_are_refused() {
    assert!(compile("<main id=\"r\"></main>", "main { color:theme(unknown); }").is_err());
    let mut doc = compile("<main id=\"r\"><button id=\"b\">Go</button></main>", "").unwrap();
    let mut wire = vec![0; MAX_WIRE];
    doc.nodes[0].style.color_token = theme::TOKEN_COUNT + 1;
    assert_eq!(doc.encode(&mut wire), Err(Error::Structure));
    doc.nodes[0].style.color_token = theme::TEXT;
    doc.decorations[0].border_token = theme::TOKEN_COUNT + 1;
    assert_eq!(doc.encode(&mut wire), Err(Error::Structure));
}

#[test]
fn a_chosen_accent_repaints_accent_tokens_and_leaves_the_rest() {
    use osaiku_ui::appearance::ACCENTS;
    let doc = compile(
        r#"<main id="r"><button id="a" class="fill" action="go">Go</button><span id="s" class="pick">Picked</span><span id="t">Text</span></main>"#,
        ".fill { background-color: theme(accent-fill); border-color: theme(accent); } .pick { background-color: theme(selected); }",
    )
    .unwrap();
    let paint = |theme: Theme, accent| {
        let mut layout = Layout::EMPTY;
        let scene = Scene::prepare(&doc, 100, 80, &mut layout)
            .unwrap()
            .with_theme(theme)
            .with_accent(accent);
        let mut pixels = vec![0; 100 * 80 * 4];
        raster::rows(&scene, 0, &mut pixels).unwrap();
        pixels
    };
    for (theme, own) in [(Theme::Dark, 0), (Theme::Light, 3)] {
        // The palette's own accent, chosen or not, paints the same.
        assert_eq!(paint(theme, None), paint(theme, Some(own)));
        assert_ne!(paint(theme, None), paint(theme, Some((own + 1) % 4)));
        // Out of range keeps the palette's own.
        assert_eq!(paint(theme, None), paint(theme, Some(ACCENTS.len())));
        let base = usize::from(theme == Theme::Light);
        for (i, a) in ACCENTS.iter().enumerate() {
            assert_eq!(
                theme.accented(theme::ACCENT_FILL, 0, Some(i)),
                0xff00_0000 | a.fill
            );
            assert_eq!(
                theme.accented(theme::SELECTED, 0, Some(i)),
                0xff00_0000 | a.selected[base]
            );
            assert_eq!(
                theme.accented(theme::ACCENT, 0, Some(i)),
                0xff00_0000 | a.token[base]
            );
            assert_eq!(
                theme.accented(theme::TEXT, 0, Some(i)),
                theme.color(theme::TEXT, 0)
            );
        }
    }
    // High contrast keeps its own colours.
    assert_eq!(
        paint(Theme::Contrast, None),
        paint(Theme::Contrast, Some(2))
    );
}

#[test]
fn a_transported_theme_id_carries_the_accent() {
    for theme in [Theme::Dark, Theme::Contrast, Theme::Light] {
        assert_eq!(Theme::decode(theme as u32), Some((theme, None)));
        assert_eq!(theme.id_with_accent(None), theme as u32);
        for accent in 0..4 {
            let id = theme.id_with_accent(Some(accent));
            assert_eq!(Theme::decode(id), Some((theme, Some(accent))));
        }
        assert_eq!(theme.id_with_accent(Some(4)), theme as u32);
    }
    assert_eq!(Theme::decode(3), None);
    assert_eq!(Theme::decode(5 << 8), None, "no fifth accent");
    assert_eq!(Theme::decode(1 << 16), None);
}
