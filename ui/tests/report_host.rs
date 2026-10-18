#![cfg(feature = "compiler")]
use osaiku_ui::{
    compiler::compile,
    report::{
        host::{Error as HostError, Host, Surface},
        parse_manifest, region,
    },
    *,
};

const CONTRACT: &str = "report 1 10\nsource local record 1\nfield term local text\naction choose select\naction filter filter\n";
fn view() -> Document {
    compile(r#"<main id="report"><section id="scroll"><button id="item" action="choose">Item</button><ui-text-input id="filter" action="filter" bind:text="term">initial</ui-text-input></section></main>"#,
        "#scroll { overflow:auto; height:40px; } button { height:60px; }").unwrap()
}
fn identity(revision: u64) -> Identity {
    Identity {
        epoch: 1,
        owner: 9,
        window: 5,
        incarnation: 3,
        revision,
    }
}
fn event(kind: u32, element: &str, action: &str, value: i32) -> input::Event {
    input::Event {
        target: identity(1),
        kind,
        element: symbol(element),
        action: if action.is_empty() { 0 } else { symbol(action) },
        value,
        ..input::Event::EMPTY
    }
}
fn index(document: &Document, id: &str) -> usize {
    document.nodes[..document.len as usize]
        .iter()
        .position(|n| n.id == symbol(id))
        .unwrap()
}
fn wire(document: &Document) -> Vec<u8> {
    let mut out = vec![0; MAX_WIRE];
    let len = document.encode(&mut out).unwrap();
    out.truncate(len);
    out
}

#[test]
fn replacement_is_transactional_and_tokens_cannot_replay_across_reopen() {
    let manifest = parse_manifest(CONTRACT).unwrap();
    let contract = manifest.contract();
    let document = view();
    let mut host = Box::new(Host::EMPTY);
    let session = host.open(9, 2, Surface::Window).unwrap();
    let first = host
        .prepare(session, 0, &contract, &document, None)
        .unwrap();
    assert!(host.current(session).unwrap().is_none());
    assert_eq!(
        host.prepare(session, 0, &contract, &document, None),
        Err(HostError::Busy)
    );
    assert_eq!(
        host.commit(
            first,
            Identity {
                owner: 10,
                ..identity(1)
            }
        ),
        Err(HostError::Owner)
    );
    host.commit(first, identity(1)).unwrap();
    let original = wire(host.current(session).unwrap().unwrap());
    assert_eq!(host.commit(first, identity(2)), Err(HostError::Stale));
    assert_eq!(
        host.prepare(session, 0, &contract, &document, None),
        Err(HostError::Stale)
    );
    let second = host
        .prepare(session, 1, &contract, &document, None)
        .unwrap();
    assert_eq!(wire(host.current(session).unwrap().unwrap()), original);
    host.discard(second).unwrap();
    assert_eq!(host.revision(session), Ok(1));
    let third = host
        .prepare(session, 1, &contract, &document, None)
        .unwrap();
    assert_ne!(second, third);
    assert_eq!(host.commit(second, identity(2)), Err(HostError::Stale));
    host.close(session).unwrap();
    assert_eq!(host.commit(third, identity(2)), Err(HostError::Closed));
    let reopened = host.open(9, 2, Surface::Window).unwrap();
    assert_ne!(session.generation, reopened.generation);
    assert_eq!(host.revision(session), Err(HostError::Owner));
    assert_eq!(host.revision(reopened), Ok(0));
    assert!(host.current(reopened).unwrap().is_none());
}

