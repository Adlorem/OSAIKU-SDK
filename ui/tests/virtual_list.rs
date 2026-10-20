#![cfg(feature = "compiler")]
use osaiku_ui::{compiler::compile, layout::Layout, virtual_list::*, *};
use std::{cell::RefCell, collections::BTreeSet};

const HTML: &str = r#"<main id="root"><header id="head"><p id="title">List</p></header><ui-virtual-list id="rows" source="data" item-height="36" overscan="2"><template><button id="item" action="open" bind:text="name"></button></template></ui-virtual-list><footer id="foot"><p id="status">Ready</p></footer></main>"#;
const CSS: &str =
    "main { padding:12px; gap:8px; overflow:hidden; } header {height:40px;} footer {height:30px;}";
fn at(d: &Document, id: &str) -> usize {
    d.nodes[..d.len as usize]
        .iter()
        .position(|n| n.id == symbol(id))
        .unwrap()
}
struct Data {
    requested: RefCell<BTreeSet<u32>>,
}
impl Items for Data {
    fn get(&self, source: u32, ordinal: u32, key: u32) -> Option<Value<'_>> {
        assert_eq!(source, symbol("data"));
        assert_eq!(key, symbol("name"));
        self.requested.borrow_mut().insert(ordinal);
        Some(Value::Text("item"))
    }
}
#[test]
fn binds_only_visible_rows_and_overscan_with_real_scroll_extent() {
    let mut view = compile(HTML, CSS).unwrap();
    let i = at(&view, "rows");
    view.lists[view.nodes[i].value as usize].total = 20_000;
    view.nodes[i].scroll_y = 36 * 10_000 + 7;
    let mut offsets = [0; MAX_NODES];
    offsets[i] = view.nodes[i].scroll_y;
    let layout = Layout::compute(&view, 800, 600, &offsets).unwrap();
    let requested = requested(&view, &layout, i).unwrap();
    assert!(requested.count < 25);
    assert_eq!(requested.first, 9998);
    let data = Data {
        requested: RefCell::default(),
    };
    let mut out = Document::EMPTY;
    realize(&view, &layout, &data, &mut out).unwrap();
    assert_eq!(
        *data.requested.borrow(),
        (requested.first..requested.first + requested.count).collect()
    );
    assert!(out.len < 35);
    let mut wire = [0; MAX_WIRE];
    let len = out.encode(&mut wire).unwrap();
    assert_eq!(out, Document::decode(&wire[..len]).unwrap());
    let scene = paint::Scene::from_document(&out, 800, 600).unwrap();
    let list = at(&out, "rows");
    assert_eq!(scene.layout().nodes[list].content_height, 720_000);
    let bar = scene.scrollbar(list).unwrap();
    assert!(bar.maximum > 700_000);
    for (i, node) in out.nodes[..out.len as usize].iter().enumerate() {
        if node.parent == list as u16 {
            assert_eq!(scene.layout().nodes[i].bounds.right(), bar.track.x);
            assert!(scene.layout().nodes[i].clip.right() <= bar.track.x);
        }
    }
    let visible = scene
        .semantics()
        .find(|s| s.action == symbol("open") && s.visible_bounds.height > 0)
        .unwrap();
    let target = Identity {
        epoch: 1,
        owner: 1,
        window: 2,
        incarnation: 1,
        revision: 1,
    };
    let event = input::pointer_event(
        &out,
        &scene,
        target,
        visible.visible_bounds.x + 5,
        visible.visible_bounds.y + 1,
    )
    .unwrap();
    assert_eq!(event.element, visible.id);
    event.validate(target, &out).unwrap();
    let stale = Identity {
        revision: 2,
        ..target
    };
    assert_eq!(event.validate(stale, &out), Err(Error::Stale));
}
#[test]
fn anchored_to_content_and_resize_changes_requested_rows() {
    let mut view = compile(HTML, CSS).unwrap();
    let i = at(&view, "rows");
    view.lists[view.nodes[i].value as usize].total = 1000;
    let mut previous = 0;
    for (w, h) in [(320, 240), (800, 600), (1400, 900)] {
        let l = Layout::compute(&view, w, h, &[0; MAX_NODES]).unwrap();
        let r = l.nodes[i].bounds;
        assert_eq!(r.x, 12);
        assert_eq!(r.width, w - 24);
        assert_eq!(r.y, l.nodes[at(&view, "head")].bounds.bottom() + 8);
        assert_eq!(r.bottom() + 8, l.nodes[at(&view, "foot")].bounds.y);
        assert_eq!(l.nodes[at(&view, "foot")].bounds.bottom(), h as i32 - 12);
        let requested = requested(&view, &l, i).unwrap();
        assert!(requested.count > previous);
        previous = requested.count;
    }
}
#[test]
fn clamps_end_empty_and_shrinking_data_without_underflow() {
    let mut c = Config {
        source: 1,
        item_height: 36,
        overscan: 2,
        total: 1000,
        first: 0,
    };
    let r = range(c, u32::MAX, 100, 16).unwrap();
    assert_eq!(r.scroll, 35_900);
    assert_eq!(r.first + r.count, 1000);
    c.total = 1;
    assert_eq!(
        range(c, 900, 200, 16).unwrap(),
        Range {
            first: 0,
            count: 1,
            scroll: 0
        }
    );
    c.total = 0;
    assert_eq!(range(c, 900, 200, 16).unwrap(), Range::default());
    c.total = 10;
    assert_eq!(range(c, u32::MAX, u32::MAX, 16).unwrap().count, 10);
    assert_eq!(range(c, 0, 360, 4), Err(Error::Limit));
    c.total = u32::MAX;
    assert_eq!(range(c, 0, 100, 16), Err(Error::Limit));
}
#[test]
fn refuses_bad_templates_extents_and_binding_failures() {
    for html in [
        HTML.replace("36", "0"),
        HTML.replace("overscan=\"2\"", "overscan=\"9\""),
        HTML.replace("source=\"data\"", ""),
        HTML.replace("</template>", "<p id=\"extra\">Bad</p></template>"),
    ] {
        assert!(compile(&html, CSS).is_err());
    }
    let mut view = compile(HTML, CSS).unwrap();
    let i = at(&view, "rows");
    view.lists[view.nodes[i].value as usize].total = u32::MAX;
    assert!(view.validate().is_err());
    view.lists[view.nodes[i].value as usize].total = 10;
    view.lists[view.nodes[i].value as usize].first = 11;
    assert!(view.validate().is_err());
}

