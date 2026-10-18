use osaiku_runtime::report::{
    artifact::{self, Header},
    ReportView, Surface,
};
use osaiku_ui::{
    compiler::compile, report::parse_manifest, symbol, Bindings, Document, Identity, Value,
    MAX_WIRE,
};
fn header(kind: u64) -> Header {
    Header {
        kind,
        recipe: 1,
        settings: 17,
        contract: 1,
        recorded: Identity {
            owner: 9,
            window: 2,
            incarnation: 3,
            epoch: 4,
            revision: 1,
        },
        mount: 7,
        catalog: 8,
    }
}
#[test]
fn envelope_is_exact_bounded_versioned_and_corruption_detecting() {
    let mut b = vec![0; artifact::MAX_BYTES];
    b[artifact::HEADER..].fill(37);
    let n = header(artifact::DEFINITION).seal(&mut b, MAX_WIRE).unwrap();
    assert_eq!(
        Header::parse(&b[..n]).unwrap().0,
        header(artifact::DEFINITION)
    );
    for offset in [
        0,
        16,
        24,
        32,
        40,
        48,
        56,
        64,
        72,
        80,
        88,
        96,
        104,
        112,
        120,
        128,
        n - 1,
    ] {
        b[offset] ^= 1;
        assert!(Header::parse(&b[..n]).is_err(), "{offset}");
        b[offset] ^= 1;
    }
    b[8] = 2;
    assert_eq!(Header::parse(&b), Err(artifact::Error::Version));
    b[8] = 1;
    assert!(Header::parse(&b[..n - 1]).is_err());
    b.push(0);
    assert!(Header::parse(&b).is_err());
    assert!(header(3).seal(&mut b, 1).is_err());
    assert!(header(1).seal(&mut b, MAX_WIRE + 1).is_err());
}
#[test]
fn compatibility_refuses_provider_schema_and_recipe_changes() {
    let manifest = format!(
        "report 1 {}\nsource sample record 1\nfield label sample text",
        osaiku_ui::VERSION
    );
    let m = parse_manifest(&manifest).unwrap();
    let changed_manifest = format!(
        "report 1 {}\nsource sample record 2\nfield label sample text",
        osaiku_ui::VERSION
    );
    let changed = parse_manifest(&changed_manifest).unwrap();
    let mut h = header(1);
    h.contract = artifact::contract_key(&m.contract());
    assert!(h.compatible(&m.contract(), 1).is_ok());
    assert!(h.compatible(&m.contract(), 2).is_err());
    assert!(h.compatible(&changed.contract(), 1).is_err());
}
#[test]
fn committed_snapshots_are_frozen_and_regions_do_not_export_host_text() {
    static HOST: ReportView = ReportView::new();
    let manifest = format!(
        "report 1 {}\nsource sample record 1\nfield label sample text\nfield control sample text\naction save select",
        osaiku_ui::VERSION
    );
    let m = parse_manifest(&manifest).unwrap();
    let doc=compile(r#"<main id="root"><p id="value" bind:text="label"></p><button id="control" action="save" bind:text="control"></button></main>"#,"").unwrap();
    let mut template = vec![0; MAX_WIRE];
    let n = doc.encode(&mut template).unwrap();
    template.truncate(n);
    let outer = compile(
        r#"<main id="host"><p id="private">host secret</p><section id="anchor"></section></main>"#,
        "",
    )
    .unwrap();
    let mut surrounding = vec![0; MAX_WIRE];
    let n = outer.encode(&mut surrounding).unwrap();
    surrounding.truncate(n);
    struct Data(&'static str);
    impl Bindings for Data {
        fn get(&self, key: u32) -> Option<Value<'_>> {
            Some(Value::Text(if key == symbol("label") {
                self.0
            } else {
                "live control"
            }))
        }
    }
    for region in [false, true] {
        let session = HOST
            .open(
                9,
                1,
                if region {
                    Surface::Region(symbol("anchor"))
                } else {
                    Surface::Window
                },
            )
            .unwrap();
        let mut wire = vec![0; MAX_WIRE];
        let (p, _) = HOST
            .prepare(
                session,
                0,
                &m.contract(),
                &template,
                &Data("recorded value"),
                region.then_some(surrounding.as_slice()),
                &mut wire,
            )
            .unwrap();
        let mut id = header(1).recorded;
        HOST.commit(p, id).unwrap();
        // A refused candidate must never leak into the saved accepted snapshot.
        let (p, _) = HOST
            .prepare(
                session,
                1,
                &m.contract(),
                &template,
                &Data("refused value"),
                region.then_some(surrounding.as_slice()),
                &mut wire,
            )
            .unwrap();
        HOST.discard(p).unwrap();
        let mut h = header(artifact::SNAPSHOT);
        h.contract = artifact::contract_key(&m.contract());
        let mut saved = vec![0; artifact::MAX_BYTES];
        let n = HOST
            .save_artifact(
                session,
                &template,
                h,
                &[symbol("control")],
                &[symbol("save")],
                &mut saved,
            )
            .unwrap();
        saved.truncate(n);
        let (actual, payload) = Header::parse(&saved).unwrap();
        assert_eq!(actual.recorded, id);
        let snapshot = Document::decode(payload).unwrap();
        assert_eq!(snapshot.len, 3);
        assert_eq!(snapshot.text_of(1), "recorded value");
        assert!(!snapshot.text.windows(11).any(|s| s == b"host secret"));
        assert_eq!(snapshot.nodes[1].bindings, [0; 6]);
        HOST.open_artifact(
            session,
            &m.contract(),
            1,
            &[symbol("control")],
            &[symbol("save")],
            &saved,
        )
        .unwrap();
        let (p, n) = HOST
            .prepare(
                session,
                1,
                &m.contract(),
                &template,
                &Data("fresh value"),
                region.then_some(surrounding.as_slice()),
                &mut wire,
            )
            .unwrap();
        let displayed = Document::decode(&wire[..n]).unwrap();
        let index = displayed.nodes[..displayed.len as usize]
            .iter()
            .position(|n| n.id == symbol("value"))
            .unwrap();
        assert_eq!(displayed.text_of(index), "recorded value");
        id.revision = 2;
        HOST.commit(p, id).unwrap();
        // A saved definition instead invokes current bindings on reopening.
        HOST.reset_definition(session).unwrap();
        h.kind = artifact::DEFINITION;
        saved.resize(artifact::MAX_BYTES, 0);
        let n = HOST
            .save_artifact(session, &template, h, &[], &[], &mut saved)
            .unwrap();
        saved.truncate(n);
        HOST.open_artifact(session, &m.contract(), 1, &[], &[], &saved)
            .unwrap();
        let (p, n) = HOST
            .prepare(
                session,
                2,
                &m.contract(),
                &template,
                &Data("fresh authorized value"),
                region.then_some(surrounding.as_slice()),
                &mut wire,
            )
            .unwrap();
        let displayed = Document::decode(&wire[..n]).unwrap();
        let index = displayed.nodes[..displayed.len as usize]
            .iter()
            .position(|n| n.id == symbol("value"))
            .unwrap();
        assert_eq!(displayed.text_of(index), "fresh authorized value");
        HOST.discard(p).unwrap();
        HOST.close(session).unwrap();
    }
}

#[test]
fn report_requests_are_confined_to_slots_without_names_or_ordinary_file_authority() {
    use osaiku_runtime::files::*;
    for op in operation::REPORT_INFO..=operation::REPORT_ABORT {
        assert!(report_operation(op));
        assert!(!private_operation(op));
        assert!(!broker_operation(op));
        let mut r = FileRequest {
            operation: op,
            sequence: 1,
            parent: 0,
            ..FileRequest::EMPTY
        };
        assert!(r.canonical());
        r.parent = 16;
        assert!(!r.canonical());
        r.parent = 0;
        r.name_len = 1;
        r.name[0] = b'x';
        assert!(!r.canonical());
    }
    assert_eq!(&artifact::storage::name(0).unwrap(), b"report-1.osa-def1");
    assert_eq!(&artifact::storage::name(15).unwrap(), b"report-8.osa-res1");
    assert!(artifact::storage::name(16).is_none());
}

#[test]
fn canceled_and_mixed_version_downloads_wipe_partial_bytes() {
    use osaiku_runtime::{
        files::{operation as op, status, FileReply},
        report::metadata::Scope,
    };
    let mut bytes = vec![9; artifact::MAX_BYTES];
    let scope = Scope {
        window: 2,
        incarnation: 3,
    };
    let mut reads = 0;
    let result = artifact::storage::run(
        scope,
        0,
        None,
        &mut bytes,
        || false,
        |r| {
            let mut p = FileReply {
                sequence: r.sequence,
                status: status::OK,
                mount_generation: 1,
                object: 2,
                object_version: 1,
                kind: 1,
                capacity: 600,
                ..FileReply::EMPTY
            };
            if r.operation == op::REPORT_READ {
                reads += 1;
                p.content_len = 256;
                p.content.fill(42);
                if reads == 2 {
                    p.object_version = 2;
                }
            }
            p
        },
    );
    assert_eq!(result, Err(status::STALE));
    assert!(bytes.iter().all(|b| *b == 0));
    bytes.fill(9);
    let result = artifact::storage::run(
        scope,
        0,
        None,
        &mut bytes,
        || true,
        |_| panic!("canceled read must not access storage"),
    );
    assert_eq!(result, Err(artifact::storage::CANCELED));
    assert!(bytes.iter().all(|b| *b == 0));
}

#[test]
fn lost_commit_reply_is_unknown_and_never_retried() {
    use osaiku_runtime::{
        files::{operation as op, status, FileReply},
        report::metadata::Scope,
    };
    let mut bytes = vec![0; artifact::HEADER + 400];
    bytes[artifact::HEADER..].fill(7);
    let n = header(1).seal(&mut bytes, 400).unwrap();
    let mut commits = 0;
    let mut aborts = 0;
    let result = artifact::storage::run(
        Scope {
            window: 2,
            incarnation: 3,
        },
        0,
        Some(n),
        &mut bytes,
        || false,
        |r| {
            let mut p = FileReply {
                status: status::OK,
                sequence: r.sequence,
                mount_generation: 1,
                ..FileReply::EMPTY
            };
            match r.operation {
                op::REPORT_INFO => p.status = status::END,
                op::REPORT_BEGIN => p.object = 1,
                op::REPORT_COMMIT => {
                    commits += 1;
                    p.status = status::UNAVAILABLE;
                }
                op::REPORT_ABORT => aborts += 1,
                op::REPORT_CHUNK => (),
                _ => panic!("unexpected operation"),
            }
            p
        },
    );
    assert_eq!(result, Err(status::UNKNOWN_COMMIT));
    assert_eq!(commits, 1);
    assert_eq!(aborts, 1);
}

#[test]
fn fresh_publication_does_not_inherit_a_previous_reports_filter_text() {
    use osaiku_ui::report::host::Host;
    let doc = compile(
        r#"<ui-text-input id="filter" action="filter" bind:text="label">recorded</ui-text-input>"#,
        "",
    )
    .unwrap();
    // Heap-backed native host budgets are not relevant to this codec/state test.
    std::thread::Builder::new()
        .stack_size(8 * 1024 * 1024)
        .spawn(move || {
            let manifest = format!(
                "report 1 {}\nsource sample record 1\nfield label sample text\naction filter filter",
                osaiku_ui::VERSION
            );
            let m = parse_manifest(&manifest).unwrap();
            let mut host = Host::EMPTY;
            let session = host.open(9, 1, Surface::Window).unwrap();
            let p = host.prepare(session, 0, &m.contract(), &doc, None).unwrap();
            let id = header(1).recorded;
            host.commit(p, id).unwrap();
            host.set_filter(session, id, symbol("filter"), "edited elsewhere")
                .unwrap();
            let p = host
                .prepare_fresh(session, 1, &m.contract(), &doc, None)
                .unwrap();
            assert_eq!(host.prepared(p).unwrap().text_of(0), "recorded");
        })
        .unwrap()
        .join()
        .unwrap();
}
