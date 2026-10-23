//! Authorization/serialization only; no controls, layout or UI interaction.
use osaiku_runtime::file_broker::*;
use osaiku_ui::{input::Event, Identity};
fn receipt() -> Receipt {
    Receipt {
        token: 8,
        event: Event {
            target: Identity {
                owner: 91,
                window: 2,
                incarnation: 3,
                revision: 4,
                epoch: 5,
            },
            ..Event::EMPTY
        },
        binding: Binding {
            element: 7,
            operations: [1, 40, 41],
            object: 23,
            version: 9,
            ..Binding::EMPTY
        },
        time_ms: 100,
        ..Receipt::EMPTY
    }
}
fn command() -> Command {
    let r = receipt();
    Command {
        token: r.token,
        target: r.event.target,
        operation: 1,
        object: 23,
        version: 9,
    }
}
#[test]
fn scope_is_exact_and_cannot_be_replayed_renewed_or_overwritten() {
    let mut slot = ReceiptSlot::EMPTY;
    assert!(slot.insert(receipt(), 100));
    assert!(!slot.insert(
        Receipt {
            token: 9,
            ..receipt()
        },
        100
    ));
    assert!(slot.get(92, 8, 101).is_none());
    assert!(slot.get(91, 8, 99).is_none());
    for change in 0..9 {
        let mut c = command();
        match change {
            0 => c.target.owner += 1,
            1 => c.target.window += 1,
            2 => c.target.incarnation += 1,
            3 => c.target.revision += 1,
            4 => c.target.epoch += 1,
            5 => c.token += 1,
            6 => c.object += 1,
            7 => c.version += 1,
            _ => c.operation = 46,
        }
        assert!(!slot.consume(91, &c, 101));
    }
    assert!(slot.consume(91, &command(), 101));
    assert!(!slot.consume(91, &command(), 101));
    assert!(!slot.insert(receipt(), 100));
    assert!(slot.insert(
        Receipt {
            token: 9,
            time_ms: 200,
            ..receipt()
        },
        200
    ));
    assert!(slot.get(91, 9, 2199).is_some());
    assert!(slot.get(91, 9, 2200).is_none());
}
#[test]
fn map_does_not_authorize_objects_outside_the_published_inventory() {
    use osaiku_runtime::{file_manager::*, files};
    let mut d = osaiku_ui::Document::EMPTY;
    d.len = 1;
    d.nodes[0].id = 7;
    let mut map = Map::EMPTY;
    assert!(map.push(receipt().binding));
    assert!(!map.push(receipt().binding));
    let mut v = View {
        magic: MAGIC,
        generation: 1,
        parent: 1,
        status: files::status::OK,
        revision: 1,
        mount_generation: 1,
        count: 1,
        total: 1,
        ..View::EMPTY
    };
    v.rows[0] = Row {
        object: 23,
        version: 9,
        kind: 1,
        name_len: 1,
        ..Row::EMPTY
    };
    v.rows[0].name[0] = b'a';
    assert!(map.valid_for(&v, &d));
    v.rows[0].version += 1;
    assert!(!map.valid_for(&v, &d));
    map.len = u64::MAX;
    assert!(!map.valid_for(&v, &d));
}