#[test]
fn invalid_configuration_and_missing_items_are_refused_before_publication() {
    let mut view = compile(HTML, CSS).unwrap();
    let i = at(&view, "rows");
    view.nodes[i].value = u16::MAX;
    assert_eq!(view.validate(), Err(Error::Structure));
    assert!(Layout::compute(&view, 800, 600, &[0; MAX_NODES]).is_err());
    assert!(!in_template(&view, usize::MAX));
    view.nodes[i].value = 0;
    view.lists[0].total = 2;
    struct Missing;
    impl Items for Missing {
        fn get(&self, _: u32, _: u32, _: u32) -> Option<Value<'_>> {
            None
        }
    }
    let layout = Layout::compute(&view, 800, 600, &[0; MAX_NODES]).unwrap();
    let mut refused = Document::EMPTY;
    assert_eq!(
        realize(&view, &layout, &Missing, &mut refused),
        Err(Error::Binding)
    );
    assert!(compile(
        &HTML.replace(
            "<button id=\"item\" action=\"open\" bind:text=\"name\"></button>",
            "<ui-chart id=\"item\" bind:series=\"series\"></ui-chart>"
        ),
        CSS
    )
    .is_err());
}
#[test]
fn hidden_list_never_calls_provider_and_id_is_stable_across_recycling() {
    let mut view = compile(HTML, CSS).unwrap();
    let i = at(&view, "rows");
    view.lists[view.nodes[i].value as usize].total = 100;
    view.nodes[i].state |= state::HIDDEN;
    let l = Layout::compute(&view, 800, 600, &[0; MAX_NODES]).unwrap();
    let data = Data {
        requested: RefCell::default(),
    };
    let mut out = Document::EMPTY;
    realize(&view, &l, &data, &mut out).unwrap();
    assert!(data.requested.borrow().is_empty());
    assert_ne!(item_id(1, 2, 0), item_id(1, 2, 1));
    assert_ne!(item_id(1, 2, 1), item_id(2, 2, 1));
}

