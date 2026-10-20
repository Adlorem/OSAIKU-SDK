use osaiku_ui::appearance::{
    photo, shelf_alpha, Appearance, Base, ACCENTS, BACKGROUNDS, DARK, LIGHT, SHELF_TRANSPARENCY,
};

#[test]
fn selections_outside_the_tables_do_not_exist() {
    let last = |len: usize| len - 1;
    let edge = Appearance::new(
        Base::Light,
        last(ACCENTS.len()),
        last(BACKGROUNDS.len()),
        last(SHELF_TRANSPARENCY.len()),
    )
    .unwrap();
    assert_eq!(edge.accent_colors(), &ACCENTS[ACCENTS.len() - 1]);
    assert_eq!(
        edge.background_colors(),
        &BACKGROUNDS[BACKGROUNDS.len() - 1]
    );
    assert_eq!(edge.chrome(), &LIGHT);
    assert_eq!(Appearance::new(Base::Dark, ACCENTS.len(), 0, 0), None);
    assert_eq!(Appearance::new(Base::Dark, 0, BACKGROUNDS.len(), 0), None);
    assert_eq!(
        Appearance::new(Base::Dark, 0, 0, SHELF_TRANSPARENCY.len()),
        None
    );
    assert_eq!(
        Appearance::new(Base::Dark, 0, 1, 0),
        Some(Appearance::DEFAULT)
    );
    assert_eq!(Appearance::DEFAULT.chrome(), &DARK);
    assert_eq!(Appearance::default(), Appearance::DEFAULT);
}

#[test]
fn shelf_coverage_is_the_rounded_remaining_opacity() {
    let alphas: Vec<u8> = (0..SHELF_TRANSPARENCY.len())
        .map(|shelf| {
            Appearance::new(Base::Dark, 0, 0, shelf)
                .unwrap()
                .shelf_alpha()
        })
        .collect();
    assert_eq!(SHELF_TRANSPARENCY, [0, 25, 50, 75]);
    assert_eq!(alphas, [255, 191, 128, 64]);
    assert_eq!(shelf_alpha(100), 0);
    assert_eq!(shelf_alpha(250), 0);
}

#[test]
fn photograph_presets_have_their_jpeg_and_the_default_is_one() {
    for (i, background) in BACKGROUNDS.iter().enumerate() {
        let jpeg = photo(i);
        assert_eq!(jpeg.is_some(), background.photo, "{}", background.name);
        if let Some(jpeg) = jpeg {
            assert!(jpeg.starts_with(&[0xff, 0xd8, 0xff]), "{}", background.name);
        }
    }
    assert_eq!(photo(BACKGROUNDS.len()), None);
    assert!(Appearance::DEFAULT.background_colors().photo);
    assert_eq!(Appearance::DEFAULT.background_colors().name, "mountains");
}

#[test]
fn each_base_theme_names_its_palette_and_its_starting_look() {
    use osaiku_ui::theme::Theme;
    for base in [Base::Dark, Base::Light] {
        assert_eq!(Base::from_index(base.index()), Some(base));
        assert!(Theme::from_id(base.theme_id()).is_some());
        assert!(BACKGROUNDS[base.background()].photo);
        assert!(base.accent() < ACCENTS.len());
    }
    assert_eq!(Base::from_index(2), None);
    assert_eq!(Theme::from_id(Base::Light.theme_id()), Some(Theme::Light));
    assert_eq!(Base::Light.chrome(), &LIGHT);
    assert_eq!(BACKGROUNDS[Base::Light.background()].name, "origami");
    let light = Theme::Light;
    assert_eq!(
        light.color(osaiku_ui::theme::SURFACE, 0),
        0xff00_0000 | LIGHT.surface
    );
    assert_eq!(
        light.color(osaiku_ui::theme::TEXT, 0),
        0xff00_0000 | LIGHT.text
    );
}

/// WCAG contrast of two sRGB colours.
fn contrast(a: u32, b: u32) -> f64 {
    let luminance = |rgb: u32| {
        let channel = |shift: u32| {
            let c = f64::from((rgb >> shift) & 0xff) / 255.0;
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(16) + 0.7152 * channel(8) + 0.0722 * channel(0)
    };
    let (x, y) = (luminance(a) + 0.05, luminance(b) + 0.05);
    x.max(y) / x.min(y)
}

#[test]
fn the_light_palette_keeps_its_text_readable() {
    let accent = ACCENTS[Base::Light.accent()];
    assert!(contrast(0xffffff, accent.fill) >= 4.5);
    assert!(contrast(accent.token[1], LIGHT.surface) >= 4.5);
    assert!(contrast(LIGHT.text, LIGHT.surface) >= 7.0);
    assert!(contrast(LIGHT.text, LIGHT.title) >= 7.0);
    assert!(contrast(LIGHT.text, accent.selected[1]) >= 7.0);
    assert!(contrast(LIGHT.muted, LIGHT.title) >= 3.0);
    assert!(contrast(accent.token[0], DARK.surface) >= 3.0);
}
