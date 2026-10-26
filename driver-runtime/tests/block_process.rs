use osaiku_driver_runtime::{block_process::Channel, protocol::Error};
#[test]
fn process_loss_rejects_late_step_and_preserves_receipt_sequence() {
    let mut c = Channel::new();
    c.post([1; 64]).unwrap();
    let (old, _) = c.fetch().unwrap().unwrap();
    c.lost();
    assert_eq!(c.reply(old, [2; 64]), Err(Error::Stale));
    c.rebind();
    c.post([3; 64]).unwrap();
    let (new, _) = c.fetch().unwrap().unwrap();
    assert!(new > old);
    assert_eq!(c.reply(old, [2; 64]), Err(Error::Stale));
    c.reply(new, [4; 64]).unwrap();
    assert_eq!(c.take().unwrap(), Some([4; 64]));
    assert_eq!(c.reply(new, [4; 64]), Err(Error::Stale));
}
#[test]
fn one_pending_step_and_bounded_owned_copy() {
    let mut c = Channel::new();
    let mut b = [7; 64];
    c.post(b).unwrap();
    b.fill(0);
    assert_eq!(c.post(b), Err(Error::Backpressure));
    let (id, event) = c.fetch().unwrap().unwrap();
    assert_eq!(event, [7; 64]);
    assert!(c.fetch().unwrap().is_none());
    assert_eq!(c.post(b), Err(Error::Backpressure));
    c.reply(id, [8; 64]).unwrap();
    assert_eq!(c.post(b), Err(Error::Backpressure));
    assert_eq!(c.take().unwrap(), Some([8; 64]));
    c.post(b).unwrap();
}