#[test]
fn multiple_lists_share_capacity_and_trim_overscan_before_visible_rows() {
    let html = r#"<main id="root"><ui-virtual-list id="a" source="data" item-height="20" overscan="8"><template><ui-list-item id="ar"><p id="ac" bind:text="name"></p></ui-list-item></template></ui-virtual-list><ui-virtual-list id="b" source="data" item-height="20" overscan="8"><template><ui-list-item id="br"><p id="bc" bind:text="name"></p></ui-list-item></template></ui-virtual-list></main>"#;
    let mut view = compile(html, "main { display:flex; flex-direction:row; align-items:stretch; } ui-virtual-list { width:50%; }").unwrap();
    for c in &mut view.lists[..2] {
        c.total = 1000;
    }
    let layout = Layout::compute(&view, 800, 500, &[0; MAX_NODES]).unwrap();
    let (a, b) = (at(&view, "a"), at(&view, "b"));
    let (ra, rb) = (
        requested(&view, &layout, a).unwrap(),
        requested(&view, &layout, b).unwrap(),
    );
    assert!(ra.count >= 25 && rb.count >= 25);
    assert!(3 + 2 * (ra.count + rb.count) <= MAX_NODES as u32);
    let data = Data {
        requested: RefCell::default(),
    };
    let mut out = Document::EMPTY;
    realize(&view, &layout, &data, &mut out).unwrap();
    for (id, r) in [("a", ra), ("b", rb)] {
        assert_eq!(
            out.nodes[..out.len as usize]
                .iter()
                .filter(|n| n.parent == at(&out, id) as u16)
                .count(),
            r.count as usize
        );
    }
    // Neither list may silently lose visible rows when the combined view is too large.
    let overflow_height = (MAX_NODES as u32 / 4 + 1) * 20;
    let large = Layout::compute(&view, 800, overflow_height, &[0; MAX_NODES]).unwrap();
    assert_eq!(requested(&view, &large, a), Err(Error::Limit));
    assert_eq!(realize(&view, &large, &data, &mut out), Err(Error::Limit));
}

#[test]
fn realization_preserves_list_sources_for_valid_non_depth_first_node_order() {
    let html = r#"<main id="root"><section id="group"><ui-virtual-list id="a" source="data" item-height="36"><template><p id="ac" bind:text="name"></p></template></ui-virtual-list></section><ui-virtual-list id="b" source="data" item-height="36"><template><p id="bc" bind:text="name"></p></template></ui-virtual-list></main>"#;
    let original = compile(html, "").unwrap();
    let mut view = original.clone();
    // Parent-before-child order is the public Document contract, not depth-first order.
    let order = [0, 1, 5, 6, 7, 2, 3, 4];
    for (new, old) in order.into_iter().enumerate() {
        let mut n = original.nodes[old];
        if n.parent != NONE {
            n.parent = order.iter().position(|v| *v == n.parent as usize).unwrap() as u16;
        }
        if n.kind == Kind::VirtualList {
            n.value = 1 - n.value;
        }
        view.nodes[new] = n;
    }
    view.lists[0].total = 2; // b is first in the serialized node order.
    view.lists[1].total = 1; // a is visited first by tree traversal.
    view.validate().unwrap();
    let layout = Layout::compute(&view, 800, 500, &[0; MAX_NODES]).unwrap();
    let data = Data {
        requested: RefCell::default(),
    };
    let mut out = Document::EMPTY;
    realize(&view, &layout, &data, &mut out).unwrap();
    assert_eq!(out.lists[out.nodes[at(&out, "a")].value as usize].total, 1);
    assert_eq!(out.lists[out.nodes[at(&out, "b")].value as usize].total, 2);
}
