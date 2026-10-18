#![cfg(feature = "compiler")]
//! Proportional UI text: one pre-rasterized Inter table per admitted size.
use osaiku_ui::{
    compiler::compile_unstyled as compile,
    font::{ui_font_text_width, ui_text_glyph, ui_text_run, ui_text_size_supported},
    layout::Layout,
    paint::Scene,
    raster,
    text_atlas::{UI_TEXT_ATLAS_CODEPOINTS, UI_TEXT_SIZES},
    Document, Error, MAX_WIRE,
};

#[test]
fn every_admitted_size_carries_every_character_inside_its_line_box() {
    assert_eq!(UI_TEXT_SIZES, [12, 14, 16, 18, 20, 22, 24, 28, 32]);
    for size in UI_TEXT_SIZES {
        assert!(ui_text_size_supported(size));
        for code in UI_TEXT_ATLAS_CODEPOINTS {
            let character = char::from_u32(code).unwrap();
            let glyph = ui_text_glyph(character, size).unwrap();
            assert!(glyph.top() + glyph.height() <= u32::from(size));
            assert!(glyph.advance() > 0);
            let pixels: Vec<u8> = (0..glyph.height())
                .flat_map(|y| (0..glyph.width()).map(move |x| glyph.alpha(x, y)))
                .collect();
            // Sixteen levels, expanded to the full 0..=255 range.
            assert!(pixels.iter().all(|alpha| alpha % 17 == 0));
            if character != ' ' {
                assert!(
                    pixels.iter().any(|alpha| *alpha != 0),
                    "{character} at {size}px"
                );
            }
            // The inked box is tight: its first and last rows hold ink.
            if glyph.height() != 0 {
                let row = |y| (0..glyph.width()).any(|x| glyph.alpha(x, y) != 0);
                assert!(row(0) && row(glyph.height() - 1), "{character} at {size}px");
            }
            assert_eq!(glyph.alpha(glyph.width(), 0), 0);
        }
    }
    for size in [0, 8, 13, 15, 17, 48] {
        assert!(!ui_text_size_supported(size));
        assert!(ui_text_glyph('a', size).is_none());
        assert_eq!(ui_text_run("abc", size).count(), 0);
    }
    // Absent characters are drawn as '?', in its own box.
    assert_eq!(ui_text_glyph('€', 16), ui_text_glyph('?', 16));
}

#[test]
fn pens_accumulate_fractional_advances_and_measurement_agrees() {
    for size in UI_TEXT_SIZES {
        let text = "Menedżer systemu · 24.37%";
        let mut pen = 0u32;
        let mut previous = None;
        for (x, character, glyph) in ui_text_run(text, size) {
            assert_eq!(x as u32, (pen + 32) >> 6, "{character} at {size}px");
            assert!(previous.is_none_or(|before| before <= x));
            previous = Some(x);
            pen += glyph.advance();
        }
        assert_eq!(ui_font_text_width(text, size), (pen + 32) >> 6);
    }
    // Wider sizes are wider; letters of different shapes differ.
    assert!(ui_font_text_width("Procesor", 18) > ui_font_text_width("Procesor", 16));
    assert!(ui_font_text_width("il", 16) < ui_font_text_width("WM", 16));
}

#[test]
fn font_sizes_without_a_table_are_refused_by_compiler_and_wire() {
    let markup = r#"<main id="r"><span id="s">x</span></main>"#;
    let error = compile(markup, "span { font-size: 15px; }").unwrap_err();
    assert!(error.message.contains("12px, 14px, 16px"), "{error}");
    for size in UI_TEXT_SIZES {
        assert!(compile(markup, &format!("span {{ font-size: {size}px; }}")).is_ok());
    }
    let view = compile(markup, "span { font-size: 18px; }").unwrap();
    let mut bytes = [0; MAX_WIRE];
    let len = view.encode(&mut bytes).unwrap();
    Document::decode(&bytes[..len]).unwrap();
    let mut bad = view.clone();
    bad.nodes[1].style.font_size = 15;
    assert_eq!(bad.validate(), Err(Error::Structure));
}

#[test]
fn rasterized_text_stays_in_its_line_box_and_band_height_never_changes_it() {
    for size in [12u16, 22, 32] {
        let view = compile(
            r#"<main id="r"><span id="s">Zażółć gęślą jaźń 0123</span></main>"#,
            &format!("main {{ padding: 2px; }} span {{ font-size: {size}px; color: #ffffff; }}"),
        )
        .unwrap();
        let (width, height) = (360u32, u32::from(size) + 8);
        let render = |band: u32| {
            let mut layout = Layout::EMPTY;
            let scene = Scene::prepare(&view, width, height, &mut layout).unwrap();
            let stride = (width * 4) as usize;
            let mut bytes = vec![0; stride * height as usize];
            for (i, rows) in bytes.chunks_mut(stride * band as usize).enumerate() {
                raster::rows(&scene, i as u32 * band, rows).unwrap();
            }
            bytes
        };
        let image = render(height);
        for band in [1, 3] {
            assert_eq!(render(band), image, "{size}px band {band}");
        }
        let inked: Vec<u32> = (0..height)
            .filter(|y| {
                let row = &image[(y * width * 4) as usize..((y + 1) * width * 4) as usize];
                row.chunks_exact(4).any(|pixel| pixel[3] != 0)
            })
            .collect();
        assert!(!inked.is_empty());
        assert!(*inked.first().unwrap() >= 2 && *inked.last().unwrap() < 2 + u32::from(size));
    }
}
