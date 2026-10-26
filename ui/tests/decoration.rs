use osaiku_ui::decoration::{corners, frame, geometry::Bounds, glass, shadow, theme};

#[test]
fn native_decoration_preserves_the_released_opaque_pixels() {
    let screen = Bounds::new(0, 0, 128, 96);
    let window = Bounds::new(24, 24, 80, 48);
    let saved = shadow::footprint(window, screen).unwrap();
    let mut image: Vec<u32> = (0..128 * 96).map(|i| (i * 7919) & 0xffffff).collect();
    let underlay: Vec<u32> = (saved.y..saved.y + saved.height)
        .flat_map(|y| {
            image[(y * 128 + saved.x) as usize..(y * 128 + saved.x + saved.width) as usize]
                .iter()
                .copied()
        })
        .collect();
    let mut surface = vec![0xff343d50; (window.width * window.height) as usize];
    corners::frame_surface(&mut surface, window.width, window.height, &mut || {}).unwrap();
    shadow::apply(
        &mut image,
        screen,
        window,
        &underlay,
        saved,
        saved.width,
        &mut || {},
    )
    .unwrap();
    for y in 0..window.height {
        let dst = ((window.y + y) * screen.width + window.x) as usize;
        let src = (y * window.width) as usize;
        image[dst..dst + window.width as usize]
            .copy_from_slice(&surface[src..src + window.width as usize]);
    }
    corners::apply(
        &mut image,
        screen,
        window,
        &surface,
        &underlay,
        saved,
        saved.width,
    )
    .unwrap();
    let hash = image
        .iter()
        .flat_map(|p| p.to_le_bytes())
        .fold(0xcbf29ce484222325u64, |h, b| {
            (h ^ u64::from(b)).wrapping_mul(0x100000001b3)
        });
    assert_eq!(hash, 0xb96ca52fc2db52e3);
}

fn metrics() -> frame::Metrics {
    frame::Metrics {
        title_height: 28,
        control: Bounds::new(0, 0, 32, 16),
        inset: Bounds::new(0, 0, 4, 8),
    }
}

#[test]
fn public_frame_rejects_overflow_and_out_of_title_controls() {
    for (w, h) in [(0, 100), (100, 0), (u32::MAX, 100), (100, u32::MAX)] {
        assert!(frame::slots(metrics(), w, h).is_none());
        assert!(frame::resize_regions(w, h, 28).is_none());
    }
    let mut m = metrics();
    m.inset.width = u32::MAX;
    assert!(frame::slots(m, 512, 300).is_none());
    m = metrics();
    m.control.height = u32::MAX;
    assert!(frame::slots(m, 512, 300).is_none());
    m = metrics();
    m.title_height = 300;
    assert!(frame::slots(m, 512, 300).is_none());
}

#[test]
#[cfg(feature = "compiler")]
fn standalone_frame_template_binds_literal_metadata_and_the_painted_controls() {
    use osaiku_ui::{compiler::compile, symbol};
    for colors in [
        ["#343d50", "#2d3548", "#e6edf7"],
        ["#101010", "#000000", "#ffffff"],
    ] {
        let css = frame::CSS
            .replace("$SURFACE", colors[0])
            .replace("$TITLE", colors[1])
            .replace("$TEXT", colors[2])
            .replace("$MUTED", "#8b91a0");
        let doc = compile(frame::HTML, &css).unwrap();
        for centered in [false, true] {
            let scene = frame::scene(
                &doc,
                512,
                300,
                metrics(),
                "<b>Report</b>",
                "R",
                80,
                centered,
            )
            .unwrap();
            let title = scene.semantics().find(|n| n.id == symbol("title")).unwrap();
            assert_eq!(title.text, "<b>Report</b>");
            let content = scene
                .semantics()
                .find(|n| n.id == symbol("content-slot"))
                .unwrap();
            assert_eq!((content.bounds.y, content.bounds.height), (28, 272));
            for action in ["close", "drag", "resize-left", "resize-bottom"] {
                let n = scene.semantics().find(|n| n.id == symbol(action)).unwrap();
                let r = n.visible_bounds;
                assert_eq!(
                    scene.hit(r.x + r.width as i32 / 2, r.y + r.height as i32 / 2),
                    Some(symbol(action))
                );
            }
        }
    }
}