#[test]
fn compatible_updates_keep_user_state_without_mutating_the_accepted_document() {
    let manifest = parse_manifest(CONTRACT).unwrap();
    let contract = manifest.contract();
    let mut document = view();
    let mut host = Box::new(Host::EMPTY);
    let session = host.open(9, 1, Surface::Window).unwrap();
    let token = host
        .prepare(session, 0, &contract, &document, None)
        .unwrap();
    host.commit(token, identity(1)).unwrap();
    let original = wire(host.current(session).unwrap().unwrap());
    host.event(session, event(input::ACTIVATE, "item", "choose", 0))
        .unwrap();
    host.event(session, event(input::SCROLL, "scroll", "", 17))
        .unwrap();
    host.event(session, event(input::FOCUS, "filter", "", 0))
        .unwrap();
    host.set_filter(session, identity(1), symbol("filter"), "Zażółć <literal>")
        .unwrap();
    assert_eq!(
        host.filter(session, symbol("filter")),
        Ok("Zażółć <literal>")
    );
    assert_eq!(wire(host.current(session).unwrap().unwrap()), original);
    // A responsive/layout edit is compatible with these application identities.
    document.nodes[0].style.padding = 12;
    let token = host
        .prepare(session, 1, &contract, &document, None)
        .unwrap();
    let prepared = host.prepared(token).unwrap();
    assert_ne!(
        prepared.nodes[index(prepared, "item")].state & state::SELECTED,
        0
    );
    assert_eq!(prepared.nodes[index(prepared, "scroll")].scroll_y, 17);
    assert_ne!(
        prepared.nodes[index(prepared, "filter")].state & state::FOCUSED,
        0
    );
    assert_eq!(
        prepared.text_of(index(prepared, "filter")),
        "Zażółć <literal>"
    );
    host.commit(token, identity(2)).unwrap();
    assert_eq!(
        host.event(session, event(input::ACTIVATE, "item", "choose", 0)),
        Err(HostError::Stale)
    );
    assert_eq!(
        host.set_filter(session, identity(1), symbol("filter"), "stale"),
        Err(HostError::Stale)
    );
}

#[test]
fn changed_source_schema_drops_filter_and_authoritative_restrictions_win() {
    let manifest = parse_manifest(CONTRACT).unwrap();
    let mut host = Box::new(Host::EMPTY);
    let session = host.open(9, 1, Surface::Window).unwrap();
    let mut document = view();
    let token = host
        .prepare(session, 0, &manifest.contract(), &document, None)
        .unwrap();
    host.commit(token, identity(1)).unwrap();
    host.set_filter(session, identity(1), symbol("filter"), "private old filter")
        .unwrap();
    host.event(session, event(input::FOCUS, "filter", "", 0))
        .unwrap();
    let filter = index(&document, "filter");
    document.nodes[filter].state |= state::DISABLED;
    let updated = CONTRACT.replace("record 1", "record 2");
    let updated = parse_manifest(&updated).unwrap();
    let token = host
        .prepare(session, 1, &updated.contract(), &document, None)
        .unwrap();
    let next = host.prepared(token).unwrap();
    assert_eq!(next.text_of(filter), "initial");
    assert_eq!(next.nodes[filter].state & state::FOCUSED, 0);
    assert_ne!(next.nodes[filter].state & state::DISABLED, 0);
    host.commit(token, identity(2)).unwrap();
    assert_eq!(
        host.set_filter(session, identity(2), symbol("filter"), "x"),
        Err(HostError::View(Error::Disabled))
    );
}

