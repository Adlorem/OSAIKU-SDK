#![cfg(feature = "compiler")]
use osaiku_ui::{
    compiler::compile,
    layout::{HostSlot, Rect},
    paint::Scene,
    symbol, Error,
};
fn slot(id: &str, x: i32, width: u32) -> HostSlot {
    HostSlot {
        id: symbol(id),
        bounds: Rect {
            x,
            y: 0,
            width,
            height: 40,
        },
    }
}
#[test]
fn host_slots_clip_descendants_and_share_input_semantic_geometry() {
    let doc = compile(r#"<main id="r"><section id="a"><button id="b" action="go">OK</button></section><section id="c" disabled><button id="d" action="no">No</button></section></main>"#, "button { width: 200px; height: 40px; }").unwrap();
    let scene = Scene::in_slots(&doc, 100, 40, &[slot("a", 0, 40), slot("c", 60, 40)]).unwrap();
    assert_eq!(scene.hit(39, 20), Some(symbol("b")));
    assert_eq!(scene.hit(40, 20), None);
    assert_eq!(scene.hit(70, 20), None);
    let b = scene.semantics().find(|s| s.id == symbol("b")).unwrap();
    assert_eq!(b.visible_bounds.width, 40);
    assert_eq!(b.parent, Some(symbol("a")));
    assert!(b.clipped);
}
#[test]
fn invalid_host_replacement_cannot_produce_a_partial_scene() {
    let doc = compile(r#"<main id="r"><section id="a"><span id="b">B</span></section><section id="c"></section></main>"#, "").unwrap();
    for slots in [
        vec![slot("a", 0, 40)],
        vec![slot("a", 0, 40), slot("a", 50, 40)],
        vec![slot("b", 0, 40), slot("c", 60, 40)],
    ] {
        assert!(matches!(
            Scene::in_slots(&doc, 100, 40, &slots),
            Err(Error::Structure)
        ));
    }
    assert!(matches!(
        Scene::in_slots(&doc, 100, 40, &[slot("a", -1, 40), slot("c", 60, 40)]),
        Err(Error::Limit)
    ));
    assert!(matches!(
        Scene::in_slots(&doc, 100, 40, &[slot("a", 0, 40), slot("c", 61, 40)]),
        Err(Error::Limit)
    ));
    assert!(Scene::in_slots(&doc, 100, 40, &[slot("a", 0, 40), slot("c", 60, 40)]).is_ok());
}

#[test]
fn borrowed_host_text_is_literal_and_matches_painted_semantics() {
    use osaiku_ui::paint::Painter;
    #[derive(Default)]
    struct Text(Vec<String>);
    impl Painter for Text {
        fn fill_alpha(&mut self, _: Rect, _: u32, _: u8, _: Rect) {}
        fn text(&mut self, _: Rect, text: &str, _: u16, _: u32, _: Rect) {
            self.0.push(text.into());
        }
        fn line(&mut self, _: (i32, i32), _: (i32, i32), _: u32, _: Rect) {}
    }
    let doc = compile(
        r#"<main id="r"><span id="label" bind:text="title">Default</span></main>"#,
        "",
    )
    .unwrap();
    let scene = Scene::from_document(&doc, 200, 40)
        .unwrap()
        .with_text_binding(symbol("title"), "<b>Żółć</b>")
        .unwrap();
    let mut painter = Text::default();
    scene.paint(&mut painter);
    assert_eq!(painter.0, ["<b>Żółć</b>"]);
    assert_eq!(scene.semantics().nth(1).unwrap().text, "<b>Żółć</b>");
    assert_eq!(doc.text_of(1), "Default");
    assert!(matches!(
        Scene::from_document(&doc, 200, 40)
            .unwrap()
            .with_text_binding(symbol("missing"), "data"),
        Err(Error::Binding)
    ));
}
