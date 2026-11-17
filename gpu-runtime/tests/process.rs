use osaiku_gpu_runtime::{
    process::{Process, Transport},
    protocol::control::{op, Message},
};
use std::{cell::RefCell, rc::Rc};
struct Channel {
    role: u64,
    corrupt: Rc<RefCell<bool>>,
    calls: Rc<RefCell<Vec<Message>>>,
}
impl Transport for Channel {
    fn exchange(&mut self, request: Message) -> Result<Message, isize> {
        self.calls.borrow_mut().push(request);
        if request.operation == op::IDENTITY {
            return Ok(Message {
                attempt: 7,
                selection: 8,
                device: 9,
                connection: 10,
                role: self.role,
                digest: [42; 32],
                ..request
            });
        }
        Ok(if *self.corrupt.borrow() {
            Message {
                connection: request.connection + 1,
                ..request
            }
        } else {
            request
        })
    }
}
#[test]
fn scope_is_bound_once_and_changed_reply_poisoning_prevents_retries() {
    let calls = Rc::new(RefCell::new(Vec::new()));
    let corrupt = Rc::new(RefCell::new(false));
    let mut client = Process::connect(Channel {
        role: 4,
        corrupt: corrupt.clone(),
        calls: calls.clone(),
    })
    .unwrap();
    client.heartbeat().unwrap();
    client.fence(3).unwrap();
    client.presented(5, 3).unwrap();
    for c in calls.borrow().iter().skip(1) {
        assert_eq!(
            (
                c.attempt,
                c.selection,
                c.device,
                c.connection,
                c.role,
                c.digest
            ),
            (7, 8, 9, 10, 4, [42; 32])
        );
    }
    *corrupt.borrow_mut() = true;
    assert_eq!(client.heartbeat(), Err(-5));
    let before = calls.borrow().len();
    assert_eq!(client.heartbeat(), Err(-5));
    assert_eq!(client.fail(), Err(-5));
    assert_eq!(calls.borrow().len(), before);
}
#[test]
fn renderer_cannot_report_backend_health_and_failure_is_terminal() {
    let calls = Rc::new(RefCell::new(Vec::new()));
    let mut client = Process::connect(Channel {
        role: 5,
        corrupt: Rc::new(RefCell::new(false)),
        calls: calls.clone(),
    })
    .unwrap();
    assert_eq!(client.fence(1), Err(-8));
    assert_eq!(client.presented(1, 1), Err(-8));
    assert_eq!(client.stopped(), Err(-8));
    assert_eq!(unsafe { client.firmware_aborted() }, Err(-8));
    assert_eq!(calls.borrow().len(), 1);
    client.heartbeat().unwrap();
    client.fail().unwrap();
    assert_eq!(client.heartbeat(), Err(-5));
    assert_eq!(calls.borrow().len(), 3);
}
#[test]
fn changed_drain_identity_is_terminal_and_never_acknowledged_as_stopped() {
    let calls = Rc::new(RefCell::new(Vec::new()));
    let corrupt = Rc::new(RefCell::new(false));
    let mut client = Process::connect(Channel {
        role: 4,
        corrupt: corrupt.clone(),
        calls: calls.clone(),
    })
    .unwrap();
    assert_eq!(client.draining(), Ok(false));
    *corrupt.borrow_mut() = true;
    assert_eq!(client.draining(), Err(-5));
    let before = calls.borrow().len();
    assert_eq!(client.stopped(), Err(-5));
    assert_eq!(calls.borrow().len(), before);
}
#[test]
fn firmware_abort_uses_bound_identity_and_corrupt_receipt_prevents_failure_retry() {
    for bad in [false, true] {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let corrupt = Rc::new(RefCell::new(false));
        let mut client = Process::connect(Channel {
            role: 4,
            corrupt: corrupt.clone(),
            calls: calls.clone(),
        })
        .unwrap();
        *corrupt.borrow_mut() = bad;
        let result = unsafe { client.firmware_aborted() };
        assert_eq!(result, if bad { Err(-5) } else { Ok(()) });
        assert_eq!(
            calls.borrow()[1],
            Message {
                operation: op::FIRMWARE_ABORTED,
                ..client.identity()
            }
        );
        if bad {
            assert_eq!(client.fail(), Err(-5));
            assert_eq!(calls.borrow().len(), 2);
        } else {
            client.fail().unwrap();
            assert_eq!(calls.borrow()[2].operation, op::FAILURE);
        }
    }
}
