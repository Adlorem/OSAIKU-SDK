#![cfg(feature = "compiler")]
use osaiku_ui::{
    compiler::compile,
    report::{
        host::{Host, Surface},
        parse_manifest,
    },
    *,
};

#[test]
fn materialized_rows_keep_source_scope_without_reusing_ordinal_selection() {
    let manifest = parse_manifest(
        "report 1 10\nsource data rows 1\nfield name data text\naction choose select",
    )
    .unwrap();
    let mut template = compile(r#"<main id="root"><ui-virtual-list id="list" source="data" item-height="32"><template><ui-list-item id="row" action="choose"><span id="label" bind:text="name"></span></ui-list-item></template></ui-virtual-list></main>"#,
        "main {height:120px;} ui-virtual-list {height:64px;}").unwrap();
    template.lists[0].total = 2;
    let layout = layout::Layout::compute(&template, 200, 120, &[0; MAX_NODES]).unwrap();
    struct Items(bool);
    impl virtual_list::Items for Items {
        fn get(&self, source: u32, ordinal: u32, field: u32) -> Option<Value<'_>> {
            if source != symbol("data") || field != symbol("name") {
                return None;
            }
            Some(Value::Text(if (ordinal == 0) == self.0 {
                "first"
            } else {
                "second"
            }))
        }
    }
    let mut realized = Box::new(Document::EMPTY);
    virtual_list::realize(&template, &layout, &Items(false), &mut realized).unwrap();
    manifest.contract().validate_document(&realized).unwrap();
    let mut host = Box::new(Host::EMPTY);
    let session = host.open(9, 1, Surface::Window).unwrap();
    let token = host
        .prepare(session, 0, &manifest.contract(), &realized, None)
        .unwrap();
    let identity = Identity {
        owner: 9,
        epoch: 1,
        window: 1,
        incarnation: 1,
        revision: 1,
    };
    host.commit(token, identity).unwrap();
    let selected = virtual_list::item_id(symbol("list"), symbol("row"), 0);
    let event = input::Event::activation(Action {
        target: identity,
        element: selected,
        action: symbol("choose"),
    });
    assert_eq!(
        host.event(session, event).unwrap(),
        Some(catalog::ActionKind::Select)
    );
    virtual_list::realize(&template, &layout, &Items(true), &mut realized).unwrap();
    let token = host
        .prepare(session, 1, &manifest.contract(), &realized, None)
        .unwrap();
    let next = host.prepared(token).unwrap();
    let row = next.nodes[..next.len as usize]
        .iter()
        .find(|n| n.id == selected)
        .unwrap();
    assert_eq!(row.state & state::SELECTED, 0);
}