#[test]
fn sdk_effects_render_without_a_kernel_and_keep_private_foreground_and_underlay() {
    let screen = Bounds::new(0, 0, 128, 96);
    let window = Bounds::new(24, 24, 80, 48);
    let saved = shadow::footprint(window, screen).unwrap();
    let underlay = vec![0x102030; (saved.width * saved.height) as usize];
    let mut image = vec![0x102030; (screen.width * screen.height) as usize];
    let mut surface = vec![0; (window.width * window.height) as usize];
    let mut checkpoints = 0;
    let mut cooperate = || checkpoints += 1;
    corners::frame_surface(&mut surface, window.width, window.height, &mut cooperate).unwrap();
    surface[30 * window.width as usize + 40] = 0xffaabbcc;
    shadow::apply(
        &mut image,
        screen,
        window,
        &underlay,
        saved,
        saved.width,
        &mut cooperate,
    )
    .unwrap();
    assert_eq!(
        glass::compose(
            &mut image,
            screen,
            window,
            28,
            &surface,
            &underlay,
            saved,
            saved.width,
            &mut cooperate
        ),
        Some(Some(0x102030))
    );
    corners::apply(
        &mut image,
        screen,
        window,
        &surface,
        &underlay,
        saved,
        saved.width,
    )
    .unwrap();
    assert_eq!(image[(54 * screen.width + 64) as usize], 0xaabbcc);
    assert_eq!(
        image[(55 * screen.width + 64) as usize],
        glass::material(theme::SURFACE, 0x102030)
    );
    assert!(underlay.iter().all(|p| *p == 0x102030));
    assert!(checkpoints > 0);
}

#[test]
fn malformed_public_effect_inputs_refuse_before_mutating_the_destination() {
    let screen = Bounds::new(0, 0, 128, 96);
    let window = Bounds::new(24, 24, 80, 48);
    let saved = shadow::footprint(window, screen).unwrap();
    let underlay = vec![0x102030; (saved.width * saved.height) as usize];
    let surface = vec![0; (window.width * window.height) as usize];
    let original = vec![0xabcdef; (screen.width * screen.height) as usize];
    let mut image = original.clone();
    for clip in [
        Bounds::empty(),
        Bounds::new(u32::MAX, 0, 2, 2),
        Bounds::new(0, u32::MAX, 2, 2),
    ] {
        assert!(shadow::apply_clipped(
            &mut image,
            screen,
            window,
            &underlay,
            saved,
            saved.width,
            clip,
            &mut || {}
        )
        .is_none());
        assert!(corners::apply_clipped(
            &mut image,
            screen,
            window,
            &surface,
            &underlay,
            saved,
            saved.width,
            clip
        )
        .is_none());
        assert!(glass::compose_clipped(
            &mut image,
            screen,
            window,
            28,
            &surface,
            &underlay,
            saved,
            saved.width,
            clip,
            &mut || {}
        )
        .is_none());
        assert_eq!(image, original);
    }
    let repair = glass::PartialRepair::plan(
        Bounds::new(45, 45, 1, 1),
        screen,
        [window].into_iter(),
        saved.width,
        underlay.len() + 4096,
    )
    .unwrap();
    assert!(repair.layer_clip(usize::MAX, screen).is_none());
    let mut invalid = repair;
    invalid.work.x = u32::MAX;
    assert!(invalid
        .restore_halo(&mut image, screen, &underlay, &mut || {})
        .is_none());
    assert_eq!(image, original);
    assert!(glass::uniform_color(&underlay, Bounds::empty(), saved.width, &mut || {}).is_none());
}
