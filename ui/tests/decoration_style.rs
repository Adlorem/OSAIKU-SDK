use osaiku_ui::decoration::{geometry::Bounds, Falloff, PreparedStyle, WindowStyle};

static SQUARE: PreparedStyle = PreparedStyle::new(WindowStyle {
    corner_radius: 0,
    shadow: Falloff::NONE,
    glow: Falloff::NONE,
    ..WindowStyle::NATIVE
});
static ROUNDED_FLAT: PreparedStyle = PreparedStyle::new(WindowStyle {
    shadow: Falloff::NONE,
    glow: Falloff::NONE,
    ..WindowStyle::NATIVE
});
static SQUARE_SHADOW: PreparedStyle = PreparedStyle::new(WindowStyle {
    corner_radius: 0,
    ..WindowStyle::NATIVE
});
static SMALL: PreparedStyle = PreparedStyle::new(WindowStyle {
    corner_radius: 4,
    shadow: Falloff {
        extent: 7,
        color: 0x301008,
        opacity: 180,
    },
    glow: Falloff::NONE,
    border_color: 0x987654,
    focus_color: 0xfedcba,
});
static MAXIMUM: PreparedStyle = PreparedStyle::new(WindowStyle {
    corner_radius: 32,
    shadow: Falloff {
        extent: 32,
        color: 0,
        opacity: 255,
    },
    glow: Falloff {
        extent: 32,
        color: 0x55cc88,
        opacity: 255,
    },
    ..WindowStyle::NATIVE
});
static GLOW_ONLY: PreparedStyle = PreparedStyle::new(WindowStyle {
    corner_radius: 1,
    shadow: Falloff {
        extent: 32,
        opacity: 0,
        color: 0,
    },
    glow: Falloff {
        extent: 5,
        color: 0x2255ee,
        opacity: 120,
    },
    ..WindowStyle::NATIVE
});

fn background(screen: Bounds) -> Vec<u32> {
    (0..screen.width * screen.height)
        .map(|i| (i * 7919) & 0xffffff)
        .collect()
}
fn capture(image: &[u32], screen: Bounds, rect: Bounds) -> Vec<u32> {
    (rect.y..rect.y + rect.height)
        .flat_map(|y| {
            image[(y * screen.width + rect.x) as usize
                ..(y * screen.width + rect.x + rect.width) as usize]
                .iter()
                .copied()
        })
        .collect()
}
fn surface(style: &PreparedStyle, window: Bounds) -> Vec<u32> {
    let mut pixels = vec![0xff204060; (window.width * window.height) as usize];
    style
        .frame_surface(&mut pixels, window.width, window.height, &mut || {})
        .unwrap();
    pixels
}
fn draw(
    style: &PreparedStyle,
    image: &mut [u32],
    screen: Bounds,
    window: Bounds,
    foreground: &[u32],
    underlay: &[u32],
    clip: Bounds,
) {
    let saved = style.style().footprint(window, screen).unwrap();
    style
        .apply_shadow_clipped(
            image,
            screen,
            window,
            underlay,
            saved,
            saved.width,
            clip,
            &mut || {},
        )
        .unwrap();
    for y in window.y.max(clip.y)..(window.y + window.height).min(clip.y + clip.height) {
        for x in window.x.max(clip.x)..(window.x + window.width).min(clip.x + clip.width) {
            image[(y * screen.width + x) as usize] =
                foreground[((y - window.y) * window.width + x - window.x) as usize];
        }
    }
    style
        .apply_corners_clipped(
            image,
            screen,
            window,
            foreground,
            underlay,
            saved,
            saved.width,
            clip,
        )
        .unwrap();
}

