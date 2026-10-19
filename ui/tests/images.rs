#![cfg(feature = "compiler")]
//! `ui-icon src`: host images, pre-rasterized at fixed sizes.
use osaiku_ui::{
    compiler::compile_unstyled as compile,
    image::{Image, ImageSet},
    layout::Rect,
    paint::{Painter, Scene},
};

static MARK_2: [u32; 4] = [0xffff_0000, 0x80ff_0000, 0x0000_0000, 0xff00_ff00];
static MARK_4: [u32; 16] = [0xff00_00ff; 16];
static IMAGES: [ImageSet; 1] = [ImageSet {
    name: "mark",
    images: &[
        Image {
            width: 2,
            height: 2,
            pixels: &MARK_2,
        },
        Image {
            width: 4,
            height: 4,
            pixels: &MARK_4,
        },
    ],
}];

#[derive(Default)]
struct Calls {
    fills: Vec<(Rect, u32, u8)>,
    images: Vec<(Rect, u16)>,
}
impl Painter for Calls {
    fn fill_alpha(&mut self, rect: Rect, color: u32, alpha: u8, _: Rect) {
        self.fills.push((rect, color, alpha));
    }
    fn text(&mut self, _: Rect, _: &str, _: u16, _: u32, _: Rect) {}
    fn line(&mut self, _: (i32, i32), _: (i32, i32), _: u32, _: Rect) {}
}

/// Records images instead of expanding them, as a recipe planner does.
#[derive(Default)]
struct Retained(Calls);
impl Painter for Retained {
    fn fill_alpha(&mut self, rect: Rect, color: u32, alpha: u8, clip: Rect) {
        self.0.fill_alpha(rect, color, alpha, clip);
    }
    fn text(&mut self, _: Rect, _: &str, _: u16, _: u32, _: Rect) {}
    fn line(&mut self, _: (i32, i32), _: (i32, i32), _: u32, _: Rect) {}
    fn image(&mut self, at: Rect, image: &'static Image, _: Rect) {
        self.0.images.push((at, image.width));
    }
}

fn icon(src: &str, side: u32) -> osaiku_ui::Document {
    compile(
        &format!(r#"<main id="r"><ui-icon id="i" src="{src}"></ui-icon></main>"#),
        &format!("ui-icon {{ width: {side}px; height: {side}px; }}"),
    )
    .unwrap()
}

#[test]
fn icon_src_names_a_host_image_and_takes_no_text() {
    let view = icon("mark", 8);
    assert_eq!(view.text_of(1), "mark");
    for markup in [
        r#"<main id="r"><ui-icon id="i" src="mark">x</ui-icon></main>"#,
        r#"<main id="r"><ui-icon id="i" src="a b"></ui-icon></main>"#,
        r#"<main id="r"><ui-icon id="i" src=""></ui-icon></main>"#,
        r#"<main id="r"><span id="s" src="mark"></span></main>"#,
    ] {
        assert!(compile(markup, "").is_err(), "{markup}");
    }
}

#[test]
fn icons_take_the_largest_size_that_fits_and_otherwise_keep_the_square() {
    let paint = |view: &osaiku_ui::Document, images: &'static [ImageSet]| {
        let mut calls = Retained::default();
        Scene::from_document(view, 40, 40)
            .unwrap()
            .with_images(images)
            .paint(&mut calls);
        calls.0
    };
    // 5 px fits the 4 px size, centred; 3 px only the 2 px one.
    assert_eq!(paint(&icon("mark", 5), &IMAGES).images[0].1, 4);
    assert_eq!(paint(&icon("mark", 3), &IMAGES).images[0].1, 2);
    // Too small for any size, an unknown name, or no host images: the square.
    for calls in [
        paint(&icon("mark", 1), &IMAGES),
        paint(&icon("other", 8), &IMAGES),
        paint(&icon("mark", 8), &[]),
    ] {
        assert!(calls.images.is_empty());
        assert_eq!(calls.fills.len(), 1);
    }
}

#[test]
fn default_image_runs_skip_transparent_pixels_and_respect_the_clip() {
    let mut calls = Calls::default();
    let at = Rect {
        x: 10,
        y: 20,
        width: 2,
        height: 2,
    };
    calls.image(at, &IMAGES[0].images[0], at);
    assert_eq!(
        calls.fills,
        [
            (
                Rect {
                    x: 10,
                    y: 20,
                    width: 1,
                    height: 1
                },
                0xff_0000,
                255
            ),
            (
                Rect {
                    x: 11,
                    y: 20,
                    width: 1,
                    height: 1
                },
                0xff_0000,
                128
            ),
            (
                Rect {
                    x: 11,
                    y: 21,
                    width: 1,
                    height: 1
                },
                0x00_ff00,
                255
            ),
        ]
    );
    let mut clipped = Calls::default();
    let at = Rect {
        width: 4,
        height: 4,
        ..at
    };
    clipped.image(at, &IMAGES[0].images[1], Rect { width: 1, ..at });
    // Four rows of one visible column, one run each.
    assert_eq!(clipped.fills.len(), 4);
    assert!(clipped
        .fills
        .iter()
        .all(|(r, ..)| r.width == 1 && r.x == 10));
}

#[test]
fn a_host_image_backs_a_container_centred_under_its_children() {
    let view = compile(
        r#"<main id="r"><div id="d"><span id="s">x</span></div><div id="none"></div></main>"#,
        "div { width: 6px; height: 4px; background-image: image(0); }
         #none { background-image: image(1); }",
    )
    .unwrap();
    let mut retained = Retained::default();
    Scene::from_document(&view, 6, 8)
        .unwrap()
        .with_images(&IMAGES)
        .paint(&mut retained);
    // The largest size that fits the 6x4 box, centred; a missing set draws
    // nothing; the child's text comes after the image.
    assert_eq!(
        retained.0.images,
        [(
            Rect {
                x: 1,
                y: 0,
                width: 4,
                height: 4
            },
            4
        )]
    );
    for value in ["image(8)", "image(-1)", "image()", "image(mark)"] {
        assert!(
            compile(
                r#"<main id="r"><div id="d"></div></main>"#,
                &format!("div {{ background-image: {value}; }}"),
            )
            .is_err(),
            "{value}"
        );
    }
}

static INK: [u32; 4] = [0xffe0_e0e0; 4];
static PAPER_INK: [u32; 9] = [0xff40_3020; 9];
static THEMED: [ImageSet; 2] = [
    ImageSet {
        name: "glyph",
        images: &[Image {
            width: 2,
            height: 2,
            pixels: &INK,
        }],
    },
    ImageSet {
        name: "glyph.light",
        images: &[Image {
            width: 3,
            height: 3,
            pixels: &PAPER_INK,
        }],
    },
];

#[test]
fn the_light_palette_prefers_a_light_variant_of_the_same_name() {
    let paint = |src: &str, theme: osaiku_ui::theme::Theme| {
        let mut calls = Retained::default();
        Scene::from_document(&icon(src, 8), 40, 40)
            .unwrap()
            .with_theme(theme)
            .with_images(&THEMED)
            .paint(&mut calls);
        calls.0.images
    };
    use osaiku_ui::theme::Theme;
    assert_eq!(paint("glyph", Theme::Dark)[0].1, 2);
    assert_eq!(paint("glyph", Theme::Contrast)[0].1, 2);
    assert_eq!(paint("glyph", Theme::Light)[0].1, 3);
    // Without a variant the light palette keeps the plain image; a variant is
    // never found by a prefix of its name.
    assert_eq!(paint("glyph.light", Theme::Dark)[0].1, 3);
    assert!(paint("glyp", Theme::Light).is_empty());
}
