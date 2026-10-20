use osaiku_runtime::report::{ReportView, Surface};
use osaiku_ui::{
    compiler::compile,
    report::{host::Error as HostError, parse_manifest},
    *,
};
use std::cell::Cell;

#[test]
fn provider_calls_are_guarded_and_refused_encoding_does_not_commit_or_poison_the_host() {
    static HOST: ReportView = ReportView::new();
    let session = HOST.open(9, 1, Surface::Window).unwrap();
    let manifest =
        parse_manifest("report 1 11\nsource sample record 1\nfield label sample text").unwrap();
    let template = compile(r#"<span id="root" bind:text="label"></span>"#, "").unwrap();
    let mut source = vec![0; MAX_WIRE];
    let len = template.encode(&mut source).unwrap();
    let source = &source[..len];
    let calls = Cell::new(0);
    struct Model<'a>(&'a Cell<usize>, osaiku_runtime::report::Session);
    impl Bindings for Model<'_> {
        fn get(&self, _: u32) -> Option<Value<'_>> {
            self.0.set(self.0.get() + 1);
            assert_eq!(HOST.revision(self.1), Err(HostError::Busy));
            Some(Value::Text("<literal>"))
        }
    }
    let model = Model(&calls, session);
    let mut output = vec![0; MAX_WIRE];
    assert_eq!(
        HOST.prepare(
            session,
            1,
            &manifest.contract(),
            source,
            &model,
            None,
            &mut output
        ),
        Err(HostError::Stale)
    );
    assert_eq!(calls.get(), 0);
    assert!(HOST
        .prepare(
            session,
            0,
            &manifest.contract(),
            source,
            &model,
            None,
            &mut output[..1]
        )
        .is_err());
    assert_eq!(HOST.revision(session), Ok(0));
    let (token, len) = HOST
        .prepare(
            session,
            0,
            &manifest.contract(),
            source,
            &model,
            None,
            &mut output,
        )
        .unwrap();
    assert_eq!(
        Document::decode(&output[..len]).unwrap().text_of(0),
        "<literal>"
    );
    let before = calls.get();
    assert_eq!(
        HOST.prepare(
            session,
            0,
            &manifest.contract(),
            source,
            &model,
            None,
            &mut output
        ),
        Err(HostError::Busy)
    );
    assert_eq!(calls.get(), before);
    HOST.discard(token).unwrap();
    let (token, _) = HOST
        .prepare(
            session,
            0,
            &manifest.contract(),
            source,
            &model,
            None,
            &mut output,
        )
        .unwrap();
    HOST.commit(
        token,
        Identity {
            epoch: 1,
            owner: 9,
            window: 4,
            incarnation: 1,
            revision: 1,
        },
    )
    .unwrap();
    HOST.close(session).unwrap();
    let next = HOST.open(9, 1, Surface::Window).unwrap();
    assert_eq!(HOST.revision(next), Ok(0));
    assert_eq!(HOST.encode(token, &mut output), Err(HostError::Owner));
    HOST.close(next).unwrap();
}