#[test]
fn style_geometry_refuses_unbounded_settings_and_overflows() {
    let base = WindowStyle::NATIVE;
    let screen = Bounds::new(0, 0, 128, 128);
    let window = Bounds::new(40, 40, 48, 48);
    for bad in [
        WindowStyle {
            corner_radius: 33,
            ..base
        },
        WindowStyle {
            corner_radius: u32::MAX,
            ..base
        },
        WindowStyle {
            shadow: Falloff {
                extent: 33,
                ..base.shadow
            },
            ..base
        },
        WindowStyle {
            glow: Falloff {
                extent: u32::MAX,
                ..base.glow
            },
            ..base
        },
        WindowStyle {
            shadow: Falloff {
                color: 0x1000000,
                ..base.shadow
            },
            ..base
        },
        WindowStyle {
            glow: Falloff {
                color: 0x1000000,
                ..base.glow
            },
            ..base
        },
        WindowStyle {
            border_color: u32::MAX,
            ..base
        },
        WindowStyle {
            focus_color: u32::MAX,
            ..base
        },
    ] {
        assert!(!bad.valid());
        assert!(bad.footprint(window, screen).is_none());
        assert!(bad.corner_boxes(window).is_none());
    }
    for bad in [
        Bounds::empty(),
        Bounds::new(u32::MAX, 0, 10, 10),
        Bounds::new(0, u32::MAX, 10, 10),
    ] {
        assert!(base.footprint(bad, screen).is_none());
        assert!(base.footprint(window, bad).is_none());
        assert!(base.corner_boxes(bad).is_none());
    }
    assert_eq!(GLOW_ONLY.style().extent(), 5);
    assert_eq!(SQUARE.style().footprint(window, screen), Some(window));
    assert_eq!(
        SMALL.style().footprint(window, screen),
        Some(Bounds::new(33, 33, 62, 62))
    );
    assert_eq!(
        MAXIMUM
            .style()
            .footprint(Bounds::new(0, 0, 100, 100), screen),
        Some(screen)
    );
}

#[test]
fn square_without_shadow_is_an_opaque_rectangle_and_preserves_its_surroundings() {
    let screen = Bounds::new(0, 0, 100, 100);
    let window = Bounds::new(20, 20, 60, 60);
    let original = background(screen);
    let saved = capture(&original, screen, window);
    let foreground = surface(&SQUARE, window);
    let mut actual = original.clone();
    draw(
        &SQUARE,
        &mut actual,
        screen,
        window,
        &foreground,
        &saved,
        screen,
    );
    for y in 0..100 {
        for x in 0..100 {
            let expected = if (20..80).contains(&x) && (20..80).contains(&y) {
                foreground[((y - 20) * 60 + x - 20) as usize]
            } else {
                original[(y * 100 + x) as usize]
            };
            assert_eq!(actual[(y * 100 + x) as usize], expected);
        }
    }
    assert!(SQUARE.style().corner_boxes(window).is_none());
    assert_eq!(SQUARE.shade(0xabcdef, window, 20, 20), 0xabcdef);
}

#[test]
fn styles_keep_full_clipped_and_retained_moves_identical_at_screen_edges() {
    let screen = Bounds::new(0, 0, 180, 160);
    let original = background(screen);
    for style in [
        &SQUARE,
        &SMALL,
        &MAXIMUM,
        &GLOW_ONLY,
        &ROUNDED_FLAT,
        &SQUARE_SHADOW,
    ] {
        for (width, height) in [(100, 80), (11, 13), (1, 1)] {
            let foreground = surface(style, Bounds::new(0, 0, width, height));
            let mut moved = original.clone();
            let mut old = None;
            for (x, y) in [(0, 0), (39, 37), (180 - width, 160 - height), (0, 0)] {
                let window = Bounds::new(x, y, width, height);
                let saved = style.style().footprint(window, screen).unwrap();
                let underlay = capture(&original, screen, saved);
                if let Some(previous) = old {
                    let previous: Bounds = previous;
                    for row in previous.y..previous.y + previous.height {
                        let start = (row * screen.width + previous.x) as usize;
                        let end = start + previous.width as usize;
                        moved[start..end].copy_from_slice(&original[start..end]);
                    }
                }
                let mut fresh = original.clone();
                draw(
                    style,
                    &mut fresh,
                    screen,
                    window,
                    &foreground,
                    &underlay,
                    screen,
                );
                for clip in [
                    Bounds::new(0, 0, 73, 61),
                    Bounds::new(73, 0, 107, 61),
                    Bounds::new(0, 61, 73, 99),
                    Bounds::new(73, 61, 107, 99),
                ] {
                    draw(
                        style,
                        &mut moved,
                        screen,
                        window,
                        &foreground,
                        &underlay,
                        clip,
                    );
                }
                assert_eq!(moved, fresh, "style {:?}, window {window:?}", style.style());
                old = Some(saved);
            }
        }
    }
}