#[test]
fn region_ownership_clipping_and_surrounding_controls_are_preserved() {
    let manifest = parse_manifest(CONTRACT).unwrap();
    let surrounding = compile(r#"<main id="host"><button id="outside" action="outside">Host</button><section id="region"></section></main>"#,
        "#region { height:50px; width:90px; }").unwrap();
    let mut host = Box::new(Host::EMPTY);
    let session = host.open(9, 1, Surface::Region(symbol("region"))).unwrap();
    let report = view();
    let token = host
        .prepare(
            session,
            0,
            &manifest.contract(),
            &report,
            Some(&surrounding),
        )
        .unwrap();
    let combined = host.prepared(token).unwrap();
    assert_eq!(
        combined.nodes[index(combined, "report")].parent as usize,
        index(combined, "region")
    );
    let scene = paint::Scene::from_document(combined, 200, 200).unwrap();
    let clip = scene.layout().nodes[index(combined, "region")].bounds;
    for node in &report.nodes[..report.len as usize] {
        let i = combined.nodes[..combined.len as usize]
            .iter()
            .position(|n| n.id == node.id)
            .unwrap();
        let visible = scene.layout().nodes[i].clip;
        if visible.width != 0 && visible.height != 0 {
            assert_eq!(visible.intersect(clip), visible);
        }
    }
    host.commit(token, identity(1)).unwrap();
    assert_eq!(
        host.event(session, event(input::ACTIVATE, "outside", "outside", 0)),
        Err(HostError::Owner)
    );
    host.event(session, event(input::FOCUS, "filter", "", 0))
        .unwrap();
    let mut focused_host = surrounding.clone();
    focused_host.set_focus(symbol("outside"));
    let focus_update = host
        .prepare(
            session,
            1,
            &manifest.contract(),
            &report,
            Some(&focused_host),
        )
        .unwrap();
    let candidate = host.prepared(focus_update).unwrap();
    assert_eq!(
        candidate.nodes[index(candidate, "outside")].state & state::FOCUSED,
        0
    );
    assert_ne!(
        candidate.nodes[index(candidate, "filter")].state & state::FOCUSED,
        0
    );
    host.discard(focus_update).unwrap();
    let before = wire(host.current(session).unwrap().unwrap());
    let collision = compile(
        r#"<main id="report"><section id="region"></section></main>"#,
        "",
    )
    .unwrap();
    assert!(host
        .prepare(session, 1, &manifest.contract(), &report, Some(&collision))
        .is_err());
    assert_eq!(wire(host.current(session).unwrap().unwrap()), before);
    let mut disabled = surrounding.clone();
    disabled.nodes[0].state = state::DISABLED;
    let token = host
        .prepare(session, 1, &manifest.contract(), &report, Some(&disabled))
        .unwrap();
    host.commit(token, identity(2)).unwrap();
    let mut click = event(input::ACTIVATE, "item", "choose", 0);
    click.target = identity(2);
    assert_eq!(
        host.event(session, click),
        Err(HostError::View(Error::Disabled))
    );
}

#[test]
fn region_remaps_shared_style_series_and_list_tables() {
    let host = compile(r#"<main id="host"><ui-chart id="host-chart"></ui-chart><section id="slot"></section></main>"#,
        "#slot { border-radius:8px; } main:disabled { color:#123456; }").unwrap();
    let report = compile(r#"<main id="report"><ui-chart id="chart"></ui-chart><ui-virtual-list id="rows" source="rows"><template><span id="label" bind:text="label"></span></template></ui-virtual-list></main>"#,
        "#report { border-radius:4px; } #report:disabled { color:#654321; }").unwrap();
    let mut output = Box::new(Document::EMPTY);
    region::mount(&host, symbol("slot"), &report, &mut output).unwrap();
    output.validate().unwrap();
    let root = index(&output, "report");
    assert_eq!(output.decoration(output.nodes[root].style).radius, 4);
    assert_eq!(
        output.lists[output.nodes[index(&output, "rows")].value as usize].source,
        symbol("rows")
    );
    // The full composition remains an ordinary, roundtrippable UI artifact.
    Document::decode(&wire(&output)).unwrap();
}

#[test]
fn accepted_window_identity_cannot_be_rebound_and_epoch_recovery_revokes_input() {
    let manifest = parse_manifest(CONTRACT).unwrap();
    let mut host = Box::new(Host::EMPTY);
    let session = host.open(9, 1, Surface::Window).unwrap();
    let document = view();
    let token = host
        .prepare(session, 0, &manifest.contract(), &document, None)
        .unwrap();
    host.commit(token, identity(9)).unwrap();
    let token = host
        .prepare(session, 1, &manifest.contract(), &document, None)
        .unwrap();
    assert_eq!(
        host.commit(
            token,
            Identity {
                window: 6,
                ..identity(10)
            }
        ),
        Err(HostError::Owner)
    );
    assert_eq!(host.commit(token, identity(9)), Err(HostError::Stale));
    host.commit(
        token,
        Identity {
            epoch: 2,
            ..identity(1)
        },
    )
    .unwrap();
    assert_eq!(
        host.event(session, event(input::ACTIVATE, "item", "choose", 0)),
        Err(HostError::Stale)
    );
}
