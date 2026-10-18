#![cfg(feature = "compiler")]
use osaiku_ui::{compiler::compile, layout::Rect, paint::Scene, surface::*};

#[test]
fn sealed_layout_preserves_scrolling_hit_tests_and_semantics_without_reflow() {
    let mut view = compile(
        "<main id=\"root\"><section id=\"list\"><button id=\"a\" action=\"a\">Zażółć</button><button id=\"b\" action=\"b\">Gęślą</button><button id=\"c\" action=\"c\">Jaźń</button></section></main>",
        "#list { height: 40px; overflow: auto; } button { height: 30px; }",
    ).unwrap();
    view.nodes[1].scroll_y = 15;
    let scene = Scene::from_document(&view, 120, 80).unwrap();
    let mut metadata = Box::new(Publication::EMPTY);
    metadata
        .prepare(
            &scene,
            Handle {
                target: Target {
                    owner: (1 << 32) | 8,
                    window: 2,
                    incarnation: 3,
                    epoch: 4,
                },
                serial: 5,
                revision: 6,
            },
            &[],
        )
        .unwrap();
    metadata.source_hash = 7;
    let restored = Scene::from_layout(&view, metadata.layout(&view).unwrap()).unwrap();
    for (a, b) in scene.layout().nodes[..view.len as usize]
        .iter()
        .zip(restored.layout().nodes.iter())
    {
        assert_eq!(
            (a.bounds, a.content_height, a.scroll_y),
            (b.bounds, b.content_height, b.scroll_y)
        );
        if a.clip.width != 0 && a.clip.height != 0 {
            assert_eq!(a.clip, b.clip);
        }
    }
    for y in 0..80 {
        for x in 0..120 {
            assert_eq!(
                scene.layout().hit(&view, x, y),
                restored.layout().hit(&view, x, y)
            );
        }
    }
    assert_eq!(
        scene.scrollbar(1).map(|b| (b.track, b.thumb, b.maximum)),
        restored.scrollbar(1).map(|b| (b.track, b.thumb, b.maximum))
    );
    let mut clipped = metadata.layout(&view).unwrap();
    for node in &mut clipped.nodes[..clipped.len] {
        node.clip = node.clip.intersect(Rect {
            x: 0,
            y: 0,
            width: 20,
            height: 20,
        });
    }
    assert_eq!(
        clipped.viewport.width, 120,
        "resize must not reflow a sealed image"
    );
    assert_eq!(clipped.hit(&view, 21, 10), None);
    metadata.placements[1].x = i32::MAX;
    assert!(metadata.layout(&view).is_err());
    metadata.placements[1].x = 0;
    metadata.elements[1].action ^= 1;
    assert!(metadata.layout(&view).is_err());
}
