use osaiku_ui::{editing::*, Error};

#[test]
fn utf8_selection_edits_are_bounded_and_scrub_removed_bytes() {
    let mut bytes = [0; 16];
    let len = append(&mut bytes, 0, "zażółć").unwrap();
    let (len, selection) = replace_selection(
        &mut bytes,
        len,
        Selection {
            anchor: 2,
            caret: 8,
        },
        "a",
    )
    .unwrap();
    assert_eq!(core::str::from_utf8(&bytes[..len]).unwrap(), "zaać");
    assert_eq!(selection, Selection::at(3));
    let len = backspace(&mut bytes, len).unwrap();
    assert_eq!(&bytes[..len], b"zaa");
    assert!(bytes[len..].iter().all(|b| *b == 0));
    assert_eq!(replace(&mut bytes, len, "").unwrap(), 0);
    assert_eq!(bytes, [0; 16]);
}

#[test]
fn invalid_boundaries_and_capacity_refuse_without_mutation() {
    let mut bytes = [0; 4];
    let len = append(&mut bytes, 0, "ąb").unwrap();
    let saved = bytes;
    assert_eq!(
        replace_selection(&mut bytes, len, Selection::at(1), "c"),
        Err(Error::Text)
    );
    assert_eq!(
        replace_selection(&mut bytes, len, Selection::at(9), "c"),
        Err(Error::Text)
    );
    assert_eq!(append(&mut bytes, len, "cd"), Err(Error::Limit));
    assert_eq!(bytes, saved);
    assert_eq!(backspace(&mut [0; 4], 0), Ok(0));
}

#[cfg(feature = "compiler")]
#[test]
fn text_control_shares_native_input_layout_and_semantics() {
    use osaiku_ui::{compiler::compile, input, paint::Scene, symbol, Action, Role};
    let mut document = compile(
        "<main id=\"root\"><ui-text-input id=\"entry\" action=\"edit\">Zażółć</ui-text-input></main>",
        "ui-text-input { height: 40px; padding: 4px; }",
    ).unwrap();
    assert!(document.set_focus(symbol("entry")));
    let scene = Scene::from_document(&document, 320, 200).unwrap();
    let entry = scene.semantics().nth(1).unwrap();
    assert_eq!(entry.role, Role::TextBox);
    assert_eq!(entry.text, "Zażółć");
    let event = input::key_event(
        &document,
        &scene,
        Action::EMPTY.target,
        input::Key::Activate,
    );
    // An empty owner identity cannot acquire authority through a control.
    assert!(event.is_none());
    assert_eq!(scene.hit(10, 10), Some(symbol("entry")));
}