#[test]
fn prepared_shadow_matches_an_independent_scalar_falloff() {
    use osaiku_ui::decoration::pixels::blend_shape_channel;
    let window = Bounds::new(40, 40, 90, 80);
    for renderer in [
        &SQUARE,
        &SMALL,
        &MAXIMUM,
        &GLOW_ONLY,
        &ROUNDED_FLAT,
        &SQUARE_SHADOW,
    ] {
        let style = renderer.style();
        let radius = style.corner_radius as f64;
        for y in 4..157 {
            for x in 4..167 {
                let nx = (x as f64 - 40.0).min(129.0 - x as f64);
                let ny = (y as f64 - 40.0).min(119.0 - y as f64);
                let distance = if nx < radius && ny < radius {
                    (((radius - nx - 0.5).max(0.0).powi(2) + (radius - ny - 0.5).max(0.0).powi(2))
                        .sqrt()
                        - radius)
                        .max(0.0)
                } else {
                    (-nx.min(ny) - 0.5).max(0.0)
                };
                // The public contract quantizes fixed-point distances to 0.5px.
                let distance = ((distance * 256.0).floor() / 128.0).round() * 0.5;
                let alpha = |layer: Falloff| -> u8 {
                    if layer.extent == 0 || distance >= layer.extent as f64 {
                        0
                    } else {
                        (layer.opacity as f64 * (1.0 - distance / layer.extent as f64).powi(2))
                            .round() as u8
                    }
                };
                let mut expected = 0;
                for channel in 0..3 {
                    let dark = blend_shape_channel(
                        (style.shadow.color >> (channel * 8)) as u8,
                        128,
                        alpha(style.shadow),
                    );
                    let value = blend_shape_channel(
                        (style.glow.color >> (channel * 8)) as u8,
                        dark,
                        alpha(style.glow),
                    );
                    expected |= u32::from(value) << (channel * 8);
                }
                assert_eq!(
                    renderer.shade(0x808080, window, x, y),
                    expected,
                    "at {x},{y} style {style:?}"
                );
            }
        }
    }
}

#[test]
fn malformed_custom_passes_leave_output_untouched() {
    let screen = Bounds::new(0, 0, 100, 100);
    let window = Bounds::new(20, 20, 60, 60);
    let foreground = surface(&SMALL, window);
    let saved = SMALL.style().footprint(window, screen).unwrap();
    let original = background(screen);
    let backdrop = capture(&original, screen, saved);
    for (bounds, stride, len) in [
        (window, saved.width, backdrop.len()),
        (saved, 1, backdrop.len()),
        (saved, saved.width, backdrop.len() - 1),
    ] {
        let mut image = original.clone();
        assert!(SMALL
            .apply_shadow(
                &mut image,
                screen,
                window,
                &backdrop[..len],
                bounds,
                stride,
                &mut || {}
            )
            .is_none());
        assert_eq!(image, original);
        assert!(SMALL
            .apply_corners(
                &mut image,
                screen,
                window,
                &foreground,
                &backdrop[..len],
                bounds,
                stride
            )
            .is_none());
        assert_eq!(image, original);
    }
    let mut short = vec![0x123456; 59 * 60];
    assert!(SMALL
        .frame_surface(&mut short, 60, 60, &mut || {})
        .is_none());
    assert!(short.iter().all(|p| *p == 0x123456));
    let outline = surface(&SMALL, window);
    assert_eq!(outline[30], 0xfffedcba);
    assert_eq!(outline[30 * 60], 0xff987654);
}
